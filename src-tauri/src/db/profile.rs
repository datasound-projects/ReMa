//! The profile (one row plus ordered lists) and profile document metadata.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::profile::{
        CustomField, CustomFieldKind, DocumentFormat, DocumentKind, Education, Experience,
        Language, Profile, ProfileDocument, ProfileLink,
    },
};

// ── Profile ─────────────────────────────────────────────────────────

/// The stored profile (empty if never saved) and when it was saved.
pub fn get(conn: &Connection) -> AppResult<(Profile, Option<i64>)> {
    let row = conn
        .query_row(
            "SELECT first_name, last_name, email, phone, location, title, summary, skills,
                    website, resume_website, github, linkedin, updated_at
             FROM profile WHERE id = 1",
            [],
            |r| {
                let skills: String = r.get(7)?;
                Ok((
                    Profile {
                        first_name: r.get(0)?,
                        last_name: r.get(1)?,
                        email: r.get(2)?,
                        phone: r.get(3)?,
                        location: r.get(4)?,
                        title: r.get(5)?,
                        summary: r.get(6)?,
                        skills: serde_json::from_str(&skills).unwrap_or_default(),
                        website: r.get(8)?,
                        resume_website: r.get(9)?,
                        github: r.get(10)?,
                        linkedin: r.get(11)?,
                        ..Profile::default()
                    },
                    r.get::<_, i64>(12)?,
                ))
            },
        )
        .optional()?;
    let Some((mut profile, updated_at)) = row else {
        return Ok((Profile::default(), None));
    };

    profile.experience = collect(
        conn,
        "SELECT title, company, location, start_date, end_date, current, description
         FROM profile_experience ORDER BY position",
        |r| {
            Ok(Experience {
                title: r.get(0)?,
                company: r.get(1)?,
                location: r.get(2)?,
                start: r.get(3)?,
                end: r.get(4)?,
                current: r.get(5)?,
                description: r.get(6)?,
            })
        },
    )?;
    profile.education = collect(
        conn,
        "SELECT school, degree, field, start_date, end_date, description
         FROM profile_education ORDER BY position",
        |r| {
            Ok(Education {
                school: r.get(0)?,
                degree: r.get(1)?,
                field: r.get(2)?,
                start: r.get(3)?,
                end: r.get(4)?,
                description: r.get(5)?,
            })
        },
    )?;
    profile.languages = collect(
        conn,
        "SELECT name, level FROM profile_languages ORDER BY position",
        |r| {
            Ok(Language {
                name: r.get(0)?,
                level: r.get(1)?,
            })
        },
    )?;
    profile.other_links = collect(
        conn,
        "SELECT label, url FROM profile_links ORDER BY position",
        |r| {
            Ok(ProfileLink {
                label: r.get(0)?,
                url: r.get(1)?,
            })
        },
    )?;
    profile.custom_fields = collect(
        conn,
        "SELECT label, kind, value, document_id FROM profile_fields ORDER BY position",
        |r| {
            let kind: String = r.get(1)?;
            Ok(CustomField {
                label: r.get(0)?,
                kind: CustomFieldKind::parse(&kind).unwrap_or(CustomFieldKind::Text),
                value: r.get(2)?,
                document_id: r.get(3)?,
            })
        },
    )?;
    Ok((profile, Some(updated_at)))
}

