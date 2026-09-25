//! Reads job listings from Markdown tables in model answers.
//!
//! A table counts as a job table when its header names a role/title column
//! and a company or link column. Columns are recognized by their header
//! ("Company", "Employer", "Role", "Position", "Salary", "Key skills", …).
//! Salary columns labelled as estimates are ignored.

use std::sync::OnceLock;

use regex::Regex;

use super::{ingest::RawJob, normalize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Title,
    Company,
    Location,
    WorkMode,
    EmploymentType,
    Seniority,
    Salary,
    Posted,
    Link,
    Source,
    Requirements,
    Description,
    Reference,
    Ignore,
}

fn column(header: &str) -> Col {
    let h = header
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '#' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>();
    let words: Vec<&str> = h.split_whitespace().collect();
    let h = words.join(" ");
    // Stems and phrases match anywhere; short keys only as whole words.
    let has = |parts: &[&str]| parts.iter().any(|p| h.contains(p));
    let word = |keys: &[&str]| keys.iter().any(|k| words.contains(k));
    if h.is_empty() || ["#", "rank", "no", "nr", "n"].contains(&h.as_str()) {
        Col::Ignore
    } else if has(&[
        "salary",
        "compensation",
        "gehalt",
        "vergütung",
        "day rate",
        "hourly rate",
    ]) || word(&["pay", "rate", "tc", "wage", "wages"])
    {
        if has(&["estimat", "approx"])
            || word(&["est", "expected", "typical", "market", "guess", "likely"])
        {
            Col::Ignore
        } else {
            Col::Salary
        }
    } else if word(&[
        "why",
        "fit",
        "match",
        "matches",
        "relevance",
        "notes",
        "note",
        "comment",
        "comments",
        "score",
        "recommendation",
        "status",
        "verdict",
    ]) {
        Col::Ignore
    } else if has(&[
        "company",
        "employer",
        "organization",
        "organisation",
        "unternehmen",
        "arbeitgeber",
    ]) || word(&["firm", "hiring"])
    {
        Col::Company
    } else if has(&["job type", "employment", "contract"]) {
        Col::EmploymentType
    } else if has(&["job link", "job url"])
        || word(&["link", "links", "url", "apply", "posting", "listing"])
    {
        Col::Link
    } else if has(&["job id"]) || word(&["id", "ref", "reference", "req", "requisition"]) {
        Col::Reference
    } else if has(&["job board", "found on"])
        || word(&["source", "site", "platform", "via", "portal", "board"])
    {
        Col::Source
    } else if has(&["seniority", "career level", "career stage"]) || word(&["level"]) {
        Col::Seniority
    } else if h == "type" {
        Col::EmploymentType
    } else if has(&["start date", "deadline", "closing", "expires", "expiry"]) {
        Col::Ignore
    } else if has(&[
        "posted",
        "published",
        "veröffentlicht",
        "datum",
        "added",
        "listed",
    ]) || word(&["date", "age", "since", "when"])
    {
        Col::Posted
    } else if has(&["location", "standort", "country", "region", "office"])
        || word(&["city", "place", "where", "ort"])
    {
        Col::Location
    } else if has(&[
        "remote",
        "work mode",
        "workplace",
        "arrangement",
        "hybrid",
        "on site",
        "onsite",
        "work type",
        "work model",
    ]) || word(&["mode", "setup", "format"])
    {
        Col::WorkMode
    } else if has(&[
        "skill",
        "requirement",
        "stack",
        "technolog",
        "qualification",
        "must have",
        "experience",
        "competenc",
    ]) || word(&["tools", "tech", "key", "keywords"])
    {
        Col::Requirements
    } else if has(&[
        "description",
        "summary",
        "about",
        "responsibilit",
        "duties",
        "tasks",
    ]) {
        Col::Description
    } else if has(&["position", "title", "stelle", "opening", "vacancy"])
        || word(&["role", "roles", "job", "jobs", "rolle"])
    {
        Col::Title
    } else {
        Col::Ignore
    }
}

/// A cell's text with Markdown removed, and the links it contains.
struct Cell {
    text: String,
    urls: Vec<String>,
}

