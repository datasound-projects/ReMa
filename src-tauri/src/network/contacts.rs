//! The user's own contacts for Network Connect: LinkedIn connections from
//! the user's LinkedIn data export (`Connections.csv`, or the export's ZIP)
//! and vCard files (a contact saved from XING, or an address book export).
//!
//! LinkedIn shares a member's connection list with apps only after it
//! approves them for its Connections API, and XING offers no API for
//! desktop apps at all; the member's own data export is what both leave to
//! the member. The user picks the file; ReMa keeps only what it matches on
//! (name, company, position, profile link, when connected) on this
//! computer, never e-mail addresses or phone numbers, matches them to
//! companies itself and never sends them to a model (see `policy`).

use std::{
    io::Read,
    path::{Path, PathBuf},
};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    policy::{self, DataClass, DataSource, Operation, Purpose},
    relationships::{Member, Origin},
};
use crate::{error::AppResult, state::AppState, time::now_ms};

/// Where imported contacts came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ContactSource {
    /// LinkedIn's data export (`Connections.csv`).
    LinkedinExport,
    /// vCard files (XING, an address book).
    Vcard,
}

impl ContactSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::LinkedinExport => "linkedin_export",
            Self::Vcard => "vcard",
        }
    }

    fn origin(self) -> Origin {
        match self {
            Self::LinkedinExport => Origin::LinkedinExport,
            Self::Vcard => Origin::Vcard,
        }
    }
}

/// One contact, as kept.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Contact {
    pub name: String,
    pub company: String,
    pub position: String,
    pub profile_url: Option<String>,
    pub connected_on: Option<String>,
}

/// What Settings shows about imported contacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactsSummary {
    pub linkedin: u32,
    pub vcard: u32,
    pub last_imported_at: Option<i64>,
}

/// The result of one import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactsImported {
    /// Contacts read from the files.
    pub read: u32,
    /// Files that held no contacts ReMa could read, with the reason.
    pub problems: Vec<String>,
    pub summary: ContactsSummary,
}

/// Most contacts kept (LinkedIn allows 30,000 connections).
const MAX_CONTACTS: usize = 30_000;
/// Largest file read.
const MAX_FILE: u64 = 50 * 1024 * 1024;
const MAX_FIELD: usize = 200;

fn clean(value: &str) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    value.chars().take(MAX_FIELD).collect()
}

/// An http(s) link, or nothing.
fn link(value: &str) -> Option<String> {
    let value = value.trim();
    let parsed = reqwest::Url::parse(value).ok()?;
    matches!(parsed.scheme(), "https" | "http").then(|| value.chars().take(500).collect())
}

/// RFC 4180 CSV: quoted fields with commas, doubled quotes and line breaks.
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Connections from LinkedIn's `Connections.csv` (notes above the header
/// are skipped; columns are found by name).
pub fn linkedin_connections(text: &str) -> Result<Vec<Contact>, String> {
    let rows = parse_csv(text);
    let Some(header_at) = rows
        .iter()
        .position(|r| r.first().is_some_and(|c| c.trim() == "First Name"))
    else {
        return Err("It is not LinkedIn's Connections.csv (no “First Name” column).".into());
    };
    let header: Vec<String> = rows[header_at]
        .iter()
        .map(|h| h.trim().to_string())
        .collect();
    let column = |name: &str| header.iter().position(|h| h == name);
    let (first, last) = (column("First Name"), column("Last Name"));
    let (url, company, position, connected) = (
        column("URL"),
        column("Company"),
        column("Position"),
        column("Connected On"),
    );
    let cell = |row: &[String], at: Option<usize>| {
        at.and_then(|i| row.get(i))
            .map(|v| clean(v))
            .unwrap_or_default()
    };
    Ok(rows[header_at + 1..]
        .iter()
        .filter_map(|row| {
            let name = clean(&format!("{} {}", cell(row, first), cell(row, last)));
            (!name.is_empty()).then(|| Contact {
                name,
                company: cell(row, company),
                position: cell(row, position),
                profile_url: link(&cell(row, url)),
                connected_on: Some(cell(row, connected)).filter(|c| !c.is_empty()),
            })
        })
        .take(MAX_CONTACTS)
        .collect())
}

/// `Connections.csv` from the export's ZIP, or the CSV itself.
pub fn read_linkedin_file(path: &Path) -> Result<Vec<Contact>, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE {
        return Err("The file is too large.".into());
    }
    let is_zip = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    let text = if is_zip {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|_| "The ZIP file could not be read.")?;
        let name = archive
            .file_names()
            .find(|n| {
                n.rsplit('/')
                    .next()
                    .is_some_and(|base| base.eq_ignore_ascii_case("Connections.csv"))
            })
            .map(str::to_string)
            .ok_or("The export has no Connections.csv. Ask LinkedIn for “Connections”.")?;
        let entry = archive.by_name(&name).map_err(|e| e.to_string())?;
        let mut text = String::new();
        entry
            .take(MAX_FILE)
            .read_to_string(&mut text)
            .map_err(|_| "Connections.csv could not be read.")?;
        text
    } else {
        std::fs::read_to_string(path).map_err(|_| "The file could not be read as text.")?
    };
    linkedin_connections(&text)
}

