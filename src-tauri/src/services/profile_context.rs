//! Profile Context: a compact, provider-neutral description of the user's
//! profile for a model.
//!
//! Only built when the user asked for it (the Chat "Profile" toggle, or a
//! scheduled task set to include the profile). It is made from the
//! structured profile only: document contents are never included, and
//! neither are email address or phone number.

use crate::{
    db::profile as repo,
    error::AppResult,
    models::profile::{CustomFieldKind, DocumentKind, Profile, ProfileDocument},
    state::AppState,
};

/// Upper bound for the whole context.
pub const MAX_CHARS: usize = 8_000;
const MAX_DESCRIPTION: usize = 300;
const MAX_SUMMARY: usize = 1_200;
const MAX_ENTRIES: usize = 12;

/// The context for the current profile, or `None` if the profile is empty.
pub fn load(state: &AppState) -> AppResult<Option<String>> {
    let (profile, documents) = state.db.call(|c| {
        let (profile, _) = repo::get(c)?;
        Ok((profile, repo::list_documents(c)?))
    })?;
    Ok(build(&profile, &documents))
}

fn short(value: &str, max: usize) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() <= max {
        value
    } else {
        format!(
            "{}…",
            value.chars().take(max).collect::<String>().trim_end()
        )
    }
}

fn join_nonempty(parts: &[&str], separator: &str) -> String {
    parts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(separator)
}

fn period(start: &str, end: &str, current: bool) -> String {
    let end = if current { "present" } else { end };
    match (start.trim(), end.trim()) {
        ("", "") => String::new(),
        (s, "") => s.to_string(),
        ("", e) => format!("until {e}"),
        (s, e) => format!("{s} – {e}"),
    }
}

fn kind_label(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Cv => "CV",
        DocumentKind::Certificate => "certificate",
        DocumentKind::Portfolio => "portfolio",
        DocumentKind::Other => "document",
    }
}

