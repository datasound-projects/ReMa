//! The profile: validation, persistence and documents.
//!
//! The user owns the profile. Imports only propose values; everything is
//! saved through `save`, which validates and normalizes every field.

use std::path::{Path, PathBuf};

use crate::{
    db::profile::{self as repo, NewDocument},
    error::{AppError, AppResult},
    models::profile::{
        CustomField, CustomFieldKind, DocumentKind, Education, Experience, Language, Profile,
        ProfileDocument, ProfileLink, ProfileView,
    },
    services::documents,
    state::AppState,
    time::now_ms,
};

const MAX_SHORT: usize = 200;
const MAX_LONG: usize = 5_000;
const MAX_ITEMS: usize = 100;
const MAX_SKILLS: usize = 200;
const MAX_SKILL_CHARS: usize = 80;
const MAX_URL: usize = 2_000;

pub fn get(state: &AppState) -> AppResult<ProfileView> {
    state.db.call(|c| {
        let (profile, updated_at) = repo::get(c)?;
        Ok(ProfileView {
            profile,
            documents: repo::list_documents(c)?,
            updated_at,
        })
    })
}

/// Validates, normalizes and stores the whole profile.
pub fn save(state: &AppState, profile: Profile) -> AppResult<ProfileView> {
    let profile = state.db.call(|c| normalize(c, profile))?;
    state.db.call(|c| {
        let tx = c.transaction()?;
        repo::save(&tx, &profile, now_ms())?;
        tx.commit()?;
        Ok(())
    })?;
    state.events.profile_changed();
    get(state)
}

// ── Validation ──────────────────────────────────────────────────────

/// One line: trimmed, inner whitespace collapsed, length checked.
fn line(value: &str, max: usize, field: &str) -> AppResult<String> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() > max {
        return Err(AppError::validation(format!("{field} is too long.")));
    }
    Ok(value)
}

/// Multi-line text: lines trimmed, at most one blank line in a row.
fn text(value: &str, max: usize, field: &str) -> AppResult<String> {
    let mut out: Vec<&str> = Vec::new();
    for l in value.lines().map(str::trim) {
        if l.is_empty() && out.last().is_none_or(|last| last.is_empty()) {
            continue;
        }
        out.push(l);
    }
    let value = out.join("\n").trim().to_string();
    if value.chars().count() > max {
        return Err(AppError::validation(format!("{field} is too long.")));
    }
    Ok(value)
}

/// A web address, with `https://` added when missing. Empty stays empty.
pub fn normalize_url(value: &str, field: &str) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let with_scheme = if value.contains("://") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let invalid = || AppError::validation(format!("{field}: enter a web address (https://…)."));
    let url = reqwest::Url::parse(&with_scheme).map_err(|_| invalid())?;
    let host_ok = url
        .host_str()
        .is_some_and(|h| h.contains('.') || h == "localhost");
    if !matches!(url.scheme(), "http" | "https") || !host_ok || with_scheme.len() > MAX_URL {
        return Err(invalid());
    }
    Ok(url.to_string())
}

fn normalize_email(value: &str) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let valid = value.len() <= 254
        && !value.contains(char::is_whitespace)
        && value.split('@').count() == 2
        && value.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && domain.contains('.') && !domain.starts_with('.')
        });
    if !valid {
        return Err(AppError::validation("Enter a valid email address."));
    }
    Ok(value.to_string())
}

fn normalize_phone(value: &str) -> AppResult<String> {
    let value = line(value, 40, "Phone")?;
    if value.is_empty() {
        return Ok(value);
    }
    let allowed = value
        .chars()
        .all(|c| c.is_ascii_digit() || " +-()./".contains(c));
    let digits = value.chars().filter(char::is_ascii_digit).count();
    if !allowed || digits < 5 {
        return Err(AppError::validation("Enter a valid phone number."));
    }
    Ok(value)
}

fn limit<T>(items: &[T], what: &str) -> AppResult<()> {
    if items.len() > MAX_ITEMS {
        return Err(AppError::validation(format!("Too many {what}.")));
    }
    Ok(())
}

