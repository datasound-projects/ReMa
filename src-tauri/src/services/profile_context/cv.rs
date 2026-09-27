//! CV text → facts, deterministically. A CV is split at its section
//! headings ("Experience", "SKILLS", "## Education", …); list sections
//! become short items, the others become entries (a heading line with its
//! bullets). `Label: value` lines are classified on their own. A CV without
//! recognizable headings is kept as one block of text.

use super::{
    labels::{self, Label},
    Field,
};

/// One fact read from a CV, before precedence and deduplication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub field: Field,
    pub text: String,
    /// Identity facts: which one ("name", "headline", "header").
    pub key: Option<&'static str>,
}

/// A CV read into facts, plus its blocks (full text of each entry) for
/// relevant excerpts.
#[derive(Debug, Clone, Default)]
pub struct Normalized {
    pub items: Vec<Item>,
    /// `(section heading, text)` for every entry, untruncated.
    pub blocks: Vec<(String, String)>,
    /// No section heading was found.
    pub unstructured: bool,
}

/// Where a line starts a section, which field it opens.
fn heading(line: &str) -> Option<Label> {
    let text = line
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches(|c: char| matches!(c, '*' | '_' | '=' | '-' | '—' | '–' | '|' | ':' | ' '));
    // A colon inside ("Location: Vienna") makes it a `Label: value` line.
    if text.is_empty()
        || text.chars().count() > 40
        || text.split_whitespace().count() > 5
        || text
            .chars()
            .any(|c| c.is_ascii_digit() || matches!(c, '@' | ',' | '.' | '(' | '/' | ':'))
    {
        return None;
    }
    match labels::classify(text) {
        Label::Unknown => None,
        // "Name" or "Title" alone is not a section.
        Label::Field(Field::Identity) => None,
        label => Some(label),
    }
}

/// `Label: value` on one line, with a short label.
fn label_value(line: &str) -> Option<(Label, &str, &str)> {
    let (label, value) = line.split_once(':')?;
    let label = label.trim().trim_start_matches(['-', '•', '*', '·']).trim();
    let value = value.trim();
    if label.is_empty()
        || value.is_empty()
        || label.chars().count() > 40
        || label.split_whitespace().count() > 5
        || value.starts_with("//")
    {
        return None;
    }
    match labels::classify(label) {
        Label::Unknown => None,
        class => Some((class, label, value)),
    }
}

fn is_bullet(line: &str) -> bool {
    line.starts_with(['-', '•', '*', '·', '–', '▪', '●', '◦'])
}

fn strip_bullet(line: &str) -> &str {
    line.trim_start_matches(['-', '•', '*', '·', '–', '▪', '●', '◦'])
        .trim()
}

/// Short list items: "Python, Rust | SQL • Docker".
pub fn list_items(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    for line in text.lines() {
        let line = strip_bullet(line.trim());
        // "Programming: Python, Rust" → the values.
        let line = match line.split_once(':') {
            Some((label, rest))
                if label.split_whitespace().count() <= 4 && !rest.trim().is_empty() =>
            {
                rest
            }
            _ => line,
        };
        for part in line.split([',', ';', '|', '•', '·']) {
            let part = part.trim().trim_end_matches('.').trim();
            if !part.is_empty() && part.chars().count() <= 60 {
                items.push(part.to_string());
            }
        }
    }
    items
}

/// The parts of a short list ("AI Engineer, Forward Deployed Engineer"),
/// or `None` for prose, which stays one fact.
pub fn short_list(text: &str) -> Option<Vec<String>> {
    let prose = text.contains(". ") || text.trim_end().ends_with(['.', '!', '?']);
    let parts: Vec<String> = text
        .split(['\n', ',', ';', '|', '•'])
        .map(|p| strip_bullet(p.trim()).trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    (!prose && parts.len() > 1 && parts.iter().all(|p| p.split_whitespace().count() <= 6))
        .then_some(parts)
}

/// Entries: a line that is not a bullet starts one; bullets and
/// continuation lines join it. Blank lines end it.
fn entries(lines: &[&str]) -> Vec<String> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut open = false;
    let mut last_bullet = false;
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            open = false;
            continue;
        }
        let bullet = is_bullet(line);
        let starts = !open || (!bullet && last_bullet);
        if starts {
            out.push(Vec::new());
            open = true;
        }
        if let Some(entry) = out.last_mut() {
            entry.push(strip_bullet(line).to_string());
        }
        last_bullet = bullet;
    }
    out.into_iter()
        .map(|lines| lines.join("; "))
        .filter(|entry| !entry.trim().is_empty())
        .collect()
}

/// A name: two to four capitalized words, letters only.
fn looks_like_name(line: &str) -> bool {
    let words: Vec<&str> = line.split_whitespace().collect();
    (2..=4).contains(&words.len())
        && line.chars().count() <= 40
        && words.iter().all(|w| {
            w.chars().next().is_some_and(char::is_uppercase)
                && w.chars()
                    .all(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '.'))
        })
}

/// Only removed contact details and separators left.
fn is_contact_only(line: &str) -> bool {
    line.replace("[email removed]", "")
        .replace("[phone removed]", "")
        .chars()
        .all(|c| !c.is_alphanumeric())
}

