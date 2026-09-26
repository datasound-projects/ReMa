//! Portfolio Studio: CVs built in ReMa.
//!
//! Documents are separate from uploaded files, which are never changed.
//! The interface lays out and renders the PDF (the preview and the export
//! are the same file); ReMa validates the content, stores it and writes the
//! exported PDF where the user chooses.

use std::path::Path;

use base64::Engine;

use crate::{
    db::{portfolio as repo, profile as profile_repo},
    error::{AppError, AppResult},
    models::{
        portfolio::{
            PageSize, PortfolioContent, PortfolioDocument, PortfolioEntry, PortfolioHeader,
            PortfolioInput, PortfolioSection, PortfolioStart, SectionKind,
        },
        profile::{Profile, ProfileCredential},
    },
    services::profile::{line, normalize_email, normalize_phone, normalize_url, text},
    state::AppState,
    time::now_ms,
};

pub const DEFAULT_TEMPLATE: &str = "modern";
const MAX_SECTIONS: usize = 30;
const MAX_ENTRIES: usize = 60;
const MAX_TAGS: usize = 100;
const MAX_TITLE: usize = 200;
const MAX_DESCRIPTION: usize = 5_000;
const MAX_TEXT: usize = 8_000;
/// Largest exported PDF accepted from the interface.
const MAX_PDF_BYTES: usize = 25 * 1024 * 1024;

pub fn list(state: &AppState) -> AppResult<Vec<PortfolioDocument>> {
    state.db.call(|c| repo::list(c))
}

pub fn get(state: &AppState, id: i64) -> AppResult<PortfolioDocument> {
    state.db.call(|c| repo::get(c, id))
}

/// A short random id for sections and entries.
fn new_id() -> String {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes).unwrap_or_default();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn section(kind: SectionKind, entries: Vec<PortfolioEntry>, text: String) -> PortfolioSection {
    PortfolioSection {
        id: new_id(),
        kind,
        title: kind.default_title().to_string(),
        visible: true,
        text,
        entries,
    }
}

