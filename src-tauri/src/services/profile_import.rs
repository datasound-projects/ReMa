//! Reads profile details from a document's extracted text.
//!
//! Contact details and links are found deterministically. When a model is
//! configured, it reads the rest (title, experience, skills, …) as JSON.
//! Facts that can be checked against the text (name, email, phone, links)
//! are dropped if the text does not contain them. The result is only a
//! proposal: the user reviews it before anything is saved.

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::{
    db::profile as repo,
    error::{AppError, AppResult},
    jobs::extract::json_object,
    llm::{ChatRequest, Finish, Turn},
    models::{
        chat::MessageRole,
        profile::{Education, Experience, Language, Profile, ProfileImport, ProfileLink},
        provider::ModelRef,
    },
    services::{profile::normalize_url, providers},
    state::AppState,
};

/// Characters of CV text sent to the model.
const MAX_MODEL_INPUT: usize = 30_000;

pub async fn import(state: &AppState, document_id: i64) -> AppResult<ProfileImport> {
    let (document, text) = state.db.call(|c| {
        Ok((
            repo::get_document(c, document_id)?,
            repo::document_text(c, document_id)?,
        ))
    })?;
    let Some(text) = text else {
        return Err(AppError::validation(
            "This document has no readable text (for example a scanned PDF or an image), \
             so ReMa cannot import it. You can still fill in your profile manually.",
        ));
    };

    let mut notes = Vec::new();
    let mut extracted = deterministic(&text);
    let mut model_name = None;

    match providers::catalog(state)?.default_model {
        Some(model) => match read_with_model(state, &model, &text).await {
            Ok(read) => {
                model_name = Some(model.model_id.clone());
                let (verified, dropped) = verify(read, &text);
                notes.extend(dropped);
                extracted = merge(verified, extracted);
            }
            Err(error) => notes.push(format!(
                "The model could not read the document ({error}). Only contact details and \
                 links were imported."
            )),
        },
        None => notes.push(
            "No model is set up, so only contact details and links were imported. Connect a \
             model in Settings to import experience, education and skills too."
                .into(),
        ),
    }

    Ok(ProfileImport {
        document,
        extracted: tidy(extracted),
        model: model_name,
        notes,
    })
}

// ── Deterministic extraction ────────────────────────────────────────

fn trim_punctuation(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';' | '"' | '\'' | '|'
        ) || (c == '.' || c == ':')
    })
}

fn is_email(token: &str) -> bool {
    let Some((local, domain)) = token.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.ends_with('.')
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._%+-".contains(c))
}

/// URL-like tokens with a scheme or a `www.` / known-site prefix.
fn urls(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for raw in text.split_whitespace() {
        let token = trim_punctuation(raw);
        let lower = token.to_lowercase();
        let looks_like_url = lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("www.")
            || lower.starts_with("github.com/")
            || lower.starts_with("linkedin.com/")
            || lower.contains(".linkedin.com/");
        if looks_like_url && !token.contains('@') {
            if let Ok(url) = normalize_url(token, "Link") {
                if !found.contains(&url) {
                    found.push(url);
                }
            }
        }
    }
    found
}