fn collect<T>(
    conn: &Connection,
    sql: &str,
    map: impl FnMut(&Row) -> rusqlite::Result<T>,
) -> AppResult<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], map)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Replaces the whole profile. Call inside a transaction.
pub fn save(conn: &Connection, profile: &Profile, now: i64) -> AppResult<()> {
    let skills =
        serde_json::to_string(&profile.skills).map_err(|e| AppError::internal(e.to_string()))?;
    conn.execute(
        "INSERT INTO profile (id, first_name, last_name, email, phone, location, title, summary,
             skills, website, resume_website, github, linkedin, updated_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT (id) DO UPDATE SET
             first_name = excluded.first_name, last_name = excluded.last_name,
             email = excluded.email, phone = excluded.phone, location = excluded.location,
             title = excluded.title, summary = excluded.summary, skills = excluded.skills,
             website = excluded.website, resume_website = excluded.resume_website,
             github = excluded.github, linkedin = excluded.linkedin,
             updated_at = excluded.updated_at",
        params![
            profile.first_name,
            profile.last_name,
            profile.email,
            profile.phone,
            profile.location,
            profile.title,
            profile.summary,
            skills,
            profile.website,
            profile.resume_website,
            profile.github,
            profile.linkedin,
            now,
        ],
    )?;

    conn.execute_batch(
        "DELETE FROM profile_experience; DELETE FROM profile_education;
         DELETE FROM profile_languages; DELETE FROM profile_links; DELETE FROM profile_fields;",
    )?;
    for (i, e) in profile.experience.iter().enumerate() {
        conn.execute(
            "INSERT INTO profile_experience
                 (position, title, company, location, start_date, end_date, current, description)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                i as i64,
                e.title,
                e.company,
                e.location,
                e.start,
                e.end,
                e.current,
                e.description
            ],
        )?;
    }
    for (i, e) in profile.education.iter().enumerate() {
        conn.execute(
            "INSERT INTO profile_education
                 (position, school, degree, field, start_date, end_date, description)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                i as i64,
                e.school,
                e.degree,
                e.field,
                e.start,
                e.end,
                e.description
            ],
        )?;
    }
    for (i, l) in profile.languages.iter().enumerate() {
        conn.execute(
            "INSERT INTO profile_languages (position, name, level) VALUES (?1, ?2, ?3)",
            params![i as i64, l.name, l.level],
        )?;
    }
    for (i, l) in profile.other_links.iter().enumerate() {
        conn.execute(
            "INSERT INTO profile_links (position, label, url) VALUES (?1, ?2, ?3)",
            params![i as i64, l.label, l.url],
        )?;
    }
    for (i, f) in profile.custom_fields.iter().enumerate() {
        conn.execute(
            "INSERT INTO profile_fields (position, label, kind, value, document_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![i as i64, f.label, f.kind.as_str(), f.value, f.document_id],
        )?;
    }
    Ok(())
}

// ── Documents ───────────────────────────────────────────────────────

const DOCUMENT_COLUMNS: &str = "id, name, kind, format, original_name, size, text IS NOT NULL,
    created_at, updated_at";

