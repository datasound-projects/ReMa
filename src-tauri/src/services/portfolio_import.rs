//! Imports an existing CV or cover letter (PDF, DOCX, text, Markdown) into
//! Portfolio Studio.
//!
//! The file is stored as a Profile document and never changed; the text
//! ReMa extracted from it is read into editable sections. Contact details
//! and links are found deterministically; the model chosen in Settings
//! reads the rest, marking entries it is unsure about. Entries whose
//! employer or title is not in the text are dropped, and the whole result
//! is a proposal the user reviews before a document is made.
//!
//! Scanned PDFs and images have no text layer. ReMa has no OCR engine, so
//! such files import as an empty document with a clear note.

use std::collections::HashSet;

use serde::Deserialize;

use crate::{
    db::profile as repo,
    error::AppResult,
    jobs::extract::json_object,
    models::{
        portfolio::{
            CoverLetter, PortfolioContent, PortfolioHeader, PortfolioImport, PortfolioKind,
            PortfolioSection, SectionKind,
        },
        profile::{DocumentKind, Profile},
    },
    services::{
        portfolio::new_id,
        portfolio_ai::{
            self, ask, figures, from_raw_content, from_raw_letter, RawContent, RawLetter,
        },
        profile, profile_import,
    },
    state::AppState,
};

const CV_RULES: &str = "You read a CV/resume for ReMa's Portfolio Studio and return it as JSON so the \
person can edit it in a template. Rules:\n\
- Copy facts exactly as written: names, employers, titles, dates, places, figures. Never \
invent, complete or guess anything that is not in the text.\n\
- Keep the person's wording for descriptions; turn lists of responsibilities into lines \
starting with \"- \".\n\
- Put each part where it belongs: summary, experience, projects, education, skills (tags \
grouped by heading), languages, certifications, publications, links; anything else goes \
into a custom section with the heading used in the CV.\n\
- Keep the CV's section order.\n\
- If a value is unreadable or ambiguous (garbled text, an unclear date), leave it empty \
and set \"uncertain\": true on that entry.\n\
- Answer with one JSON object only, in this shape: {\"header\": {\"fullName\", \"headline\", \
\"email\", \"phone\", \"location\", \"website\", \"linkedin\", \"github\"}, \"sections\": [{\"kind\", \
\"title\", \"text\", \"entries\": [{\"title\", \"subtitle\", \"location\", \"start\", \"end\", \"url\", \
\"description\", \"tags\", \"uncertain\"}]}], \"notes\": [\"…\"]}. For experience, \"title\" is \
the job title and \"subtitle\" the employer; for education, \"title\" is the degree and \
\"subtitle\" the school; for skills, one entry per group with the group name as \"title\" \
and the skills as \"tags\"; for languages, \"title\" is the language and \"subtitle\" the \
level.";