fn phone(text: &str) -> Option<String> {
    for line in text.lines() {
        let lower = line.to_lowercase();
        // Candidate runs of phone characters, e.g. "+43 660 123 4567".
        let mut run = String::new();
        let mut candidates = Vec::new();
        for c in line.chars().chain(std::iter::once('\n')) {
            if c.is_ascii_digit()
                || (!run.is_empty() && " +-()./".contains(c))
                || (run.is_empty() && c == '+')
            {
                run.push(c);
            } else {
                candidates.push(std::mem::take(&mut run));
            }
        }
        for candidate in candidates {
            let candidate = candidate.trim().trim_end_matches(['-', '.', '/', '(']);
            let digits = candidate.chars().filter(char::is_ascii_digit).count();
            let labelled = ["phone", "tel", "mobile", "handy", "telefon"]
                .iter()
                .any(|l| lower.contains(l));
            // Years ("2019 - 2023") and dates are not phone numbers.
            if (8..=15).contains(&digits) && (candidate.starts_with('+') || labelled) {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

/// Contact details and links found without a model.
pub fn deterministic(text: &str) -> Profile {
    let email = text
        .split_whitespace()
        .map(trim_punctuation)
        .find(|t| is_email(t))
        .map(|t| t.trim_start_matches("mailto:").to_string())
        .unwrap_or_default();
    let mut profile = Profile {
        email,
        phone: phone(text).unwrap_or_default(),
        ..Profile::default()
    };
    for url in urls(text) {
        let host = reqwest::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_lowercase))
            .unwrap_or_default();
        if host.ends_with("github.com") && profile.github.is_empty() {
            profile.github = url;
        } else if host.ends_with("linkedin.com") && profile.linkedin.is_empty() {
            profile.linkedin = url;
        } else {
            let label = host.trim_start_matches("www.").to_string();
            profile.other_links.push(ProfileLink { label, url });
        }
    }
    profile
}

// ── Model extraction ────────────────────────────────────────────────

const RULES: &str = r#"You read a CV/resume for ReMa and return the person's profile as JSON.

Rules:
- Use only what the document states. Never invent or guess anything.
- Copy names, email, phone and links exactly as written.
- Keep descriptions short (at most three sentences each), in the document's language.
- "skills": individual skills or technologies, short names ("Python", "Kubernetes").
- Dates as written ("2021", "03/2021", "March 2021"). "current": true if the position is ongoing.
- Use "" for anything the document does not state.

Return JSON only:
{"first_name": "", "last_name": "", "email": "", "phone": "", "location": "",
 "title": "professional title", "summary": "", "skills": [""],
 "experience": [{"title": "", "company": "", "location": "", "start": "", "end": "", "current": false, "description": ""}],
 "education": [{"school": "", "degree": "", "field": "", "start": "", "end": "", "description": ""}],
 "languages": [{"name": "", "level": ""}],
 "website": "", "github": "", "linkedin": "",
 "other_links": [{"label": "", "url": ""}]}"#;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawProfile {
    first_name: String,
    last_name: String,
    email: String,
    phone: String,
    location: String,
    title: String,
    summary: String,
    skills: Vec<String>,
    experience: Vec<RawExperience>,
    education: Vec<RawEducation>,
    languages: Vec<RawLanguage>,
    website: String,
    github: String,
    linkedin: String,
    other_links: Vec<RawLink>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawExperience {
    title: String,
    company: String,
    location: String,
    start: String,
    end: String,
    current: bool,
    description: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawEducation {
    school: String,
    degree: String,
    field: String,
    start: String,
    end: String,
    description: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawLanguage {
    name: String,
    level: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawLink {
    label: String,
    url: String,
}

async fn read_with_model(state: &AppState, model: &ModelRef, text: &str) -> AppResult<Profile> {
    let endpoint = providers::resolve_endpoint(state, &model.provider_id).await?;
    let request = ChatRequest {
        system: Some(RULES.into()),
        turns: vec![Turn {
            role: MessageRole::User,
            content: format!(
                "CV text:\n\n{}",
                text.chars().take(MAX_MODEL_INPUT).collect::<String>()
            ),
        }],
        max_output_tokens: Some(8_000),
        ..ChatRequest::default()
    };
    let mut answer = String::new();
    let mut sink = |delta: &str| answer.push_str(delta);
    let finish = state
        .llm
        .stream_chat(
            &endpoint,
            &model.model_id,
            &request,
            CancellationToken::new(),
            &mut sink,
        )
        .await?;
    if finish == Finish::Cancelled {
        return Err(AppError::validation("The import was cancelled."));
    }
    parse(&answer)
}

fn parse(answer: &str) -> AppResult<Profile> {
    let raw: RawProfile = serde_json::from_str(json_object(answer)?)
        .map_err(|_| AppError::provider("the answer was not in the expected format"))?;
    Ok(Profile {
        first_name: raw.first_name,
        last_name: raw.last_name,
        email: raw.email,
        phone: raw.phone,
        location: raw.location,
        title: raw.title,
        summary: raw.summary,
        skills: raw.skills,
        experience: raw
            .experience
            .into_iter()
            .map(|e| Experience {
                title: e.title,
                company: e.company,
                location: e.location,
                start: e.start,
                end: e.end,
                current: e.current,
                description: e.description,
            })
            .collect(),
        education: raw
            .education
            .into_iter()
            .map(|e| Education {
                school: e.school,
                degree: e.degree,
                field: e.field,
                start: e.start,
                end: e.end,
                description: e.description,
            })
            .collect(),
        languages: raw
            .languages
            .into_iter()
            .map(|l| Language {
                name: l.name,
                level: l.level,
            })
            .collect(),
        website: raw.website,
        github: raw.github,
        linkedin: raw.linkedin,
        other_links: raw
            .other_links
            .into_iter()
            .map(|l| ProfileLink {
                label: l.label,
                url: l.url,
            })
            .collect(),
        ..Profile::default()
    })
}

// ── Verification ────────────────────────────────────────────────────

fn simplify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// Host + path without scheme, `www.` or trailing slash.
fn url_key(url: &str) -> String {
    let lower = url.trim().to_lowercase();
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    without_scheme
        .trim_start_matches("www.")
        .trim_end_matches('/')
        .to_string()
}

/// Drops checkable facts the text does not contain. Returns notes.
fn verify(mut profile: Profile, text: &str) -> (Profile, Vec<String>) {
    let plain = simplify(text);
    let text_digits = digits(text);
    let lower = text.to_lowercase();
    let text_urls: Vec<String> = urls(text).iter().map(|u| url_key(u)).collect();
    let mut notes = Vec::new();

    let name_ok = |name: &str| {
        let name = simplify(name);
        name.is_empty() || format!(" {plain} ").contains(&format!(" {name} "))
    };
    if !name_ok(&profile.first_name) || !name_ok(&profile.last_name) {
        notes.push("The name the model read does not appear in the document; left empty.".into());
        profile.first_name.clear();
        profile.last_name.clear();
    }
    if !profile.email.is_empty() && !lower.contains(&profile.email.trim().to_lowercase()) {
        notes.push("Ignored an email address that is not in the document.".into());
        profile.email.clear();
    }
    let phone_digits = digits(&profile.phone);
    if !profile.phone.is_empty() && (phone_digits.len() < 5 || !text_digits.contains(&phone_digits))
    {
        notes.push("Ignored a phone number that is not in the document.".into());
        profile.phone.clear();
    }
    let url_ok = |url: &str| {
        let key = url_key(url);
        key.is_empty() || text_urls.contains(&key) || lower.contains(&key)
    };
    for (field, value) in [
        ("website", &mut profile.website),
        ("GitHub", &mut profile.github),
        ("LinkedIn", &mut profile.linkedin),
    ] {
        if !url_ok(value) {
            notes.push(format!(
                "Ignored a {field} link that is not in the document."
            ));
            value.clear();
        }
    }
    let before = profile.other_links.len();
    profile.other_links.retain(|l| url_ok(&l.url));
    if profile.other_links.len() < before {
        notes.push("Ignored links that are not in the document.".into());
    }
    (profile, notes)
}

/// Model values first; deterministic contact details fill the gaps.
fn merge(model: Profile, found: Profile) -> Profile {
    let pick = |a: String, b: String| if a.trim().is_empty() { b } else { a };
    let mut other_links = model.other_links;
    for link in found.other_links {
        let key = url_key(&link.url);
        let known = other_links.iter().any(|l| url_key(&l.url) == key)
            || [&model.website, &model.github, &model.linkedin]
                .iter()
                .any(|u| url_key(u) == key);
        if !known {
            other_links.push(link);
        }
    }
    Profile {
        email: pick(model.email, found.email),
        phone: pick(model.phone, found.phone),
        github: pick(model.github, found.github),
        linkedin: pick(model.linkedin, found.linkedin),
        other_links,
        ..model
    }
}

fn cut(value: &str, max: usize) -> String {
    value.trim().chars().take(max).collect()
}

/// Bounded, cleaned values. Invalid links are dropped rather than failing,
/// so the proposal always passes `profile::save` once the user accepts it.
fn tidy(p: Profile) -> Profile {
    let url = |u: &str| normalize_url(u, "Link").unwrap_or_default();
    let one_line =
        |v: &str, max: usize| cut(&v.split_whitespace().collect::<Vec<_>>().join(" "), max);
    let phone = one_line(&p.phone, 40);
    let phone_ok = phone
        .chars()
        .all(|c| c.is_ascii_digit() || " +-()./".contains(c))
        && digits(&phone).len() >= 5;
    let email = one_line(&p.email, 254);
    Profile {
        first_name: one_line(&p.first_name, 100),
        last_name: one_line(&p.last_name, 100),
        email: if is_email(&email) {
            email
        } else {
            String::new()
        },
        phone: if phone_ok { phone } else { String::new() },
        location: one_line(&p.location, 200),
        title: one_line(&p.title, 200),
        summary: cut(&p.summary, 5_000),
        skills: p
            .skills
            .iter()
            .map(|s| one_line(s, 80))
            .filter(|s| !s.is_empty())
            .take(200)
            .collect(),
        experience: p
            .experience
            .into_iter()
            .take(100)
            .map(|e| Experience {
                title: one_line(&e.title, 200),
                company: one_line(&e.company, 200),
                location: one_line(&e.location, 200),
                start: one_line(&e.start, 40),
                end: if e.current {
                    String::new()
                } else {
                    one_line(&e.end, 40)
                },
                current: e.current,
                description: cut(&e.description, 5_000),
            })
            .collect(),
        education: p
            .education
            .into_iter()
            .take(100)
            .map(|e| Education {
                school: one_line(&e.school, 200),
                degree: one_line(&e.degree, 200),
                field: one_line(&e.field, 200),
                start: one_line(&e.start, 40),
                end: one_line(&e.end, 40),
                description: cut(&e.description, 5_000),
            })
            .collect(),
        languages: p
            .languages
            .into_iter()
            .take(100)
            .map(|l| Language {
                name: one_line(&l.name, 60),
                level: one_line(&l.level, 60),
            })
            .filter(|l| !l.name.is_empty())
            .collect(),
        website: url(&p.website),
        resume_website: url(&p.resume_website),
        github: url(&p.github),
        linkedin: url(&p.linkedin),
        other_links: p
            .other_links
            .into_iter()
            .take(100)
            .filter_map(|l| {
                let u = url(&l.url);
                (!u.is_empty()).then(|| ProfileLink {
                    label: one_line(&l.label, 80),
                    url: u,
                })
            })
            .collect(),
        custom_fields: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        llm::fake::FakeLanguageModel,
        models::provider::ProviderKind,
        services::{documents::samples, profile},
        state::testing,
    };

    const CV: &str = "Ana Tester\nData Engineer — Vienna, Austria\n\
        Email: ana.tester@example.com | Phone: +43 660 1234567\n\
        github.com/anatester · https://www.linkedin.com/in/ana-tester/ · https://ana.dev\n\
        Experience\nGlobex, Senior Data Engineer, 2021 – present\nAcme, Intern 2019 - 2020";

    #[test]
    fn finds_contact_details_and_links_without_a_model() {
        let p = deterministic(CV);
        assert_eq!(p.email, "ana.tester@example.com");
        assert_eq!(p.phone, "+43 660 1234567");
        assert_eq!(p.github, "https://github.com/anatester");
        assert_eq!(p.linkedin, "https://www.linkedin.com/in/ana-tester/");
        assert_eq!(p.other_links.len(), 1);
        assert_eq!(p.other_links[0].url, "https://ana.dev/");
        // Year ranges are not phone numbers.
        assert_eq!(deterministic("Acme 2019 - 2020").phone, "");
    }

    #[test]
    fn drops_facts_the_document_does_not_contain() {
        let read = Profile {
            first_name: "Ana".into(),
            last_name: "Tester".into(),
            email: "ana@invented.com".into(),
            phone: "+43 660 1234567".into(),
            github: "https://github.com/someone-else".into(),
            linkedin: "linkedin.com/in/ana-tester".into(),
            title: "Data Engineer".into(),
            ..Profile::default()
        };
        let (verified, notes) = verify(read, CV);
        assert_eq!(verified.first_name, "Ana");
        assert_eq!(verified.email, "");
        assert_eq!(verified.phone, "+43 660 1234567");
        assert_eq!(verified.github, "");
        assert_eq!(verified.linkedin, "linkedin.com/in/ana-tester");
        assert_eq!(notes.len(), 2);

        let (verified, _) = verify(
            Profile {
                first_name: "Maria".into(),
                ..Profile::default()
            },
            CV,
        );
        assert_eq!(verified.first_name, "", "invented names are dropped");
    }

    async fn state_with(answer: &str) -> AppState {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[answer])));
        providers::connect(&state, ProviderKind::Anthropic, "k")
            .await
            .unwrap();
        providers::set_default_model(
            &state,
            &ModelRef {
                provider_id: "anthropic".into(),
                model_id: "model-a".into(),
            },
        )
        .unwrap();
        state
    }

    async fn add_cv(state: &AppState) -> i64 {
        let path = state.data_dir.join("Ana CV.pdf");
        let lines: Vec<&str> = CV.lines().collect();
        std::fs::write(&path, samples::pdf(&lines)).unwrap();
        profile::add_document(state, &path, None).await.unwrap().id
    }

    #[tokio::test]
    async fn imports_a_pdf_with_the_model_and_proposes_a_valid_profile() {
        let answer = r#"```json
{"first_name":"Ana","last_name":"Tester","email":"ana.tester@example.com","phone":"",
 "location":"Vienna, Austria","title":"Data Engineer","summary":"Builds data platforms.",
 "skills":["Python","Kubernetes"],
 "experience":[{"title":"Senior Data Engineer","company":"Globex","start":"2021","end":"","current":true,"description":""}],
 "education":[],"languages":[{"name":"German","level":"Native"}],
 "website":"https://ana.dev","github":"","linkedin":"","other_links":[]}
```"#;
        let state = state_with(answer).await;
        let id = add_cv(&state).await;
        let import = import(&state, id).await.unwrap();
        let p = &import.extracted;
        assert_eq!(import.model.as_deref(), Some("model-a"));
        assert_eq!(p.full_name(), "Ana Tester");
        assert_eq!(p.title, "Data Engineer");
        assert_eq!(p.phone, "+43 660 1234567", "filled from the text");
        assert_eq!(p.github, "https://github.com/anatester");
        assert_eq!(p.website, "https://ana.dev/");
        assert_eq!(p.experience[0].company, "Globex");
        assert!(p.experience[0].current);

        // Nothing is saved until the user accepts.
        assert!(profile::get(&state).unwrap().profile.is_empty());
        profile::save(&state, import.extracted).expect("the proposal is valid");
    }

    #[tokio::test]
    async fn falls_back_to_contact_details_when_the_model_fails() {
        let state = state_with("not json").await;
        let id = add_cv(&state).await;
        let import = import(&state, id).await.unwrap();
        assert_eq!(import.model, None);
        assert_eq!(import.extracted.email, "ana.tester@example.com");
        assert_eq!(import.notes.len(), 1);
    }

    #[tokio::test]
    async fn images_cannot_be_imported() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let path = state.data_dir.join("certificate.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\nrest").unwrap();
        let doc = profile::add_document(&state, &path, None).await.unwrap();
        assert!(!doc.has_text);
        assert!(matches!(
            import(&state, doc.id).await,
            Err(AppError::Validation(_))
        ));
    }
}