fn entry(title: &str, subtitle: &str) -> PortfolioEntry {
    PortfolioEntry {
        id: new_id(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        ..PortfolioEntry::default()
    }
}

/// Standard sections, empty.
fn blank_content() -> PortfolioContent {
    PortfolioContent {
        header: PortfolioHeader::default(),
        sections: [
            SectionKind::Summary,
            SectionKind::Experience,
            SectionKind::Education,
            SectionKind::Skills,
            SectionKind::Languages,
        ]
        .into_iter()
        .map(|kind| section(kind, Vec::new(), String::new()))
        .collect(),
    }
}

/// A one-time copy of the Custom Profile and credentials. Afterwards the
/// document and the Profile are edited independently.
fn content_from_profile(profile: &Profile, credentials: &[ProfileCredential]) -> PortfolioContent {
    let header = PortfolioHeader {
        full_name: profile.full_name(),
        headline: profile.title.clone(),
        email: profile.email.clone(),
        phone: profile.phone.clone(),
        location: profile.location.clone(),
        website: if profile.website.is_empty() {
            profile.resume_website.clone()
        } else {
            profile.website.clone()
        },
        linkedin: profile.linkedin.clone(),
        github: profile.github.clone(),
    };
    let experience = profile
        .experience
        .iter()
        .map(|e| PortfolioEntry {
            location: e.location.clone(),
            start: e.start.clone(),
            end: if e.current {
                "Present".into()
            } else {
                e.end.clone()
            },
            description: e.description.clone(),
            ..entry(&e.title, &e.company)
        })
        .collect();
    let education = profile
        .education
        .iter()
        .map(|e| {
            let degree = [e.degree.trim(), e.field.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            PortfolioEntry {
                start: e.start.clone(),
                end: e.end.clone(),
                description: e.description.clone(),
                ..entry(&degree, &e.school)
            }
        })
        .collect();
    let skills = if profile.skills.is_empty() {
        Vec::new()
    } else {
        vec![PortfolioEntry {
            tags: profile.skills.clone(),
            ..entry("", "")
        }]
    };
    let languages = profile
        .languages
        .iter()
        .map(|l| entry(&l.name, &l.level))
        .collect();
    let certifications: Vec<PortfolioEntry> = credentials
        .iter()
        .map(|c| PortfolioEntry {
            end: c.issue_date.clone(),
            url: c.credential_url.clone(),
            ..entry(&c.title, &c.issuer)
        })
        .collect();
    let links: Vec<PortfolioEntry> = profile
        .other_links
        .iter()
        .map(|l| PortfolioEntry {
            url: l.url.clone(),
            ..entry(&l.label, "")
        })
        .collect();

    let mut sections = vec![
        section(SectionKind::Summary, Vec::new(), profile.summary.clone()),
        section(SectionKind::Experience, experience, String::new()),
        section(SectionKind::Education, education, String::new()),
        section(SectionKind::Skills, skills, String::new()),
        section(SectionKind::Languages, languages, String::new()),
    ];
    if !certifications.is_empty() {
        sections.push(section(
            SectionKind::Certifications,
            certifications,
            String::new(),
        ));
    }
    if !links.is_empty() {
        sections.push(section(SectionKind::Links, links, String::new()));
    }
    PortfolioContent { header, sections }
}

pub fn create(
    state: &AppState,
    name: &str,
    template_id: &str,
    page_size: PageSize,
    start: PortfolioStart,
) -> AppResult<PortfolioDocument> {
    let content = match start {
        PortfolioStart::Blank => blank_content(),
        PortfolioStart::CustomProfile => state.db.call(|c| {
            let (profile, _) = profile_repo::get(c)?;
            Ok(content_from_profile(
                &profile,
                &profile_repo::list_credentials(c)?,
            ))
        })?,
    };
    let input = normalize(PortfolioInput {
        name: if name.trim().is_empty() {
            "Untitled CV".into()
        } else {
            name.to_string()
        },
        template_id: template_id.to_string(),
        page_size,
        accent: String::new(),
        content,
    })?;
    let document = state.db.call(|c| {
        let id = repo::insert(c, &input, now_ms())?;
        repo::get(c, id)
    })?;
    state.events.portfolio_changed();
    Ok(document)
}

/// Validates and stores the whole document (the editor saves as you type).
pub fn save(state: &AppState, id: i64, input: PortfolioInput) -> AppResult<PortfolioDocument> {
    let input = normalize(input)?;
    let document = state.db.call(|c| {
        repo::update(c, id, &input, now_ms())?;
        repo::get(c, id)
    })?;
    state.events.portfolio_changed();
    Ok(document)
}

pub fn duplicate(state: &AppState, id: i64) -> AppResult<PortfolioDocument> {
    let document = state.db.call(|c| {
        let original = repo::get(c, id)?;
        let name: String = format!("{} (copy)", original.name)
            .chars()
            .take(120)
            .collect();
        let copy = PortfolioInput {
            name,
            template_id: original.template_id,
            page_size: original.page_size,
            accent: original.accent,
            content: original.content,
        };
        let id = repo::insert(c, &copy, now_ms())?;
        repo::get(c, id)
    })?;
    state.events.portfolio_changed();
    Ok(document)
}

pub fn delete(state: &AppState, id: i64) -> AppResult<()> {
    state.db.call(|c| repo::delete(c, id))?;
    state.events.portfolio_changed();
    Ok(())
}

/// Decodes and checks a PDF produced by the interface.
pub fn decode_pdf(pdf_base64: &str) -> AppResult<Vec<u8>> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(pdf_base64.trim())
        .map_err(|_| AppError::validation("The exported PDF is damaged. Try again."))?;
    if bytes.len() > MAX_PDF_BYTES {
        return Err(AppError::validation("The exported PDF is too large."));
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err(AppError::validation("The export did not produce a PDF."));
    }
    Ok(bytes)
}

/// Writes the exported PDF to the file the user chose.
pub fn write_pdf(path: &Path, bytes: &[u8]) -> AppResult<()> {
    std::fs::write(path, bytes)
        .map_err(|e| AppError::validation(format!("The PDF could not be saved: {e}")))
}

/// A file name for the export, from the document name.
pub fn export_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.()".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned = cleaned.trim_matches('.').trim();
    format!("{}.pdf", if cleaned.is_empty() { "CV" } else { cleaned })
}

// ── Validation ──────────────────────────────────────────────────────

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn normalize_accent(value: &str) -> AppResult<String> {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return Ok(value);
    }
    let hex = value.strip_prefix('#').unwrap_or("");
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::validation(
            "Choose an accent color from the list.",
        ));
    }
    Ok(value)
}

fn normalize_entry(e: PortfolioEntry) -> AppResult<PortfolioEntry> {
    if e.tags.len() > MAX_TAGS {
        return Err(AppError::validation("Too many skills in one group."));
    }
    let mut tags: Vec<String> = Vec::new();
    for tag in &e.tags {
        let tag = line(tag, 80, "Skill")?;
        if !tag.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            tags.push(tag);
        }
    }
    Ok(PortfolioEntry {
        id: if valid_id(&e.id) { e.id } else { new_id() },
        title: line(&e.title, MAX_TITLE, "Title")?,
        subtitle: line(&e.subtitle, MAX_TITLE, "Subtitle")?,
        location: line(&e.location, MAX_TITLE, "Location")?,
        start: line(&e.start, 40, "Start")?,
        end: line(&e.end, 40, "End")?,
        url: normalize_url(&e.url, "Link")?,
        description: text(&e.description, MAX_DESCRIPTION, "Description")?,
        tags,
    })
}