/// A vCard value with its escapes removed (`\,` `\;` `\n` `\\`).
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push(' '),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Contacts in vCard text (versions 2.1, 3.0 and 4.0; one or many cards).
pub fn parse_vcards(text: &str) -> Vec<Contact> {
    // Folded lines continue with a space or a tab.
    let mut lines: Vec<String> = Vec::new();
    for raw in text.trim_start_matches('\u{feff}').lines() {
        match raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')) {
            Some(rest) if !lines.is_empty() => lines.last_mut().unwrap().push_str(rest),
            _ => lines.push(raw.to_string()),
        }
    }
    let mut contacts = Vec::new();
    let mut card: Option<(Contact, Option<String>, Vec<String>)> = None;
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.to_ascii_uppercase();
        // "item1.URL;TYPE=WORK" → "URL"
        let name = key
            .split(';')
            .next()
            .unwrap_or_default()
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_string();
        match name.as_str() {
            "BEGIN" if value.trim().eq_ignore_ascii_case("VCARD") => {
                card = Some((Contact::default(), None, Vec::new()));
            }
            "END" if value.trim().eq_ignore_ascii_case("VCARD") => {
                if let Some((mut contact, structured, urls)) = card.take() {
                    if contact.name.is_empty() {
                        contact.name = structured.unwrap_or_default();
                    }
                    contact.profile_url = urls
                        .iter()
                        .find(|u| u.contains("xing.com") || u.contains("linkedin.com"))
                        .or(urls.first())
                        .cloned();
                    if !contact.name.is_empty() && contacts.len() < MAX_CONTACTS {
                        contacts.push(contact);
                    }
                }
            }
            _ => {
                let Some((contact, structured, urls)) = card.as_mut() else {
                    continue;
                };
                match name.as_str() {
                    "FN" => contact.name = clean(&unescape(value)),
                    "N" => {
                        // Family;Given;Additional;Prefix;Suffix
                        let parts: Vec<String> = value.split(';').map(unescape).collect();
                        let given = parts.get(1).cloned().unwrap_or_default();
                        let family = parts.first().cloned().unwrap_or_default();
                        *structured =
                            Some(clean(&format!("{given} {family}"))).filter(|n| !n.is_empty());
                    }
                    "ORG" => {
                        contact.company = clean(&unescape(value.split(';').next().unwrap_or("")))
                    }
                    "TITLE" => contact.position = clean(&unescape(value)),
                    "URL" => {
                        if let Some(url) = link(&unescape(value)) {
                            urls.push(url);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    contacts
}

fn read_vcard_file(path: &Path) -> Result<Vec<Contact>, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE {
        return Err("The file is too large.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let found = parse_vcards(&String::from_utf8_lossy(&bytes));
    if found.is_empty() {
        return Err("No contacts with a name were found in it.".into());
    }
    Ok(found)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Imports the picked files. A LinkedIn export replaces the earlier one
/// (it is the whole list); vCards add to what is there.
pub fn import(
    state: &AppState,
    source: ContactSource,
    paths: &[PathBuf],
) -> AppResult<ContactsImported> {
    policy::require(
        DataSource::ContactsImport,
        DataClass::FirstDegreeConnection,
        Purpose::ProfessionalResearch,
        Operation::Store,
    )?;
    let mut contacts = Vec::new();
    let mut problems = Vec::new();
    for path in paths {
        let read = match source {
            ContactSource::LinkedinExport => read_linkedin_file(path),
            ContactSource::Vcard => read_vcard_file(path),
        };
        match read {
            Ok(found) => contacts.extend(found),
            Err(reason) => problems.push(format!("{}: {reason}", file_name(path))),
        }
    }
    let read = contacts.len() as u32;
    if !contacts.is_empty() {
        let now = now_ms();
        state.db.call(|c| {
            let tx = c.transaction()?;
            if source == ContactSource::LinkedinExport {
                tx.execute(
                    "DELETE FROM network_contacts WHERE source = ?1",
                    [source.as_str()],
                )?;
            }
            {
                let mut insert = tx.prepare(
                    "INSERT INTO network_contacts
                        (source, name, company, position, profile_url, connected_on, imported_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (source, name, company) DO UPDATE SET
                        position = excluded.position,
                        profile_url = COALESCE(excluded.profile_url, profile_url),
                        connected_on = COALESCE(excluded.connected_on, connected_on),
                        imported_at = excluded.imported_at",
                )?;
                for contact in &contacts {
                    insert.execute(params![
                        source.as_str(),
                        contact.name,
                        contact.company,
                        contact.position,
                        contact.profile_url,
                        contact.connected_on,
                        now
                    ])?;
                }
            }
            tx.commit()?;
            Ok(())
        })?;
    }
    Ok(ContactsImported {
        read,
        problems,
        summary: summary(state)?,
    })
}

pub fn summary(state: &AppState) -> AppResult<ContactsSummary> {
    state.db.call(|c| {
        let count = |source: ContactSource| -> AppResult<u32> {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM network_contacts WHERE source = ?1",
                [source.as_str()],
                |r| r.get(0),
            )?)
        };
        Ok(ContactsSummary {
            linkedin: count(ContactSource::LinkedinExport)?,
            vcard: count(ContactSource::Vcard)?,
            last_imported_at: c.query_row(
                "SELECT MAX(imported_at) FROM network_contacts",
                [],
                |r| r.get(0),
            )?,
        })
    })
}

/// Removes imported contacts (one source, or all).
pub fn clear(state: &AppState, source: Option<ContactSource>) -> AppResult<ContactsSummary> {
    state.db.call(|c| {
        match source {
            Some(source) => c.execute(
                "DELETE FROM network_contacts WHERE source = ?1",
                [source.as_str()],
            )?,
            None => c.execute("DELETE FROM network_contacts", [])?,
        };
        Ok(())
    })?;
    summary(state)
}

/// Imported contacts as connection-list members ("Position at Company").
pub fn members(state: &AppState) -> AppResult<Vec<Member>> {
    state.db.call(|c| {
        let mut statement = c.prepare(
            "SELECT source, name, company, position, profile_url FROM network_contacts
             ORDER BY id",
        )?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(source, name, company, position, profile_url)| {
                let source = if source == ContactSource::Vcard.as_str() {
                    ContactSource::Vcard
                } else {
                    ContactSource::LinkedinExport
                };
                let headline = match (position.is_empty(), company.is_empty()) {
                    (_, true) if position.is_empty() => None,
                    (_, true) => Some(position),
                    (true, false) => Some(company),
                    (false, false) => Some(format!("{position} at {company}")),
                };
                Member {
                    name,
                    headline,
                    profile_url,
                    origin: source.origin(),
                }
            })
            .collect())
    })
}

#[cfg(test)]
mod tests {
    use std::{io::Write, sync::Arc};

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing};

    const CONNECTIONS_CSV: &str = "Notes:\n\"When exporting your connection data, you may notice that some of the email addresses are missing, because members control who sees them.\"\n\nFirst Name,Last Name,URL,Email Address,Company,Position,Connected On\nJane,Example,https://www.linkedin.com/in/jane-example,jane@example.com,Nordlicht AI,\"Senior ML Engineer, Platform\",12 Mar 2021\nMax,Muster,,,,,01 Jan 2020\n,,,,,,\n\"Anna \"\"Ann\"\"\",Beispiel,https://www.linkedin.com/in/anna,,Donau Data,Talent Acquisition Partner,05 May 2024\n";

    #[test]
    fn reads_linkedins_connections_csv_after_its_notes() {
        let found = linkedin_connections(CONNECTIONS_CSV).unwrap();
        assert_eq!(found.len(), 3, "rows without a name are skipped");
        assert_eq!(found[0].name, "Jane Example");
        assert_eq!(found[0].company, "Nordlicht AI");
        assert_eq!(found[0].position, "Senior ML Engineer, Platform");
        assert_eq!(
            found[0].profile_url.as_deref(),
            Some("https://www.linkedin.com/in/jane-example")
        );
        assert_eq!(found[0].connected_on.as_deref(), Some("12 Mar 2021"));
        assert_eq!(found[2].name, "Anna \"Ann\" Beispiel");
        assert!(linkedin_connections("Name,Title\nA,B\n").is_err());
    }

    #[test]
    fn reads_the_csv_inside_the_export_zip() {
        let dir = testing::temp_dir();
        let path = dir.join("Basic_LinkedInDataExport_09-28-2026.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("Profile.csv", options).unwrap();
        zip.write_all(b"First Name\nAna\n").unwrap();
        zip.start_file("Connections.csv", options).unwrap();
        zip.write_all(CONNECTIONS_CSV.as_bytes()).unwrap();
        zip.finish().unwrap();
        assert_eq!(read_linkedin_file(&path).unwrap().len(), 3);

        let empty = dir.join("other.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&empty).unwrap());
        zip.start_file("Profile.csv", options).unwrap();
        zip.finish().unwrap();
        assert!(read_linkedin_file(&empty)
            .unwrap_err()
            .contains("Connections.csv"));
    }

    #[test]
    fn reads_vcards_from_xing_and_address_books() {
        let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Gruber;Lukas;;;\r\nFN:Lukas Gruber\r\nORG:Donau Data GmbH;Recruiting\r\nTITLE:Talent Acquisition Partner\r\nitem1.URL;type=pref:https://www.xing.com/profile/Lukas_Gruber\r\nEMAIL;TYPE=WORK:lukas@donau.example\r\nTEL:+43 1 234\r\nNOTE:A long\r\n  folded note\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:4.0\r\nN:Muster;Erika\\, Dr.;;;\r\nURL:javascript:alert(1)\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:2.1\r\nORG:No Name Inc\r\nEND:VCARD\r\n";
        let found = parse_vcards(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "Lukas Gruber");
        assert_eq!(found[0].company, "Donau Data GmbH");
        assert_eq!(found[0].position, "Talent Acquisition Partner");
        assert_eq!(
            found[0].profile_url.as_deref(),
            Some("https://www.xing.com/profile/Lukas_Gruber")
        );
        assert_eq!(found[1].name, "Erika, Dr. Muster");
        assert_eq!(found[1].profile_url, None, "only web links are kept");
    }

    #[test]
    fn imports_replaces_the_linkedin_list_and_keeps_no_contact_details() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let dir = testing::temp_dir();
        let csv = dir.join("Connections.csv");
        std::fs::write(&csv, CONNECTIONS_CSV).unwrap();
        let bad = dir.join("notes.csv");
        std::fs::write(&bad, "Just text").unwrap();

        let result = import(&state, ContactSource::LinkedinExport, &[csv.clone(), bad]).unwrap();
        assert_eq!(result.read, 3);
        assert_eq!(result.problems.len(), 1);
        assert!(result.problems[0].starts_with("notes.csv: "));
        assert_eq!(result.summary.linkedin, 3);

        // A newer export replaces the list.
        std::fs::write(
            &csv,
            "First Name,Last Name,URL,Email Address,Company,Position,Connected On\nJane,Example,,,Nordlicht AI,CTO,\n",
        )
        .unwrap();
        let again = import(&state, ContactSource::LinkedinExport, &[csv]).unwrap();
        assert_eq!(again.summary.linkedin, 1);

        let vcf = dir.join("lukas.vcf");
        std::fs::write(
            &vcf,
            "BEGIN:VCARD\nFN:Lukas Gruber\nORG:Donau Data\nEMAIL:lukas@donau.example\nEND:VCARD\n",
        )
        .unwrap();
        let cards = import(&state, ContactSource::Vcard, std::slice::from_ref(&vcf)).unwrap();
        assert_eq!((cards.summary.linkedin, cards.summary.vcard), (1, 1));
        // The same card again adds nothing.
        assert_eq!(
            import(&state, ContactSource::Vcard, &[vcf])
                .unwrap()
                .summary
                .vcard,
            1
        );

        let members = members(&state).unwrap();
        assert_eq!(members[0].headline.as_deref(), Some("CTO at Nordlicht AI"));
        assert_eq!(members[1].origin, Origin::Vcard);
        let stored: String = state
            .db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT group_concat(name || company || position || coalesce(profile_url, ''))
                     FROM network_contacts",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert!(!stored.contains('@'), "no e-mail addresses are kept");

        assert_eq!(clear(&state, None).unwrap().linkedin, 0);
        assert!(members_empty(&state));
    }

    #[test]
    fn the_same_export_imported_twice_or_twice_at_once_is_kept_once() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let dir = testing::temp_dir();
        let csv = dir.join("Connections.csv");
        std::fs::write(&csv, CONNECTIONS_CSV).unwrap();
        let zip_path = dir.join("Basic_LinkedInDataExport.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        zip.start_file("Connections.csv", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(CONNECTIONS_CSV.as_bytes()).unwrap();
        zip.finish().unwrap();

        for _ in 0..2 {
            let result = import(
                &state,
                ContactSource::LinkedinExport,
                std::slice::from_ref(&csv),
            )
            .unwrap();
            assert_eq!(result.summary.linkedin, 3);
        }
        // The ZIP and the CSV taken out of it, picked together.
        let both = import(&state, ContactSource::LinkedinExport, &[zip_path, csv]).unwrap();
        assert_eq!(both.read, 6);
        assert_eq!(both.summary.linkedin, 3);
        let rows: i64 = state
            .db
            .call(|c| Ok(c.query_row("SELECT count(*) FROM network_contacts", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(rows, 3);
    }

    fn members_empty(state: &AppState) -> bool {
        members(state).unwrap().is_empty()
    }
}