fn cell(raw: &str) -> Cell {
    static LINK: OnceLock<Regex> = OnceLock::new();
    static BARE: OnceLock<Regex> = OnceLock::new();
    static BR: OnceLock<Regex> = OnceLock::new();
    let link =
        LINK.get_or_init(|| Regex::new(r"\[([^\]]*)\]\(\s*<?([^)\s>]+)>?(?:\s+[^)]*)?\)").unwrap());
    let bare = BARE.get_or_init(|| Regex::new(r#"<?(https?://[^\s<>()|"]+)>?"#).unwrap());
    let br = BR.get_or_init(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
    let mut urls = Vec::new();
    let with_breaks = br.replace_all(raw, ", ");
    let without_links = link.replace_all(&with_breaks, |caps: &regex::Captures| {
        urls.push(caps[2].to_string());
        caps[1].to_string()
    });
    let text = bare.replace_all(&without_links, |caps: &regex::Captures| {
        urls.push(caps[1].trim_end_matches(['.', ',']).to_string());
        String::new()
    });
    let text = text
        .replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .replace("\\|", "|");
    let text = text
        .trim()
        .trim_matches(['*', '_'])
        .trim()
        .trim_end_matches(',')
        .trim();
    Cell {
        text: normalize::clip(text, 2_000),
        urls: urls
            .into_iter()
            .filter_map(|u| normalize::web_url(&u))
            .collect(),
    }
}

/// Splits a table row into raw cells (honouring `\|`).
fn split_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                current.push_str("\\|");
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    cells.push(current);
    cells
}

fn is_separator(line: &str) -> bool {
    let cells = split_row(line);
    line.contains('-')
        && cells.iter().all(|c| {
            let c = c.trim();
            !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
        })
}

fn opt(cell: Option<&Cell>) -> Option<String> {
    cell.and_then(|c| normalize::stated(&c.text, 300))
}

/// Requirement phrases from a skills cell: "Python, K8s; Terraform".
fn requirement_items(text: &str) -> Vec<String> {
    text.split([',', ';', '•', '·', '|', '\n'])
        .map(|item| normalize::clip(item.trim().trim_start_matches(['-', '*']).trim(), 120))
        .filter(|item| !item.is_empty() && !normalize::is_missing(item))
        .take(40)
        .collect()
}

fn row_job(columns: &[Col], cells: &[Cell]) -> Option<RawJob> {
    let get = |col: Col| -> Vec<&Cell> {
        columns
            .iter()
            .zip(cells)
            .filter(|(c, _)| **c == col)
            .map(|(_, cell)| cell)
            .collect()
    };
    let first = |col: Col| get(col).into_iter().next();
    let title_cell = first(Col::Title)?;
    let mut title = normalize::stated(&title_cell.text, 200)?;
    let mut company = opt(first(Col::Company));
    // "AI Engineer @ Company A" when there is no company column.
    if company.is_none() {
        for sep in [" @ ", " at "] {
            if let Some((t, c)) = title.rsplit_once(sep) {
                if c.split_whitespace().count() <= 5 && !t.trim().is_empty() {
                    company = Some(c.trim().to_string());
                    title = t.trim().to_string();
                    break;
                }
            }
        }
    }
    let location_parts: Vec<String> = get(Col::Location)
        .into_iter()
        .filter_map(|c| normalize::stated(&c.text, 200))
        .collect();
    let location = (!location_parts.is_empty()).then(|| location_parts.join(", "));
    let url = first(Col::Link)
        .and_then(|c| c.urls.first().cloned())
        .or_else(|| title_cell.urls.first().cloned())
        .or_else(|| {
            columns
                .iter()
                .zip(cells)
                .filter(|(c, _)| !matches!(c, Col::Company))
                .find_map(|(_, cell)| cell.urls.first().cloned())
        });
    // A link column without an address may still name the site ("LinkedIn").
    let source = opt(first(Col::Source)).or_else(|| {
        first(Col::Link)
            .filter(|c| c.urls.is_empty())
            .and_then(|c| normalize::stated(&c.text, 60))
    });
    let mut employment_type = opt(first(Col::EmploymentType));
    let mut work_mode = opt(first(Col::WorkMode));
    // A generic "Type" column sometimes holds the work mode.
    if work_mode.is_none() {
        if let Some(t) = employment_type.as_deref() {
            if normalize::work_mode(t).is_some() && normalize::employment_type(t).is_none() {
                work_mode = employment_type.take();
            }
        }
    }
    Some(RawJob {
        title,
        company,
        location,
        work_mode,
        employment_type,
        seniority: opt(first(Col::Seniority)),
        salary: opt(first(Col::Salary)),
        posted: opt(first(Col::Posted)),
        url,
        source,
        requirements: get(Col::Requirements)
            .into_iter()
            .flat_map(|c| requirement_items(&c.text))
            .collect(),
        description: first(Col::Description).and_then(|c| normalize::stated(&c.text, 2_000)),
        reference: opt(first(Col::Reference)),
    })
}

/// Every job listed in the Markdown tables of `markdown`.
pub fn jobs(markdown: &str) -> Vec<RawJob> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut out = Vec::new();
    let mut in_code = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            i += 1;
            continue;
        }
        if in_code || !line.contains('|') || i + 1 >= lines.len() || !is_separator(lines[i + 1]) {
            i += 1;
            continue;
        }
        let columns: Vec<Col> = split_row(line)
            .iter()
            .map(|h| column(&cell(h).text))
            .collect();
        let job_table = columns.contains(&Col::Title)
            && (columns.contains(&Col::Company) || columns.contains(&Col::Link));
        i += 2;
        while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
            if job_table {
                let cells: Vec<Cell> = split_row(lines[i]).iter().map(|c| cell(c)).collect();
                if let Some(job) = row_job(&columns, &cells) {
                    out.push(job);
                }
            }
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = "Here are matching roles:\n\n\
| # | Company | Role | Location | Work mode | Salary | Posted | Key skills | Link |\n\
|---|---|---|---|:---:|---|---|---|---|\n\
| 1 | **Company A GmbH** | [AI Engineer (m/w/d)](https://boards.greenhouse.io/companya/jobs/5512345) | Vienna, Austria | Remote | €90k–110k | 3 days ago | Python, K8s; RAG | [Apply](https://boards.greenhouse.io/companya/jobs/5512345?gh_src=x) |\n\
| 2 | Company B | ML Engineer | Berlin | Hybrid | Not stated | — | PyTorch | https://jobs.lever.co/b/8d1c2f3a-1111-2222-3333-444455556666 |\n\
\n\
Good luck!";

    #[test]
    fn reads_job_tables() {
        let jobs = jobs(ANSWER);
        assert_eq!(jobs.len(), 2);
        let a = &jobs[0];
        assert_eq!(a.title, "AI Engineer (m/w/d)");
        assert_eq!(a.company.as_deref(), Some("Company A GmbH"));
        assert_eq!(a.location.as_deref(), Some("Vienna, Austria"));
        assert_eq!(a.work_mode.as_deref(), Some("Remote"));
        assert_eq!(a.salary.as_deref(), Some("€90k–110k"));
        assert_eq!(a.posted.as_deref(), Some("3 days ago"));
        assert_eq!(a.requirements, ["Python", "K8s", "RAG"]);
        assert_eq!(
            a.url.as_deref(),
            Some("https://boards.greenhouse.io/companya/jobs/5512345?gh_src=x")
        );
        let b = &jobs[1];
        assert_eq!(b.salary, None);
        assert_eq!(b.posted, None);
        assert!(b
            .url
            .as_deref()
            .unwrap()
            .starts_with("https://jobs.lever.co/"));
    }

    #[test]
    fn ignores_other_tables_estimates_and_code() {
        let text = "| Skill | Level |\n|---|---|\n| Python | High |\n\n\
```\n| Company | Role |\n|---|---|\n| X | Y |\n```\n\n\
| Employer | Position | Salary (est.) | URL |\n|---|---|---|---|\n| Globex | Data Engineer @ Remote | ~€100k | https://globex.com/jobs/data-engineer-4411 |";
        let jobs = jobs(text);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].company.as_deref(), Some("Globex"));
        assert_eq!(jobs[0].salary, None, "estimated salaries are ignored");
    }

    #[test]
    fn recognizes_columns_by_whole_words() {
        assert_eq!(column("Requirements"), Col::Requirements);
        assert_eq!(column("Key skills"), Col::Requirements);
        assert_eq!(column("Match"), Col::Ignore);
        assert_eq!(column("Rating"), Col::Ignore);
        assert_eq!(column("Salary (EUR)"), Col::Salary);
        assert_eq!(column("Salary range (est.)"), Col::Ignore);
        assert_eq!(column("Position"), Col::Title);
        assert_eq!(column("Job title"), Col::Title);
        assert_eq!(column("Location / Mode"), Col::Location);
        assert_eq!(column("Remote?"), Col::WorkMode);
        assert_eq!(column("Experience level"), Col::Seniority);
        assert_eq!(column("Date posted"), Col::Posted);
        assert_eq!(column("Why it fits you"), Col::Ignore);
        assert_eq!(column("Job ID"), Col::Reference);
    }

    #[test]
    fn splits_role_and_company_when_combined() {
        let text = "| Role | Link |\n|---|---|\n| AI Engineer at Initech | https://initech.com/careers/ai-engineer-123456 |";
        let jobs = jobs(text);
        assert_eq!(jobs[0].title, "AI Engineer");
        assert_eq!(jobs[0].company.as_deref(), Some("Initech"));
    }
}
