//! The Profile: documents, credentials and the optional Custom Profile.
//!
//! These are separate sources. Adding a document only stores it (and its
//! text for Profile context); it never fills in Custom Profile fields. An
//! import proposes values that the user reviews; everything in the Custom
//! Profile is saved through `save`, which validates every field.

use std::path::{Path, PathBuf};

use crate::{
    db::profile::{self as repo, NewDocument},
    error::{AppError, AppResult},
    models::profile::{
        AddDocumentsResult, CredentialInput, CustomField, CustomFieldKind, DocumentBlock,
        DocumentFormat, DocumentKind, Education, Experience, Language, Profile, ProfileCredential,
        ProfileDocument, ProfileLink, ProfileView, RejectedFile,
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
            credentials: repo::list_credentials(c)?,
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
pub(crate) fn line(value: &str, max: usize, field: &str) -> AppResult<String> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() > max {
        return Err(AppError::validation(format!("{field} is too long.")));
    }
    Ok(value)
}

/// Multi-line text: lines trimmed, at most one blank line in a row.
pub(crate) fn text(value: &str, max: usize, field: &str) -> AppResult<String> {
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

pub(crate) fn normalize_email(value: &str) -> AppResult<String> {
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

pub(crate) fn normalize_phone(value: &str) -> AppResult<String> {
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

/// A validated file, copied into the documents folder.
struct StoredFile {
    original_name: String,
    format: DocumentFormat,
    file_name: String,
    size: i64,
    sha256: String,
    text: Option<String>,
}

impl StoredFile {
    fn record<'a>(&'a self, name: &'a str, kind: DocumentKind) -> NewDocument<'a> {
        NewDocument {
            name,
            kind,
            format: self.format,
            original_name: &self.original_name,
            file_name: &self.file_name,
            size: self.size,
            sha256: &self.sha256,
            text: self.text.as_deref(),
            now: now_ms(),
        }
    }
}

/// Validates a user-chosen file (type by content, size), copies it under a
/// generated name and extracts its text. Only `allowed` formats pass.
async fn store_file(
    state: &AppState,
    source: &Path,
    allowed: &'static [DocumentFormat],
) -> AppResult<StoredFile> {
    let source = source.to_path_buf();
    let folder = documents_folder(state);
    // Parsing is CPU-bound; keep it off the async runtime threads.
    tokio::task::spawn_blocking(move || -> AppResult<StoredFile> {
        let (original_name, bytes) = documents::read_source(&source)?;
        let format = documents::detect_format(&original_name, &bytes)?;
        if !allowed.contains(&format) {
            return Err(AppError::validation(format!(
                "{} files cannot be used here.",
                format.extension().to_uppercase()
            )));
        }
        let text = documents::extract_text(format, &bytes)?;
        let file_name = documents::generated_file_name(format)?;
        documents::store(&folder, &file_name, &bytes)?;
        Ok(StoredFile {
            original_name,
            format,
            file_name,
            size: bytes.len() as i64,
            sha256: documents::sha256_hex(&bytes),
            text,
        })
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

/// Removes a stored copy that no row refers to (after a failed insert).
fn discard_file(state: &AppState, file_name: &str) {
    if let Ok(path) = documents::stored_path(&documents_folder(state), file_name) {
        if let Err(error) = std::fs::remove_file(path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("profile: could not remove a stored document: {error}");
            }
        }
    }
}

/// Formats accepted for each kind of document.
pub fn allowed_formats(kind: Option<DocumentKind>) -> &'static [DocumentFormat] {
    use DocumentFormat::*;
    match kind {
        // CVs: documents with text.
        Some(DocumentKind::Cv) => &[Pdf, Docx, Text, Markdown],
        // Credentials and portfolios: documents and images.
        Some(DocumentKind::Certificate | DocumentKind::Portfolio) => &[Pdf, Docx, Png, Jpeg, Webp],
        _ => &[Pdf, Docx, Text, Markdown, Png, Jpeg, Webp],
    }
}

/// File name extensions for the file picker, matching `allowed_formats`.
pub fn picker_extensions(kind: Option<DocumentKind>) -> Vec<&'static str> {
    let mut extensions = Vec::new();
    for format in allowed_formats(kind) {
        extensions.extend_from_slice(match format {
            DocumentFormat::Pdf => &["pdf"][..],
            DocumentFormat::Docx => &["docx"],
            DocumentFormat::Text => &["txt"],
            DocumentFormat::Markdown => &["md", "markdown"],
            DocumentFormat::Png => &["png"],
            DocumentFormat::Jpeg => &["jpg", "jpeg"],
            DocumentFormat::Webp => &["webp"],
        });
    }
    extensions
}

/// Validates a user-chosen file, stores a copy and extracts its text. The
/// document is only added to the library: nothing else changes.
pub async fn add_document(
    state: &AppState,
    source: &Path,
    kind: Option<DocumentKind>,
) -> AppResult<ProfileDocument> {
    let stored = store_file(state, source, allowed_formats(kind)).await?;
    let name = Path::new(&stored.original_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Document")
        .chars()
        .take(120)
        .collect::<String>();
    let kind = kind.unwrap_or_else(|| guess_kind(&stored.original_name));
    let inserted = state
        .db
        .call(|c| repo::insert_document(c, stored.record(&name, kind)));
    if inserted.is_err() {
        // Do not leave an orphaned copy behind.
        discard_file(state, &stored.file_name);
    }
    let document = inserted?;
    state.events.profile_changed();
    Ok(document)
}

/// Adds several files; each is validated on its own, so one bad file does
/// not stop the others.
pub async fn add_documents(
    state: &AppState,
    sources: &[PathBuf],
    kind: Option<DocumentKind>,
) -> AddDocumentsResult {
    let mut result = AddDocumentsResult {
        added: Vec::new(),
        failed: Vec::new(),
    };
    for source in sources {
        match add_document(state, source, kind).await {
            Ok(document) => result.added.push(document),
            Err(error) => result.failed.push(RejectedFile {
                name: source
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                reason: error.to_string(),
            }),
        }
    }
    result
}

/// Replaces a document's file with a new version. Its name, kind and
/// primary mark stay; the old copy is deleted.
pub async fn replace_document(
    state: &AppState,
    id: i64,
    source: &Path,
) -> AppResult<ProfileDocument> {
    let current = state.db.call(|c| repo::get_document(c, id))?;
    let stored = store_file(state, source, allowed_formats(Some(current.kind))).await?;
    let replaced = state.db.call(|c| {
        let old = repo::replace_document_file(c, id, stored.record(&current.name, current.kind))?;
        Ok((old, repo::get_document(c, id)?))
    });
    match replaced {
        Ok((old, document)) => {
            discard_file(state, &old);
            state.events.profile_changed();
            Ok(document)
        }
        Err(error) => {
            discard_file(state, &stored.file_name);
            Err(error)
        }
    }
}

pub fn set_primary_document(state: &AppState, id: i64) -> AppResult<ProfileDocument> {
    let document = state.db.call(|c| {
        let tx = c.transaction()?;
        repo::set_primary_document(&tx, id)?;
        let document = repo::get_document(&tx, id)?;
        tx.commit()?;
        Ok(document)
    })?;
    state.events.profile_changed();
    Ok(document)
}

/// A document's bytes for the in-app viewer, read from ReMa's own folder.
pub fn document_content(state: &AppState, id: i64) -> AppResult<(ProfileDocument, Vec<u8>)> {
    let (document, path) = document_path(state, id)?;
    let metadata = std::fs::symlink_metadata(&path)?;
    // Only plain files ReMa wrote; never follow a link out of the folder.
    if !metadata.is_file() || metadata.len() > documents::MAX_FILE_BYTES {
        return Err(AppError::validation("This document cannot be shown."));
    }
    Ok((document, std::fs::read(&path)?))
}

/// A Word or text document as plain blocks for the viewer.
pub async fn document_blocks(state: &AppState, id: i64) -> AppResult<Vec<DocumentBlock>> {
    let (document, bytes) = document_content(state, id)?;
    tokio::task::spawn_blocking(move || match document.format {
        DocumentFormat::Docx => documents::docx_blocks(&bytes),
        DocumentFormat::Text | DocumentFormat::Markdown => {
            Ok(documents::text_blocks(&String::from_utf8_lossy(&bytes)))
        }
        _ => Err(AppError::validation(
            "This document is shown as a page or an image.",
        )),
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

// ── Credentials ─────────────────────────────────────────────────────

/// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, or empty.
fn normalize_date(value: &str, field: &str) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let parts: Vec<&str> = value.split('-').collect();
    let number = |p: &str, len: usize| p.len() == len && p.chars().all(|c| c.is_ascii_digit());
    let valid = match parts.as_slice() {
        [y] => number(y, 4),
        [y, m] => number(y, 4) && number(m, 2) && (1..=12).contains(&m.parse::<u8>().unwrap_or(0)),
        [y, m, d] => {
            number(y, 4)
                && number(m, 2)
                && number(d, 2)
                && (1..=12).contains(&m.parse::<u8>().unwrap_or(0))
                && (1..=31).contains(&d.parse::<u8>().unwrap_or(0))
        }
        _ => false,
    };
    if !valid {
        return Err(AppError::validation(format!(
            "{field}: use a year, year-month or full date, e.g. 2024-06."
        )));
    }
    Ok(value.to_string())
}

fn normalize_credential(
    input: CredentialInput,
    fallback_title: Option<&str>,
) -> AppResult<CredentialInput> {
    let mut title = line(&input.title, MAX_SHORT, "Title")?;
    if title.is_empty() {
        title = fallback_title.map(str::to_string).unwrap_or_default();
    }
    if title.is_empty() {
        return Err(AppError::validation("Give the credential a title."));
    }
    let issue_date = normalize_date(&input.issue_date, "Issue date")?;
    let expiration_date = normalize_date(&input.expiration_date, "Expiration date")?;
    if !issue_date.is_empty() && !expiration_date.is_empty() && expiration_date < issue_date {
        return Err(AppError::validation(
            "The expiration date is before the issue date.",
        ));
    }
    Ok(CredentialInput {
        kind: input.kind,
        title,
        issuer: line(&input.issuer, MAX_SHORT, "Issuer")?,
        issue_date,
        expiration_date,
        credential_id: line(&input.credential_id, MAX_SHORT, "Credential ID")?,
        credential_url: normalize_url(&input.credential_url, "Credential URL")?,
        note: text(&input.note, 2_000, "Note")?,
    })
}

/// Creates (`id` = `None`) or updates a credential. `document_id` is its
/// file, stored before with `add_document`; a replaced file is deleted.
pub fn save_credential(
    state: &AppState,
    id: Option<i64>,
    input: CredentialInput,
    document_id: Option<i64>,
) -> AppResult<ProfileCredential> {
    let (credential, orphan) = state.db.call(|c| {
        let tx = c.transaction()?;
        let file = document_id
            .map(|doc| repo::get_document(&tx, doc))
            .transpose()?;
        let input = normalize_credential(input, file.as_ref().map(|d| d.name.as_str()))?;
        let previous = id
            .map(|id| repo::get_credential(&tx, id))
            .transpose()?
            .and_then(|c| c.document)
            .map(|d| d.id)
            .filter(|old| Some(*old) != document_id);
        let id = repo::save_credential(&tx, id, &input, document_id, now_ms())?;
        let orphan = match previous {
            Some(old) if !repo::document_used_by_credential(&tx, old)? => {
                Some(repo::delete_document(&tx, old)?)
            }
            _ => None,
        };
        let credential = repo::get_credential(&tx, id)?;
        tx.commit()?;
        Ok((credential, orphan))
    })?;
    if let Some(file_name) = orphan {
        discard_file(state, &file_name);
    }
    state.events.profile_changed();
    Ok(credential)
}

/// Deletes a credential and its file.
pub fn delete_credential(state: &AppState, id: i64) -> AppResult<()> {
    let file_name = state.db.call(|c| {
        let tx = c.transaction()?;
        let file = match repo::delete_credential(&tx, id)? {
            Some(doc) if !repo::document_used_by_credential(&tx, doc)? => {
                Some(repo::delete_document(&tx, doc)?)
            }
            _ => None,
        };
        tx.commit()?;
        Ok(file)
    })?;
    if let Some(file_name) = file_name {
        discard_file(state, &file_name);
    }
    state.events.profile_changed();
    Ok(())
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
    use crate::{llm::fake::FakeLanguageModel, models::profile::CredentialKind, state::testing};

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

    fn write(state: &AppState, name: &str, bytes: &[u8]) -> PathBuf {
        let path = state.data_dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[tokio::test]
    async fn uploading_cvs_only_adds_documents_and_marks_one_primary() {
        let state = state();
        let first = write(
            &state,
            "Ana CV.pdf",
            &documents::samples::pdf(&["Ana Tester", "Rust"]),
        );
        let second = write(
            &state,
            "cv-2024.docx",
            &documents::samples::docx(&["Ana", "SQL"]),
        );
        let bad = write(&state, "virus.pdf", b"MZ not a pdf");

        let result = add_documents(
            &state,
            &[first.clone(), second, bad],
            Some(DocumentKind::Cv),
        )
        .await;
        assert_eq!(result.added.len(), 2);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].name, "virus.pdf");

        let view = get(&state).unwrap();
        // The Custom Profile stays empty: nothing is extracted from a CV.
        assert!(view.profile.is_empty());
        assert!(view.updated_at.is_none());
        assert_eq!(view.documents.len(), 2);
        let primary: Vec<_> = view.documents.iter().filter(|d| d.is_primary).collect();
        assert_eq!(primary.len(), 1, "the first CV becomes primary");
        assert_eq!(primary[0].name, "Ana CV");

        // The stored file can be shown in the viewer.
        let (doc, bytes) = document_content(&state, primary[0].id).unwrap();
        assert_eq!(bytes, std::fs::read(&first).unwrap());
        assert_eq!(doc.format, DocumentFormat::Pdf);

        // Another CV can become primary; only one is.
        let other = view.documents.iter().find(|d| !d.is_primary).unwrap();
        set_primary_document(&state, other.id).unwrap();
        let view = get(&state).unwrap();
        assert!(
            view.documents
                .iter()
                .find(|d| d.id == other.id)
                .unwrap()
                .is_primary
        );
        assert_eq!(view.documents.iter().filter(|d| d.is_primary).count(), 1);

        // Removing the primary CV promotes the remaining one.
        delete_document(&state, other.id).unwrap();
        let view = get(&state).unwrap();
        assert_eq!(view.documents.len(), 1);
        assert!(view.documents[0].is_primary);
    }

    #[tokio::test]
    async fn replacing_a_cv_keeps_its_place_and_removes_the_old_file() {
        let state = state();
        let v1 = write(&state, "cv.pdf", &documents::samples::pdf(&["Version one"]));
        let doc = add_document(&state, &v1, Some(DocumentKind::Cv))
            .await
            .unwrap();
        update_document(&state, doc.id, "Main CV", DocumentKind::Cv).unwrap();
        let (_, old_path) = document_path(&state, doc.id).unwrap();

        let v2 = write(
            &state,
            "cv-new.docx",
            &documents::samples::docx(&["Version two"]),
        );
        let replaced = replace_document(&state, doc.id, &v2).await.unwrap();
        assert_eq!(replaced.id, doc.id);
        assert_eq!(replaced.name, "Main CV");
        assert!(replaced.is_primary);
        assert_eq!(replaced.format, DocumentFormat::Docx);
        assert_eq!(replaced.original_name, "cv-new.docx");
        assert!(!old_path.exists(), "the old copy is gone");
        let blocks = document_blocks(&state, doc.id).await.unwrap();
        assert_eq!(
            blocks,
            vec![DocumentBlock::Paragraph {
                text: "Version two".into()
            }]
        );

        // CVs must be documents with text.
        let image = write(&state, "photo.png", b"\x89PNG\r\n\x1a\nrest");
        assert!(replace_document(&state, doc.id, &image).await.is_err());
        assert!(add_document(&state, &image, Some(DocumentKind::Cv))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn credentials_keep_metadata_and_own_their_file() {
        let state = state();
        let file = write(&state, "aws.webp", &documents::samples::webp());
        let doc = add_document(&state, &file, Some(DocumentKind::Certificate))
            .await
            .unwrap();
        assert_eq!(doc.format, DocumentFormat::Webp);

        let credential = save_credential(
            &state,
            None,
            CredentialInput {
                kind: Some(CredentialKind::ProfessionalCertificate),
                issuer: " Amazon Web Services ".into(),
                issue_date: "2024-06".into(),
                expiration_date: "2027-06".into(),
                credential_url: "aws.amazon.com/verify/123".into(),
                ..CredentialInput::default()
            },
            Some(doc.id),
        )
        .unwrap();
        assert_eq!(
            credential.title, "aws",
            "the file name is the default title"
        );
        assert_eq!(credential.issuer, "Amazon Web Services");
        assert_eq!(
            credential.credential_url,
            "https://aws.amazon.com/verify/123"
        );
        assert_eq!(credential.document.as_ref().unwrap().id, doc.id);

        // Credentials without a file are fine, bad dates are not.
        let degree = save_credential(
            &state,
            None,
            CredentialInput {
                kind: Some(CredentialKind::Degree),
                title: "MSc Computer Science".into(),
                issue_date: "2019".into(),
                ..CredentialInput::default()
            },
            None,
        )
        .unwrap();
        assert!(degree.document.is_none());
        for (issue, expiry) in [("2024-13", ""), ("June 2024", ""), ("2024-06", "2023-01")] {
            let input = CredentialInput {
                title: "X".into(),
                issue_date: issue.into(),
                expiration_date: expiry.into(),
                ..CredentialInput::default()
            };
            assert!(
                save_credential(&state, None, input, None).is_err(),
                "{issue} {expiry}"
            );
        }
        assert!(save_credential(&state, None, CredentialInput::default(), None).is_err());

        // Replacing the file removes the old one.
        let (_, old_path) = document_path(&state, doc.id).unwrap();
        let pdf = write(&state, "aws.pdf", &documents::samples::pdf(&["AWS"]));
        let new_doc = add_document(&state, &pdf, Some(DocumentKind::Certificate))
            .await
            .unwrap();
        let input = CredentialInput {
            kind: Some(credential.kind),
            title: credential.title.clone(),
            ..CredentialInput::default()
        };
        save_credential(&state, Some(credential.id), input, Some(new_doc.id)).unwrap();
        assert!(!old_path.exists());

        // Deleting a credential deletes its file.
        let (_, new_path) = document_path(&state, new_doc.id).unwrap();
        delete_credential(&state, credential.id).unwrap();
        assert!(!new_path.exists());
        let view = get(&state).unwrap();
        assert_eq!(view.credentials.len(), 1);
        assert!(view.documents.is_empty());
        assert!(
            view.profile.is_empty(),
            "credentials never touch the Custom Profile"
        );
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