fn normalize_content(content: PortfolioContent) -> AppResult<PortfolioContent> {
    if content.sections.len() > MAX_SECTIONS {
        return Err(AppError::validation("Too many sections."));
    }
    let h = content.header;
    let header = PortfolioHeader {
        full_name: line(&h.full_name, MAX_TITLE, "Name")?,
        headline: line(&h.headline, MAX_TITLE, "Headline")?,
        email: normalize_email(&h.email)?,
        phone: normalize_phone(&h.phone)?,
        location: line(&h.location, MAX_TITLE, "Location")?,
        website: normalize_url(&h.website, "Website")?,
        linkedin: normalize_url(&h.linkedin, "LinkedIn")?,
        github: normalize_url(&h.github, "GitHub")?,
    };
    let mut sections = Vec::with_capacity(content.sections.len());
    let mut seen = std::collections::HashSet::new();
    for s in content.sections {
        if s.entries.len() > MAX_ENTRIES {
            return Err(AppError::validation(format!(
                "“{}” has too many entries.",
                s.title
            )));
        }
        let mut id = if valid_id(&s.id) { s.id } else { new_id() };
        if !seen.insert(id.clone()) {
            id = new_id();
        }
        let title = line(&s.title, MAX_TITLE, "Section title")?;
        sections.push(PortfolioSection {
            id,
            kind: s.kind,
            title: if title.is_empty() {
                s.kind.default_title().to_string()
            } else {
                title
            },
            visible: s.visible,
            text: text(&s.text, MAX_TEXT, "Section text")?,
            entries: s
                .entries
                .into_iter()
                .map(normalize_entry)
                .collect::<AppResult<_>>()?,
        });
    }
    Ok(PortfolioContent { header, sections })
}

fn normalize(input: PortfolioInput) -> AppResult<PortfolioInput> {
    let name = line(&input.name, 120, "Name")?;
    if name.is_empty() {
        return Err(AppError::validation("Give the CV a name."));
    }
    if !valid_id(&input.template_id) {
        return Err(AppError::validation("Choose a template."));
    }
    Ok(PortfolioInput {
        name,
        template_id: input.template_id,
        page_size: input.page_size,
        accent: normalize_accent(&input.accent)?,
        content: normalize_content(input.content)?,
    })
}

// ── Plain text (Profile context) ────────────────────────────────────