fn document_from_row(row: &Row) -> rusqlite::Result<ProfileDocument> {
    let kind: String = row.get(2)?;
    let format: String = row.get(3)?;
    Ok(ProfileDocument {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: DocumentKind::parse(&kind).unwrap_or(DocumentKind::Other),
        format: DocumentFormat::parse(&format).unwrap_or(DocumentFormat::Text),
        original_name: row.get(4)?,
        size: row.get(5)?,
        has_text: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

/// Newest first.
pub fn list_documents(conn: &Connection) -> AppResult<Vec<ProfileDocument>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {DOCUMENT_COLUMNS} FROM profile_documents ORDER BY created_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], document_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get_document(conn: &Connection, id: i64) -> AppResult<ProfileDocument> {
    conn.query_row(
        &format!("SELECT {DOCUMENT_COLUMNS} FROM profile_documents WHERE id = ?1"),
        [id],
        document_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Document not found"))
}

/// The generated file name inside the documents folder.
pub fn document_file_name(conn: &Connection, id: i64) -> AppResult<String> {
    conn.query_row(
        "SELECT file_name FROM profile_documents WHERE id = ?1",
        [id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Document not found"))
}

pub fn document_text(conn: &Connection, id: i64) -> AppResult<Option<String>> {
    conn.query_row(
        "SELECT text FROM profile_documents WHERE id = ?1",
        [id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Document not found"))
}

pub struct NewDocument<'a> {
    pub name: &'a str,
    pub kind: DocumentKind,
    pub format: DocumentFormat,
    pub original_name: &'a str,
    pub file_name: &'a str,
    pub size: i64,
    pub sha256: &'a str,
    pub text: Option<&'a str>,
    pub now: i64,
}

pub fn insert_document(conn: &Connection, doc: NewDocument) -> AppResult<ProfileDocument> {
    conn.execute(
        "INSERT INTO profile_documents
             (name, kind, format, original_name, file_name, size, sha256, text, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            doc.name,
            doc.kind.as_str(),
            doc.format.as_str(),
            doc.original_name,
            doc.file_name,
            doc.size,
            doc.sha256,
            doc.text,
            doc.now,
        ],
    )?;
    get_document(conn, conn.last_insert_rowid())
}

pub fn update_document(
    conn: &Connection,
    id: i64,
    name: &str,
    kind: DocumentKind,
    now: i64,
) -> AppResult<()> {
    let updated = conn.execute(
        "UPDATE profile_documents SET name = ?2, kind = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, name, kind.as_str(), now],
    )?;
    if updated == 0 {
        return Err(AppError::not_found("Document not found"));
    }
    Ok(())
}

/// Deletes the row and returns its file name. Custom fields pointing at it
/// keep their label and lose the file.
pub fn delete_document(conn: &Connection, id: i64) -> AppResult<String> {
    let file_name = document_file_name(conn, id)?;
    conn.execute("DELETE FROM profile_documents WHERE id = ?1", [id])?;
    Ok(file_name)
}

pub fn document_exists(conn: &Connection, id: i64) -> AppResult<bool> {
    Ok(conn
        .prepare("SELECT 1 FROM profile_documents WHERE id = ?1")?
        .exists([id])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    pub fn sample() -> Profile {
        Profile {
            first_name: "Ana".into(),
            last_name: "Tester".into(),
            email: "ana@example.com".into(),
            title: "Data Engineer".into(),
            skills: vec!["Rust".into(), "SQL".into()],
            experience: vec![
                Experience {
                    title: "Data Engineer".into(),
                    company: "Globex".into(),
                    start: "2021".into(),
                    current: true,
                    ..Experience::default()
                },
                Experience {
                    title: "Intern".into(),
                    company: "Acme".into(),
                    ..Experience::default()
                },
            ],
            education: vec![Education {
                school: "TU Wien".into(),
                degree: "MSc".into(),
                ..Education::default()
            }],
            languages: vec![Language {
                name: "German".into(),
                level: "Native".into(),
            }],
            github: "https://github.com/ana".into(),
            other_links: vec![ProfileLink {
                label: "Blog".into(),
                url: "https://ana.dev/blog".into(),
            }],
            custom_fields: vec![CustomField {
                label: "Research profile".into(),
                kind: CustomFieldKind::Url,
                value: "https://orcid.org/0000".into(),
                document_id: None,
            }],
            ..Profile::default()
        }
    }

    #[test]
    fn round_trips_the_profile_in_order() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            assert_eq!(get(c)?, (Profile::default(), None));
            save(c, &sample(), 5)?;
            assert_eq!(get(c)?, (sample(), Some(5)));

            // Saving again replaces every list.
            let mut edited = sample();
            edited.experience.reverse();
            edited.languages.clear();
            save(c, &edited, 6)?;
            assert_eq!(get(c)?, (edited, Some(6)));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn deleting_a_document_keeps_the_custom_field() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let doc = insert_document(
                c,
                NewDocument {
                    name: "Kubernetes certificate",
                    kind: DocumentKind::Certificate,
                    format: DocumentFormat::Pdf,
                    original_name: "cka.pdf",
                    file_name: "abc.pdf",
                    size: 10,
                    sha256: "00",
                    text: Some("CKA"),
                    now: 1,
                },
            )?;
            assert!(doc.has_text);
            let mut profile = sample();
            profile.custom_fields.push(CustomField {
                label: "Certificate".into(),
                kind: CustomFieldKind::File,
                value: String::new(),
                document_id: Some(doc.id),
            });
            save(c, &profile, 2)?;

            assert_eq!(delete_document(c, doc.id)?, "abc.pdf");
            let (stored, _) = get(c)?;
            let field = stored.custom_fields.last().unwrap();
            assert_eq!(field.label, "Certificate");
            assert_eq!(field.document_id, None);
            assert!(list_documents(c)?.is_empty());
            Ok(())
        })
        .unwrap();
    }
}