fn normalize(conn: &rusqlite::Connection, p: Profile) -> AppResult<Profile> {
    limit(&p.experience, "experience entries")?;
    limit(&p.education, "education entries")?;
    limit(&p.languages, "languages")?;
    limit(&p.other_links, "links")?;
    limit(&p.custom_fields, "custom fields")?;
    if p.skills.len() > MAX_SKILLS {
        return Err(AppError::validation("Too many skills."));
    }

    let mut skills: Vec<String> = Vec::new();
    for skill in &p.skills {
        let skill = line(skill, MAX_SKILL_CHARS, "A skill")?;
        if !skill.is_empty() && !skills.iter().any(|s| s.eq_ignore_ascii_case(&skill)) {
            skills.push(skill);
        }
    }

    let mut experience = Vec::new();
    for e in &p.experience {
        let entry = Experience {
            title: line(&e.title, MAX_SHORT, "Job title")?,
            company: line(&e.company, MAX_SHORT, "Company")?,
            location: line(&e.location, MAX_SHORT, "Location")?,
            start: line(&e.start, 40, "Start date")?,
            end: if e.current {
                String::new()
            } else {
                line(&e.end, 40, "End date")?
            },
            current: e.current,
            description: text(&e.description, MAX_LONG, "Description")?,
        };
        if entry
            != (Experience {
                current: entry.current,
                ..Experience::default()
            })
        {
            experience.push(entry);
        }
    }

    let mut education = Vec::new();
    for e in &p.education {
        let entry = Education {
            school: line(&e.school, MAX_SHORT, "School")?,
            degree: line(&e.degree, MAX_SHORT, "Degree")?,
            field: line(&e.field, MAX_SHORT, "Field of study")?,
            start: line(&e.start, 40, "Start date")?,
            end: line(&e.end, 40, "End date")?,
            description: text(&e.description, MAX_LONG, "Description")?,
        };
        if entry != Education::default() {
            education.push(entry);
        }
    }

    let mut languages = Vec::new();
    for l in &p.languages {
        let entry = Language {
            name: line(&l.name, 60, "Language")?,
            level: line(&l.level, 60, "Language level")?,
        };
        if !entry.name.is_empty() {
            languages.push(entry);
        }
    }

    let mut other_links = Vec::new();
    for l in &p.other_links {
        let url = normalize_url(&l.url, "Link")?;
        if url.is_empty() {
            continue;
        }
        other_links.push(ProfileLink {
            label: line(&l.label, 80, "Link name")?,
            url,
        });
    }

    let mut custom_fields = Vec::new();
    for f in &p.custom_fields {
        let label = line(&f.label, 80, "Field name")?;
        let field = match f.kind {
            CustomFieldKind::Text => CustomField {
                value: text(&f.value, 2_000, &label)?,
                document_id: None,
                ..f.clone()
            },
            CustomFieldKind::Url => CustomField {
                value: normalize_url(&f.value, &label)?,
                document_id: None,
                ..f.clone()
            },
            CustomFieldKind::File => {
                let document_id = match f.document_id {
                    Some(id) if repo::document_exists(conn, id)? => Some(id),
                    Some(_) => return Err(AppError::validation("That document no longer exists.")),
                    None => None,
                };
                CustomField {
                    value: String::new(),
                    document_id,
                    ..f.clone()
                }
            }
        };
        let blank = field.value.is_empty() && field.document_id.is_none();
        if label.is_empty() && !blank {
            return Err(AppError::validation("Give every custom field a name."));
        }
        if !label.is_empty() {
            custom_fields.push(CustomField { label, ..field });
        }
    }

    Ok(Profile {
        first_name: line(&p.first_name, 100, "First name")?,
        last_name: line(&p.last_name, 100, "Last name")?,
        email: normalize_email(&p.email)?,
        phone: normalize_phone(&p.phone)?,
        location: line(&p.location, MAX_SHORT, "Location")?,
        title: line(&p.title, MAX_SHORT, "Professional title")?,
        summary: text(&p.summary, MAX_LONG, "Summary")?,
        skills,
        experience,
        education,
        languages,
        website: normalize_url(&p.website, "Website")?,
        resume_website: normalize_url(&p.resume_website, "Resume website")?,
        github: normalize_url(&p.github, "GitHub")?,
        linkedin: normalize_url(&p.linkedin, "LinkedIn")?,
        other_links,
        custom_fields,
    })
}