/// The visible content as plain text, for Profile context.
pub fn plain_text(content: &PortfolioContent) -> String {
    let h = &content.header;
    let mut out: Vec<String> = Vec::new();
    let head = [
        h.full_name.as_str(),
        h.headline.as_str(),
        h.location.as_str(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    if !head.is_empty() {
        out.push(head);
    }
    let links = [h.website.as_str(), h.linkedin.as_str(), h.github.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    if !links.is_empty() {
        out.push(format!("Links: {links}"));
    }
    for section in content.sections.iter().filter(|s| s.visible) {
        let mut block: Vec<String> = Vec::new();
        if !section.text.is_empty() {
            block.push(section.text.clone());
        }
        for e in &section.entries {
            let what = [e.title.as_str(), e.subtitle.as_str()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" — ");
            let when = match (e.start.as_str(), e.end.as_str()) {
                ("", "") => String::new(),
                (s, "") => s.to_string(),
                ("", e) => e.to_string(),
                (s, e) => format!("{s} – {e}"),
            };
            let meta = [e.location.as_str(), when.as_str(), e.url.as_str()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            let mut item = format!("- {what}");
            if !meta.is_empty() {
                item.push_str(&format!(" ({meta})"));
            }
            if !e.tags.is_empty() {
                item.push_str(&format!(": {}", e.tags.join(", ")));
            }
            if !e.description.is_empty() {
                item.push_str(&format!("\n  {}", e.description.replace('\n', "\n  ")));
            }
            if item.trim() != "-" {
                block.push(item);
            }
        }
        if !block.is_empty() {
            out.push(format!("{}:\n{}", section.title, block.join("\n")));
        }
    }
    out.join("\n\n")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        db::profile as profile_db, llm::fake::FakeLanguageModel, models::profile::Experience,
        services::profile, state::testing,
    };

    fn state() -> AppState {
        testing::state(Arc::new(FakeLanguageModel::replying(&[]))).0
    }

    #[test]
    fn creates_blank_and_profile_based_documents() {
        let state = state();
        let blank = create(&state, "", "minimal", PageSize::A4, PortfolioStart::Blank).unwrap();
        assert_eq!(blank.name, "Untitled CV");
        assert_eq!(blank.content.sections.len(), 5);
        assert!(blank.content.header.full_name.is_empty());

        profile::save(
            &state,
            Profile {
                first_name: "Ana".into(),
                last_name: "Tester".into(),
                title: "Data Engineer".into(),
                skills: vec!["Rust".into(), "SQL".into()],
                experience: vec![Experience {
                    title: "Engineer".into(),
                    company: "Globex".into(),
                    current: true,
                    ..Experience::default()
                }],
                ..Profile::default()
            },
        )
        .unwrap();
        let copy = create(
            &state,
            "Data CV",
            "technical",
            PageSize::Letter,
            PortfolioStart::CustomProfile,
        )
        .unwrap();
        assert_eq!(copy.content.header.full_name, "Ana Tester");
        let experience = &copy.content.sections[1];
        assert_eq!(experience.entries[0].subtitle, "Globex");
        assert_eq!(experience.entries[0].end, "Present");
        assert_eq!(copy.content.sections[3].entries[0].tags, ["Rust", "SQL"]);

        // The copy is independent: editing it leaves the Custom Profile alone.
        let mut input = PortfolioInput {
            name: copy.name.clone(),
            template_id: copy.template_id.clone(),
            page_size: copy.page_size,
            accent: String::new(),
            content: copy.content.clone(),
        };
        input.content.header.full_name = "A. Tester".into();
        save(&state, copy.id, input).unwrap();
        let (saved_profile, _) = state.db.call(|c| profile_db::get(c)).unwrap();
        assert_eq!(saved_profile.full_name(), "Ana Tester");
    }

    #[test]
    fn switching_templates_keeps_content_and_duplicates_are_independent() {
        let state = state();
        let doc = create(&state, "CV", "minimal", PageSize::A4, PortfolioStart::Blank).unwrap();
        let mut content = doc.content.clone();
        content.header.full_name = "Ana Tester".into();
        content.sections[0].text = "Builds data platforms.".into();
        content.sections[2].visible = false;
        content.sections.swap(0, 1);
        let saved = save(
            &state,
            doc.id,
            PortfolioInput {
                name: "CV".into(),
                template_id: "minimal".into(),
                page_size: PageSize::A4,
                accent: "#0A66C2".into(),
                content: content.clone(),
            },
        )
        .unwrap();
        assert_eq!(saved.accent, "#0a66c2");

        let switched = save(
            &state,
            doc.id,
            PortfolioInput {
                name: "CV".into(),
                template_id: "executive".into(),
                page_size: PageSize::Letter,
                accent: String::new(),
                content: saved.content.clone(),
            },
        )
        .unwrap();
        assert_eq!(switched.template_id, "executive");
        assert_eq!(
            switched.content, saved.content,
            "content survives a template switch"
        );

        let copy = duplicate(&state, doc.id).unwrap();
        assert_eq!(copy.name, "CV (copy)");
        assert_eq!(copy.content, switched.content);
        delete(&state, doc.id).unwrap();
        assert_eq!(list(&state).unwrap().len(), 1);

        // Plain text follows the order and skips hidden sections.
        let text = plain_text(&copy.content);
        assert!(text.starts_with("Ana Tester"), "{text}");
        assert!(text.contains("Summary:\nBuilds data platforms."));
        assert!(!text.contains("Education:"));
    }

    #[test]
    fn rejects_invalid_content_and_pdfs() {
        let state = state();
        let doc = create(&state, "CV", "minimal", PageSize::A4, PortfolioStart::Blank).unwrap();
        let base = PortfolioInput {
            name: "CV".into(),
            template_id: "minimal".into(),
            page_size: PageSize::A4,
            accent: String::new(),
            content: doc.content.clone(),
        };
        let mut bad_link = base.clone();
        bad_link.content.header.website = "javascript:alert(1)".into();
        let mut bad_template = base.clone();
        bad_template.template_id = "../x".into();
        let mut bad_accent = base.clone();
        bad_accent.accent = "red; background:url(x)".into();
        for input in [bad_link, bad_template, bad_accent] {
            assert!(matches!(
                save(&state, doc.id, input),
                Err(AppError::Validation(_))
            ));
        }

        let pdf = base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.7\n...");
        assert!(decode_pdf(&pdf).is_ok());
        let html = base64::engine::general_purpose::STANDARD.encode(b"<html>");
        assert!(decode_pdf(&html).is_err());
        assert!(decode_pdf("not base64!").is_err());
        assert_eq!(
            export_file_name("Ana / Data CV: 2026"),
            "Ana Data CV 2026.pdf"
        );
        assert_eq!(export_file_name("../.."), "CV.pdf");
    }

    #[test]
    fn exported_pdfs_are_written_where_chosen() {
        let dir = testing::temp_dir();
        let path = dir.join("Ana.pdf");
        write_pdf(&path, b"%PDF-1.7").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"%PDF-1.7");
    }
}