pub fn build(p: &Profile, documents: &[ProfileDocument]) -> Option<String> {
    if p.is_empty() && documents.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    let add = |lines: &mut Vec<String>, label: &str, value: String| {
        if !value.trim().is_empty() {
            lines.push(format!("{label}: {value}"));
        }
    };

    add(&mut lines, "Name", p.full_name());
    add(&mut lines, "Professional title", short(&p.title, 200));
    add(&mut lines, "Location", short(&p.location, 200));
    add(&mut lines, "Summary", short(&p.summary, MAX_SUMMARY));
    add(&mut lines, "Skills", p.skills.join(", "));

    if !p.experience.is_empty() {
        let mut block = vec!["Experience:".to_string()];
        for e in p.experience.iter().take(MAX_ENTRIES) {
            let role = match (e.title.trim(), e.company.trim()) {
                ("", c) => c.to_string(),
                (t, "") => t.to_string(),
                (t, c) => format!("{t} at {c}"),
            };
            let meta = join_nonempty(&[&e.location, &period(&e.start, &e.end, e.current)], ", ");
            let mut item = format!("- {role}");
            if !meta.is_empty() {
                item.push_str(&format!(" ({meta})"));
            }
            if !e.description.trim().is_empty() {
                item.push_str(&format!(": {}", short(&e.description, MAX_DESCRIPTION)));
            }
            block.push(item);
        }
        lines.push(block.join("\n"));
    }

    if !p.education.is_empty() {
        let mut block = vec!["Education:".to_string()];
        for e in p.education.iter().take(MAX_ENTRIES) {
            let degree = join_nonempty(&[&e.degree, &e.field], " in ");
            let mut item = format!("- {}", join_nonempty(&[&degree, &e.school], ", "));
            let when = period(&e.start, &e.end, false);
            if !when.is_empty() {
                item.push_str(&format!(" ({when})"));
            }
            if !e.description.trim().is_empty() {
                item.push_str(&format!(": {}", short(&e.description, MAX_DESCRIPTION)));
            }
            block.push(item);
        }
        lines.push(block.join("\n"));
    }

    let languages: Vec<String> = p
        .languages
        .iter()
        .map(|l| {
            if l.level.trim().is_empty() {
                l.name.clone()
            } else {
                format!("{} ({})", l.name, l.level)
            }
        })
        .collect();
    add(&mut lines, "Languages", languages.join(", "));

    let mut links: Vec<String> = [
        ("Website", &p.website),
        ("Resume website", &p.resume_website),
        ("GitHub", &p.github),
        ("LinkedIn", &p.linkedin),
    ]
    .iter()
    .filter(|(_, url)| !url.is_empty())
    .map(|(label, url)| format!("{label} {url}"))
    .collect();
    links.extend(
        p.other_links
            .iter()
            .map(|l| format!("{} {}", l.label, l.url).trim().to_string()),
    );
    add(&mut lines, "Links", links.join(", "));

    for field in &p.custom_fields {
        match field.kind {
            CustomFieldKind::Text | CustomFieldKind::Url => {
                add(
                    &mut lines,
                    &field.label,
                    short(&field.value, MAX_DESCRIPTION),
                );
            }
            CustomFieldKind::File => {
                if let Some(doc) = field
                    .document_id
                    .and_then(|id| documents.iter().find(|d| d.id == id))
                {
                    add(
                        &mut lines,
                        &field.label,
                        format!("\"{}\" (file on record)", doc.name),
                    );
                }
            }
        }
    }

    let files: Vec<String> = documents
        .iter()
        .map(|d| format!("{} ({})", d.name, kind_label(d.kind)))
        .collect();
    add(
        &mut lines,
        "Documents on file (contents not included)",
        files.join(", "),
    );

    let mut body = lines.join("\n");
    if body.chars().count() > MAX_CHARS {
        body = format!("{}…", body.chars().take(MAX_CHARS).collect::<String>());
    }
    Some(format!(
        "The user has turned on their ReMa Profile for this conversation. Use it to \
         personalize your answers when relevant, without asking for details it already \
         contains. Do not repeat the profile back unless asked. Contact details are not \
         included.\n\n<user_profile>\n{body}\n</user_profile>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::profile::{CustomField, Education, Experience, Language, ProfileLink};

    fn profile() -> Profile {
        Profile {
            first_name: "Ana".into(),
            last_name: "Tester".into(),
            email: "ana@example.com".into(),
            phone: "+43 660 1234567".into(),
            location: "Vienna, Austria".into(),
            title: "Data Engineer".into(),
            summary: "Builds data platforms.".into(),
            skills: vec!["Rust".into(), "Kubernetes".into()],
            experience: vec![Experience {
                title: "Senior Data Engineer".into(),
                company: "Globex".into(),
                start: "2021".into(),
                current: true,
                description: "x".repeat(1_000),
                ..Experience::default()
            }],
            education: vec![Education {
                school: "TU Wien".into(),
                degree: "MSc".into(),
                field: "Computer Science".into(),
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
                label: "Salary expectation".into(),
                kind: CustomFieldKind::Text,
                value: "€100k".into(),
                document_id: None,
            }],
            ..Profile::default()
        }
    }

    #[test]
    fn describes_the_structured_profile_compactly() {
        let context = build(&profile(), &[]).unwrap();
        for expected in [
            "Name: Ana Tester",
            "Professional title: Data Engineer",
            "Skills: Rust, Kubernetes",
            "- Senior Data Engineer at Globex (2021 – present): xxx",
            "- MSc in Computer Science, TU Wien",
            "Languages: German (Native)",
            "GitHub https://github.com/ana, Blog https://ana.dev/blog",
            "Salary expectation: €100k",
        ] {
            assert!(
                context.contains(expected),
                "missing {expected:?} in\n{context}"
            );
        }
        // No contact details, descriptions are shortened.
        assert!(!context.contains("ana@example.com"));
        assert!(!context.contains("660"));
        assert!(!context.contains(&"x".repeat(400)));
    }

    #[test]
    fn empty_profiles_add_nothing_and_size_is_bounded() {
        assert_eq!(build(&Profile::default(), &[]), None);
        let mut big = profile();
        big.skills = (0..2_000).map(|i| format!("skill-{i}")).collect();
        let context = build(&big, &[]).unwrap();
        assert!(context.chars().count() < MAX_CHARS + 500);
    }
}