// ── Documents ───────────────────────────────────────────────────────

pub fn documents_folder(state: &AppState) -> PathBuf {
    documents::folder(&state.data_dir)
}

/// A kind suggested by the file name.
fn guess_kind(name: &str) -> DocumentKind {
    let name = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&[
        "certificate",
        "certification",
        "zertifikat",
        "zeugnis",
        "cert",
    ]) {
        DocumentKind::Certificate
    } else if has(&["portfolio"]) {
        DocumentKind::Portfolio
    } else if has(&["cv", "resume", "résumé", "lebenslauf", "curriculum"]) {
        DocumentKind::Cv
    } else {
        DocumentKind::Other
    }
}

/// Validates a user-chosen file, stores a copy and extracts its text.
pub async fn add_document(
    state: &AppState,
    source: &Path,
    kind: Option<DocumentKind>,
) -> AppResult<ProfileDocument> {
    let source = source.to_path_buf();
    let folder = documents_folder(state);
    // Parsing is CPU-bound; keep it off the async runtime threads.
    let (original_name, format, file_name, size, sha, text) =
        tokio::task::spawn_blocking(move || -> AppResult<_> {
            let (original_name, bytes) = documents::read_source(&source)?;
            let format = documents::detect_format(&original_name, &bytes)?;
            let text = documents::extract_text(format, &bytes)?;
            let file_name = documents::generated_file_name(format)?;
            documents::store(&folder, &file_name, &bytes)?;
            Ok((
                original_name,
                format,
                file_name,
                bytes.len() as i64,
                documents::sha256_hex(&bytes),
                text,
            ))
        })
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;

    let name = Path::new(&original_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Document")
        .chars()
        .take(120)
        .collect::<String>();
    let kind = kind.unwrap_or_else(|| guess_kind(&original_name));
    let inserted = state.db.call(|c| {
        repo::insert_document(
            c,
            NewDocument {
                name: &name,
                kind,
                format,
                original_name: &original_name,
                file_name: &file_name,
                size,
                sha256: &sha,
                text: text.as_deref(),
                now: now_ms(),
            },
        )
    });
    if inserted.is_err() {
        // Do not leave an orphaned copy behind.
        let _ = std::fs::remove_file(documents_folder(state).join(&file_name));
    }
    let document = inserted?;
    state.events.profile_changed();
    Ok(document)
}

pub fn update_document(
    state: &AppState,
    id: i64,
    name: &str,
    kind: DocumentKind,
) -> AppResult<ProfileDocument> {
    let name = line(name, 120, "Document name")?;
    if name.is_empty() {
        return Err(AppError::validation("Give the document a name."));
    }
    let document = state.db.call(|c| {
        repo::update_document(c, id, &name, kind, now_ms())?;
        repo::get_document(c, id)
    })?;
    state.events.profile_changed();
    Ok(document)
}

/// Removes the document and its stored file.
pub fn delete_document(state: &AppState, id: i64) -> AppResult<()> {
    let file_name = state.db.call(|c| repo::delete_document(c, id))?;
    let path = documents::stored_path(&documents_folder(state), &file_name)?;
    if let Err(error) = std::fs::remove_file(&path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("profile: could not remove a stored document: {error}");
        }
    }
    state.events.profile_changed();
    Ok(())
}