const LETTER_RULES: &str =
    "You read a cover letter for ReMa's Portfolio Studio and return its parts as JSON so \
the person can edit it in a template. Copy the text exactly; never invent or complete \
anything. Answer with one JSON object only: {\"header\": {\"fullName\", \"headline\", \"email\", \
\"phone\", \"location\", \"website\", \"linkedin\", \"github\"} (the sender), \"letter\": \
{\"recipientName\", \"recipientTitle\", \"company\", \"address\", \"position\", \"date\", \
\"subject\", \"greeting\", \"body\" (paragraphs separated by blank lines), \"closing\", \
\"signature\"}, \"notes\": [\"…\"]}. Leave what the letter does not contain empty.";

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CvAnswer {
    #[serde(flatten)]
    content: RawContent,
    notes: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LetterAnswer {
    header: portfolio_ai::RawHeader,
    letter: RawLetter,
    notes: Vec<String>,
}

/// Reads a stored Profile document into a proposal.
pub async fn import(
    state: &AppState,
    document_id: i64,
    kind: PortfolioKind,
) -> AppResult<PortfolioImport> {
    let (document, text) = state.db.call(|c| {
        Ok((
            repo::get_document(c, document_id)?,
            repo::document_text(c, document_id)?,
        ))
    })?;
    let mut result = PortfolioImport {
        document_id,
        document_name: document.name.clone(),
        kind,
        content: PortfolioContent::default(),
        letter: CoverLetter::default(),
        model: None,
        notes: Vec::new(),
        uncertain: Vec::new(),
        has_text: false,
    };
    let text = match text {
        Some(text) if !text.trim().is_empty() => text,
        _ => {
            result.notes.push(
                "This file has no readable text layer (a scanned PDF or an image). ReMa has \
                 no OCR engine, so nothing could be read from it. You can rebuild the document \
                 in a template and type the content, or attach the file as it is."
                    .into(),
            );
            return Ok(result);
        }
    };
    result.has_text = true;

    let found = profile_import::deterministic(&text);
    let header = header_from(&found);

    let model = portfolio_ai::default_model(state).ok();
    let Some(model) = model else {
        result.notes.push(
            "No model is set up, so only contact details were read. The text is kept in one \
             section for you to split up, or connect a model in Settings and import again."
                .into(),
        );
        result.content = fallback(header, &text);
        return Ok(result);
    };

    let rules = match kind {
        PortfolioKind::Cv => CV_RULES,
        PortfolioKind::CoverLetter => LETTER_RULES,
    };
    let prompt = format!(
        "{} text:\n\n{}",
        match kind {
            PortfolioKind::Cv => "CV",
            PortfolioKind::CoverLetter => "Cover letter",
        },
        text.chars().take(30_000).collect::<String>()
    );
    match ask(state, &model, rules, prompt, 8_000).await {
        Ok(answer) => match kind {
            PortfolioKind::Cv => match serde_json::from_str::<CvAnswer>(json_object(&answer)?) {
                Ok(raw) => {
                    result.model = Some(model.model_id.clone());
                    result.notes.extend(raw.notes.into_iter().take(20));
                    let uncertain_titles: HashSet<String> = raw
                        .content
                        .sections
                        .iter()
                        .flat_map(|s| s.entries.iter())
                        .filter(|e| e.uncertain)
                        .map(|e| format!("{}|{}", e.title, e.subtitle))
                        .collect();
                    let mut content = from_raw_content(raw.content, false);
                    merge_header(&mut content.header, &header);
                    let (verified, notes, uncertain) = verify(content, &text, &uncertain_titles);
                    result.content = verified;
                    result.notes.extend(notes);
                    result.uncertain = uncertain;
                }
                Err(_) => {
                    result.notes.push(
                        "The model's answer could not be read, so only contact details were \
                         imported. The text is kept in one section for you to split up."
                            .into(),
                    );
                    result.content = fallback(header, &text);
                }
            },
            PortfolioKind::CoverLetter => {
                match serde_json::from_str::<LetterAnswer>(json_object(&answer)?) {
                    Ok(raw) => {
                        result.model = Some(model.model_id.clone());
                        result.notes.extend(raw.notes.into_iter().take(20));
                        let mut content = from_raw_content(
                            RawContent {
                                header: raw.header,
                                sections: Vec::new(),
                            },
                            false,
                        );
                        merge_header(&mut content.header, &header);
                        let letter = from_raw_letter(raw.letter);
                        let plain = simplify(&text);
                        for (field, value) in [
                            ("recipient", &letter.recipient_name),
                            ("company", &letter.company),
                            ("position", &letter.position),
                        ] {
                            if !value.trim().is_empty() && !plain.contains(&simplify(value)) {
                                result.notes.push(format!(
                                    "The {field} the model read (“{value}”) is not in the letter \
                                     as written. Check it."
                                ));
                                result.uncertain.push(field.into());
                            }
                        }
                        result.content = content;
                        result.letter = letter;
                    }
                    Err(_) => {
                        result.notes.push(
                            "The model's answer could not be read, so the letter text is kept \
                             as one body for you to edit."
                                .into(),
                        );
                        result.content = PortfolioContent {
                            header,
                            sections: Vec::new(),
                        };
                        result.letter = CoverLetter {
                            body: text.trim().to_string(),
                            ..CoverLetter::default()
                        };
                    }
                }
            }
        },
        Err(error) => {
            result.notes.push(format!(
                "The model could not read the document ({error}). Only contact details were \
                 read; the text is kept in one section for you to split up."
            ));
            result.content = fallback(header, &text);
        }
    }
    Ok(result)
}

fn header_from(profile: &Profile) -> PortfolioHeader {
    PortfolioHeader {
        email: profile.email.clone(),
        phone: profile.phone.clone(),
        website: profile.website.clone(),
        linkedin: profile.linkedin.clone(),
        github: profile.github.clone(),
        ..PortfolioHeader::default()
    }
}

/// Deterministic contact details win where they were found in the text.
fn merge_header(header: &mut PortfolioHeader, found: &PortfolioHeader) {
    for (target, value) in [
        (&mut header.email, &found.email),
        (&mut header.phone, &found.phone),
        (&mut header.linkedin, &found.linkedin),
        (&mut header.github, &found.github),
    ] {
        if !value.is_empty() {
            *target = value.clone();
        }
    }
    if header.website.is_empty() {
        header.website = found.website.clone();
    }
}

/// Without a model: the text as one editable section.
fn fallback(header: PortfolioHeader, text: &str) -> PortfolioContent {
    PortfolioContent {
        header,
        sections: vec![PortfolioSection {
            id: new_id(),
            kind: SectionKind::Custom,
            title: "Imported text".into(),
            visible: true,
            text: text.trim().chars().take(8_000).collect(),
            entries: Vec::new(),
        }],
    }
}

fn simplify(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_ascii_punctuation() || *c == '@' || *c == '.')
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drops what the text does not contain and lists what to check.
/// Returns the content, notes and the ids of uncertain entries.
fn verify(
    mut content: PortfolioContent,
    text: &str,
    uncertain_titles: &HashSet<String>,
) -> (PortfolioContent, Vec<String>, Vec<String>) {
    let plain = simplify(text);
    let known: HashSet<String> = figures(text).into_iter().collect();
    let mut notes = Vec::new();
    let mut uncertain = Vec::new();
    let in_text = |value: &str| {
        let v = simplify(value);
        v.is_empty() || plain.contains(&v)
    };

    if !in_text(&content.header.full_name) {
        notes.push("The name the model read is not in the document; left empty.".into());
        content.header.full_name.clear();
    }
    let mut dropped = 0;
    for section in &mut content.sections {
        section.entries.retain(|e| {
            let keep = in_text(&e.title) || in_text(&e.subtitle);
            if !keep {
                dropped += 1;
            }
            keep
        });
        for e in &mut section.entries {
            let mut unsure = uncertain_titles.contains(&format!("{}|{}", e.title, e.subtitle));
            for date in [&mut e.start, &mut e.end] {
                let d: String = date.chars().filter(char::is_ascii_digit).collect();
                if d.len() >= 2 && !known.contains(&d) && !text.contains(date.as_str()) {
                    date.clear();
                    unsure = true;
                }
            }
            if !in_text(&e.title) || !in_text(&e.subtitle) {
                unsure = true;
            }
            if unsure {
                uncertain.push(e.id.clone());
            }
        }
    }
    if dropped > 0 {
        notes.push(format!(
            "{dropped} entr{} the model read {} not in the document and {} left out.",
            if dropped == 1 { "y" } else { "ies" },
            if dropped == 1 { "is" } else { "are" },
            if dropped == 1 { "was" } else { "were" }
        ));
    }
    if !uncertain.is_empty() {
        notes.push(format!(
            "{} entr{} highlighted for you to check (an unclear date, employer or title).",
            uncertain.len(),
            if uncertain.len() == 1 {
                "y is"
            } else {
                "ies are"
            }
        ));
    }
    content
        .sections
        .retain(|s| !s.entries.is_empty() || !s.text.trim().is_empty());
    (content, notes, uncertain)
}

/// Stores a picked file as a Profile document and reads it.
pub async fn import_file(
    state: &AppState,
    path: &std::path::Path,
    kind: PortfolioKind,
) -> AppResult<PortfolioImport> {
    let document_kind = match kind {
        PortfolioKind::Cv => DocumentKind::Cv,
        PortfolioKind::CoverLetter => DocumentKind::Other,
    };
    let document = profile::add_document(state, path, Some(document_kind)).await?;
    import(state, document.id, kind).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        llm::fake::FakeLanguageModel,
        models::provider::{ModelRef, ProviderKind},
        services::{documents::samples, portfolio, providers},
        state::testing,
    };

    const CV: &str =
        "Ana Tester\nData Engineer\nana.tester@example.com · +43 660 1234567 · Vienna\n\
        github.com/anatester\n\nExperience\nSenior Data Engineer, Globex, 2021 – present\n\
        - Built the lakehouse\nData Engineer, Initech, 2018 – 2021\n\nSkills\nPython, SQL";

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
        profile::add_document(state, &path, Some(DocumentKind::Cv))
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn imports_a_cv_and_drops_entries_that_are_not_in_the_text() {
        let state = state_with(
            r#"{"header":{"fullName":"Ana Tester","headline":"Data Engineer","email":"","phone":"","location":"Vienna"},
            "sections":[
              {"kind":"experience","title":"Experience","entries":[
                {"title":"Senior Data Engineer","subtitle":"Globex","start":"2021","end":"Present","description":"- Built the lakehouse"},
                {"title":"Data Engineer","subtitle":"Initech","start":"2018","end":"2021"},
                {"title":"CTO","subtitle":"Acme","start":"2015","end":"2018"}]},
              {"kind":"skills","title":"Skills","entries":[{"title":"","tags":["Python","SQL"]}]}],
            "notes":["No education section found"]}"#,
        )
        .await;
        let id = add_cv(&state).await;
        let import = import(&state, id, PortfolioKind::Cv).await.unwrap();
        assert!(import.has_text);
        assert_eq!(import.model.as_deref(), Some("model-a"));
        assert_eq!(import.content.header.full_name, "Ana Tester");
        assert_eq!(
            import.content.header.email, "ana.tester@example.com",
            "found in the text"
        );
        assert_eq!(import.content.header.github, "https://github.com/anatester");
        let experience = &import.content.sections[0];
        assert_eq!(experience.kind, SectionKind::Experience);
        assert_eq!(experience.entries.len(), 2, "Acme is not in the text");
        assert_eq!(experience.entries[0].description, "- Built the lakehouse");
        assert_eq!(
            import.content.sections[1].entries[0].tags,
            ["Python", "SQL"]
        );
        assert!(
            import.notes.iter().any(|n| n.contains("1 entry")),
            "{:?}",
            import.notes
        );
        assert!(import
            .notes
            .contains(&"No education section found".to_string()));

        // The proposal is valid content for a new document.
        let doc = portfolio::create(
            &state,
            portfolio::PortfolioCreate {
                name: "Imported".into(),
                template_id: "modern".into(),
                content: Some(import.content),
                source_document_id: Some(id),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(doc.source_document_id, Some(id));
    }

    #[tokio::test]
    async fn without_a_model_the_text_is_kept_in_one_section() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let id = add_cv(&state).await;
        let import = import(&state, id, PortfolioKind::Cv).await.unwrap();
        assert_eq!(import.model, None);
        assert_eq!(import.content.header.phone, "+43 660 1234567");
        assert_eq!(import.content.sections.len(), 1);
        assert!(import.content.sections[0].text.contains("Globex"));
        assert!(import.notes[0].contains("No model"));
    }

    #[tokio::test]
    async fn scanned_files_import_empty_with_a_note() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let path = state.data_dir.join("scan.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\nrest").unwrap();
        let doc = profile::add_document(&state, &path, None).await.unwrap();
        let import = import(&state, doc.id, PortfolioKind::Cv).await.unwrap();
        assert!(!import.has_text);
        assert!(import.notes[0].contains("no OCR"));
    }

    #[tokio::test]
    async fn imports_a_cover_letter() {
        let state = state_with(
            r#"{"header":{"fullName":"Ana Tester"},"letter":{"recipientName":"Jordan Lee","company":"Fabrikam",
            "position":"Data Lead","greeting":"Dear Jordan,","body":"I am writing to apply.","closing":"Kind regards,","signature":"Ana Tester"}}"#,
        )
        .await;
        let path = state.data_dir.join("letter.txt");
        std::fs::write(&path, "Ana Tester\nDear Jordan,\nI am writing to apply for Data Lead at Fabrikam.\nKind regards,\nAna Tester").unwrap();
        let doc = profile::add_document(&state, &path, Some(DocumentKind::Other))
            .await
            .unwrap();
        let import = import(&state, doc.id, PortfolioKind::CoverLetter)
            .await
            .unwrap();
        assert_eq!(import.letter.company, "Fabrikam");
        assert_eq!(import.letter.body, "I am writing to apply.");
        assert!(
            import.uncertain.contains(&"recipient".to_string()),
            "Jordan Lee is not written in full"
        );
    }
}