/// Reads one CV's (already redacted) text.
pub fn normalize(text: &str) -> Normalized {
    let lines: Vec<&str> = text.lines().collect();
    let mut sections: Vec<(Label, String, Vec<&str>)> = Vec::new();
    let mut header: Vec<&str> = Vec::new();
    for line in &lines {
        if let Some(label) = heading(line) {
            sections.push((
                label,
                line.trim().trim_matches(['#', ':', ' ']).to_string(),
                Vec::new(),
            ));
        } else if let Some((_, _, body)) = sections.last_mut() {
            body.push(line);
        } else {
            header.push(line);
        }
    }

    let mut out = Normalized::default();
    let push = |out: &mut Normalized, field: Field, text: String, key: Option<&'static str>| {
        let text = text.trim().to_string();
        if !text.is_empty() {
            out.items.push(Item { field, text, key });
        }
    };

    // The header: name, headline, `Label: value` lines, the rest as is.
    let mut rest = Vec::new();
    let mut named = false;
    let mut headline = false;
    for line in header.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        if let Some((class, label, value)) = label_value(line) {
            match class {
                Label::Skip => {}
                Label::Field(field) => push(&mut out, field, format!("{label}: {value}"), None),
                Label::Unknown => rest.push(line),
            }
        } else if is_contact_only(line) {
        } else if !named && looks_like_name(line) {
            named = true;
            push(
                &mut out,
                Field::Identity,
                format!("Name: {line}"),
                Some("name"),
            );
        } else if named && !headline && line.chars().count() <= 80 {
            headline = true;
            push(
                &mut out,
                Field::Identity,
                format!("CV headline: {line}"),
                Some("headline"),
            );
        } else {
            rest.push(line);
        }
    }
    if !rest.is_empty() {
        let joined = rest.join(" · ");
        push(
            &mut out,
            Field::Identity,
            format!("CV header: {joined}"),
            Some("header"),
        );
    }

    if sections.is_empty() {
        out.unstructured = true;
        let body = lines
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !is_contact_only(l))
            .collect::<Vec<_>>()
            .join("\n");
        if !body.is_empty() {
            out.blocks.push((String::new(), body));
        }
        return out;
    }

    for (label, title, body) in sections {
        let Label::Field(field) = label else { continue };
        // `Label: value` lines inside a section are classified on their own
        // ("Salary expectation: …" under "Additional information").
        let mut own = Vec::new();
        for line in &body {
            match label_value(line.trim()) {
                Some((Label::Skip, ..)) => {}
                Some((Label::Field(other), label, value)) if other != field && !field.is_list() => {
                    push(&mut out, other, format!("{label}: {value}"), None);
                }
                _ => own.push(*line),
            }
        }
        let text = own.join("\n");
        if text.trim().is_empty() {
            continue;
        }
        if field.is_list() {
            for item in list_items(&text) {
                push(&mut out, field, item, None);
            }
            out.blocks.push((title, text.trim().to_string()));
        } else {
            for entry in entries(&own) {
                out.blocks.push((title.clone(), entry.clone()));
                push(&mut out, field, entry, None);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CV: &str = "Ana Tester\nSenior Data Scientist\nVienna, Austria · [email removed] · [phone removed]\n\
        Preferred location: Vienna\nDate of birth: 1 Jan 1990\n\n\
        SUMMARY\nData scientist with 8 years of experience in forecasting.\n\n\
        Work Experience\nGlobex GmbH — Senior Data Scientist, 2021 – present\n• Built demand forecasting for 40 markets\n\
        • Led a team of 4\nInitech — Data Analyst, 2017 – 2021\n- Reporting in SQL\n\n\
        Skills\nPython, SQL | Forecasting • Kubernetes\nProgramming: Rust, Go\n\n\
        Languages\nGerman (native), English (C1)\n\n\
        Education\nMSc Statistics, TU Wien, 2017\n";

    #[test]
    fn reads_sections_entries_and_items() {
        let cv = normalize(CV);
        assert!(!cv.unstructured);
        let of = |field: Field| -> Vec<&str> {
            cv.items
                .iter()
                .filter(|i| i.field == field)
                .map(|i| i.text.as_str())
                .collect()
        };
        assert_eq!(
            of(Field::Identity),
            [
                "Name: Ana Tester",
                "CV headline: Senior Data Scientist",
                "CV header: Vienna, Austria · [email removed] · [phone removed]"
            ]
        );
        assert_eq!(of(Field::Locations), ["Preferred location: Vienna"]);
        assert_eq!(
            of(Field::Experience),
            [
                "Globex GmbH — Senior Data Scientist, 2021 – present; Built demand forecasting for 40 markets; Led a team of 4",
                "Initech — Data Analyst, 2017 – 2021; Reporting in SQL",
            ]
        );
        assert_eq!(
            of(Field::Skills),
            ["Python", "SQL", "Forecasting", "Kubernetes", "Rust", "Go"]
        );
        assert_eq!(of(Field::Languages), ["German (native)", "English (C1)"]);
        assert_eq!(of(Field::Education), ["MSc Statistics, TU Wien, 2017"]);
        assert_eq!(
            of(Field::Summary),
            ["Data scientist with 8 years of experience in forecasting."]
        );
        // Sensitive details never become facts.
        assert!(cv.items.iter().all(|i| !i.text.contains("1990")));
    }

    #[test]
    fn text_without_headings_stays_one_block() {
        let cv = normalize("Ana Tester\nI build data platforms at Globex since 2021.\n");
        assert!(cv.unstructured);
        assert_eq!(cv.blocks.len(), 1);
        assert!(cv.blocks[0].1.contains("Globex"));
    }
}