/// The stored file of a document (to open it or attach it to a form).
pub fn document_path(state: &AppState, id: i64) -> AppResult<(ProfileDocument, PathBuf)> {
    let (document, file_name) = state
        .db
        .call(|c| Ok((repo::get_document(c, id)?, repo::document_file_name(c, id)?)))?;
    let path = documents::stored_path(&documents_folder(state), &file_name)?;
    if !path.is_file() {
        return Err(AppError::not_found(
            "The document's file is missing from ReMa's data folder.",
        ));
    }
    Ok((document, path))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing};

    fn state() -> AppState {
        testing::state(Arc::new(FakeLanguageModel::replying(&[]))).0
    }

    #[test]
    fn normalizes_and_validates_on_save() {
        let state = state();
        let view = save(
            &state,
            Profile {
                first_name: "  Ana  ".into(),
                email: " ana@example.com ".into(),
                phone: "+43 660 123 4567".into(),
                github: "github.com/ana".into(),
                skills: vec!["Rust".into(), " rust ".into(), "".into(), "SQL".into()],
                experience: vec![
                    Experience::default(),
                    Experience {
                        title: "Engineer".into(),
                        end: "2020".into(),
                        current: true,
                        ..Experience::default()
                    },
                ],
                summary: "Line one\n\n\n\nLine two  ".into(),
                ..Profile::default()
            },
        )
        .unwrap();
        let p = view.profile;
        assert_eq!(p.first_name, "Ana");
        assert_eq!(p.email, "ana@example.com");
        assert_eq!(p.github, "https://github.com/ana");
        assert_eq!(p.skills, ["Rust", "SQL"]);
        assert_eq!(p.experience.len(), 1, "empty entries are dropped");
        assert_eq!(p.experience[0].end, "", "current jobs have no end");
        assert_eq!(p.summary, "Line one\n\nLine two");
        assert!(view.updated_at.is_some());

        for bad in [
            Profile {
                email: "ana@".into(),
                ..Profile::default()
            },
            Profile {
                phone: "call me".into(),
                ..Profile::default()
            },
            Profile {
                linkedin: "javascript:alert(1)".into(),
                ..Profile::default()
            },
            Profile {
                website: "ftp://ana.dev".into(),
                ..Profile::default()
            },
            Profile {
                custom_fields: vec![CustomField {
                    label: "Certificate".into(),
                    kind: CustomFieldKind::File,
                    value: String::new(),
                    document_id: Some(99),
                }],
                ..Profile::default()
            },
        ] {
            assert!(matches!(save(&state, bad), Err(AppError::Validation(_))));
        }
    }

    #[tokio::test]
    async fn stores_documents_in_the_data_folder_and_removes_them() {
        let state = state();
        let source = state.data_dir.join("upload-Ana CV.pdf");
        std::fs::write(&source, documents::samples::pdf(&["Ana Tester"])).unwrap();

        let doc = add_document(&state, &source, None).await.unwrap();
        assert_eq!(doc.kind, DocumentKind::Cv, "guessed from the name");
        assert_eq!(doc.original_name, "upload-Ana CV.pdf");
        assert!(doc.has_text);
        let (_, stored) = document_path(&state, doc.id).unwrap();
        assert!(stored.starts_with(documents_folder(&state)));
        assert_ne!(stored.file_name().unwrap(), "upload-Ana CV.pdf");
        assert_eq!(
            std::fs::read(&stored).unwrap(),
            std::fs::read(&source).unwrap()
        );

        let renamed = update_document(&state, doc.id, "Main CV", DocumentKind::Cv).unwrap();
        assert_eq!(renamed.name, "Main CV");
        assert_eq!(get(&state).unwrap().documents.len(), 1);

        delete_document(&state, doc.id).unwrap();
        assert!(!stored.exists());
        assert!(get(&state).unwrap().documents.is_empty());
    }

    #[tokio::test]
    async fn rejects_invalid_uploads() {
        let state = state();
        let fake = state.data_dir.join("cv.pdf");
        std::fs::write(&fake, b"this is not a pdf").unwrap();
        assert!(matches!(
            add_document(&state, &fake, None).await,
            Err(AppError::Validation(_))
        ));
        let empty = state.data_dir.join("empty.txt");
        std::fs::write(&empty, b"").unwrap();
        assert!(add_document(&state, &empty, None).await.is_err());
        assert!(add_document(&state, &state.data_dir, None).await.is_err());
        // Nothing was stored.
        assert!(get(&state).unwrap().documents.is_empty());
        let stored = std::fs::read_dir(documents_folder(&state))
            .map(|d| d.count())
            .unwrap_or(0);
        assert_eq!(stored, 0);
    }
}
