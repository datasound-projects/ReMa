//! Recognizes messages that ask for current job listings, and reads the
//! constraints they state.
//!
//! This is deliberately deterministic: whether ReMa must search the web
//! before answering is decided by application code, never left to a model.
//! A message counts as a job search when it names jobs (openings,
//! positions, vacancies, …) together with a search cue (find, show, any,
//! current, posted, hiring, …), and is not about writing a cover letter,
//! CV or interview preparation. A message that asks whether linked
//! postings are still open counts as a verification request.

use std::sync::OnceLock;

use regex::Regex;

use crate::{analytics::normalize, models::analytics::SalaryPeriod};

/// What a job-search message asks for.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JobQuery {
    /// The user's message, as written.
    pub text: String,
    /// "AI Engineer", "senior data scientist".
    pub role: Option<String>,
    /// "Vienna", "Austria".
    pub location: Option<String>,
    /// "at Google".
    pub company: Option<String>,
    pub remote: bool,
    /// "posted within the last 10 days" → 10.
    pub posted_within_days: Option<u32>,
    /// "salary above 90K a year".
    pub min_salary: Option<MinSalary>,
    /// Postings the user asked ReMa to check.
    pub verify_urls: Vec<String>,
}

/// A salary floor the user stated.
#[derive(Debug, Clone, PartialEq)]
pub struct MinSalary {
    pub amount: f64,
    /// ISO code when stated ("EUR").
    pub currency: Option<&'static str>,
    pub period: SalaryPeriod,
}

impl JobQuery {
    /// Words of the role that a relevant posting title should share.
    pub fn role_terms(&self) -> Vec<String> {
        const GENERIC: &[&str] = &[
            "senior",
            "junior",
            "lead",
            "principal",
            "staff",
            "head",
            "mid",
            "level",
            "entry",
            "the",
            "and",
            "for",
            "with",
            "of",
            "jobs",
            "job",
            "role",
            "roles",
        ];
        self.role
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
            .filter(|w| w.len() >= 2 && !GENERIC.contains(w))
            .map(stem)
            .collect()
    }

    /// A short description for status lines: "AI Engineer in Vienna".
    pub fn subject(&self) -> String {
        let mut subject = self
            .role
            .clone()
            .unwrap_or_else(|| "matching jobs".to_string());
        if let Some(company) = &self.company {
            subject.push_str(&format!(" at {company}"));
        }
        match (&self.location, self.remote) {
            (Some(location), true) => subject.push_str(&format!(" in {location} or remote")),
            (Some(location), false) => subject.push_str(&format!(" in {location}")),
            (None, true) => subject.push_str(" (remote)"),
            (None, false) => {}
        }
        subject
    }
}

/// Reduces "engineering", "engineers" and "engineer" to one form, so a
/// request for "AI engineering jobs" matches an "AI Engineer" posting.
pub fn stem(word: &str) -> String {
    let mut w = word.to_lowercase();
    // Twice: "engineering" → "engineer" → "engine".
    for _ in 0..2 {
        if let Some(base) = ["ing", "ers", "er", "s"]
            .iter()
            .find_map(|suffix| w.strip_suffix(suffix).filter(|b| b.len() >= 4))
        {
            w = base.to_string();
        }
    }
    w
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

const NOUNS: &str = r"jobs?|job\s+(?:openings?|offers?|ads?|postings?|listings?)|openings?|positions?|roles?|vacanc(?:y|ies)|postings?|listings?|opportunit(?:y|ies)|contracts|contract\s+(?:roles?|work|positions?)|freelance\s+(?:projects|gigs|work)|gigs|stellen(?:angebote?|anzeigen?)?|jobangebote?";

fn job_noun() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(&CELL, &format!(r"(?i)\b(?:{NOUNS}|hiring)\b"))
}

/// Nouns that can only mean listings (not "the role of …").
fn listing_noun() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(?:jobs|job\s+(?:openings?|offers|ads|postings?|listings?)|openings|positions|vacanc(?:y|ies)|postings|listings|contracts|gigs|stellen(?:angebote?)?|jobangebote?)\b",
    )
}

/// Whether a message names listings ("jobs", "openings", …): a search then,
/// whatever else it mentions ("job emails" names none).
pub fn names_listings(message: &str) -> bool {
    listing_noun().is_match(message)
}

fn search_cue() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(?:find|search(?:ing)?|look(?:ing)?\s+for|list|show(?:\s+me)?|get\s+me|give\s+me|any|current(?:ly)?|latest|newest|new|recent(?:ly)?|open|available|posted|hiring|are\s+there|is\s+there|recommend|suggest|which\s+companies|where\s+can\s+i\s+apply|suche|finde)\b",
    )
}

fn off_topic() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(?:cover\s+letter|motivation\s+letter|resume\s+template|cv\s+template|interview|prepare|tailor|rewrite|draft|negotiat\w*|typical\s+salar\w*|average\s+salar\w*|what\s+skills|which\s+skills|job\s+description\s+for)\b",
    )
}

fn url() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(&CELL, r#"https?://[^\s<>"'()\[\]]+"#)
}

fn verify_cue() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(?:verify|check|still\s+(?:open|available|active|online|listed|valid|accepting)|(?:is|are)\s+(?:this|it|that|these|they|those)\s+(?:(?:job|posting|position|role)s?\s+)?(?:still\s+)?(?:open|available|live|current|valid|real)|expired|up\s+to\s+date)\b",
    )
}

/// The job search a message asks for, if it asks for one.
pub fn detect(message: &str) -> Option<JobQuery> {
    let text = message.trim();
    if text.is_empty() || text.len() > 4_000 {
        return None;
    }
    let urls: Vec<String> = url()
        .find_iter(text)
        .map(|m| {
            m.as_str()
                .trim_end_matches(['.', ',', ';', ':', '!', '?'])
                .to_string()
        })
        .filter(|u| normalize::web_url(u).is_some())
        .take(10)
        .collect();
    let verify = !urls.is_empty() && verify_cue().is_match(text);
    // Writing help about "a role" is not a search; a request naming listings
    // ("jobs", "openings") is, whatever else it mentions.
    let search = job_noun().is_match(text)
        && search_cue().is_match(text)
        && (listing_noun().is_match(text) || !off_topic().is_match(text));
    if !search && !verify {
        return None;
    }
    // Everything after a URL is often part of it; read constraints from the
    // words only.
    let words = url().replace_all(text, " ");
    let (role, remote_in_role) = role(&words);
    Some(JobQuery {
        text: text.to_string(),
        role,
        location: location(&words),
        company: company(&words),
        remote: remote_in_role || remote(&words),
        posted_within_days: posted_within_days(&words),
        min_salary: min_salary(&words),
        verify_urls: if verify { urls } else { Vec::new() },
    })
}

/// Filler words around a role ("current", "some", …); "remote" is noted.
fn clean_role(raw: &str) -> (Option<String>, bool) {
    const FILLER: &[&str] = &[
        "me",
        "us",
        "most",
        "some",
        "the",
        "a",
        "an",
        "all",
        "any",
        "current",
        "currently",
        "new",
        "newest",
        "latest",
        "recent",
        // "newly posted AI jobs": when, not what.
        "newly",
        "recently",
        "freshly",
        "just",
        "posted",
        "published",
        "listed",
        "added",
        "fresh",
        "open",
        "available",
        "good",
        "great",
        "top",
        "best",
        "full-time",
        "fulltime",
        "part-time",
        "permanent",
        "more",
        "other",
        "few",
        "several",
        "there",
        "are",
        "is",
        "please",
    ];
    let mut remote = false;
    let words: Vec<&str> = raw
        .split_whitespace()
        .filter(|w| {
            let lower = w.to_lowercase();
            let lower = lower.trim_matches(|c: char| !c.is_alphanumeric());
            if lower == "remote" {
                remote = true;
                return false;
            }
            // "10 AI Engineer positions": how many, not what.
            const COUNTS: &[&str] = &[
                "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
                "twelve", "fifteen", "twenty", "dozen", "couple",
            ];
            let count = (!lower.is_empty()
                && lower.len() <= 3
                && lower.chars().all(|c| c.is_ascii_digit()))
                || COUNTS.contains(&lower);
            !FILLER.contains(&lower) && !count
        })
        .collect();
    let role = words.join(" ");
    let role = role.trim_matches(|c: char| !c.is_alphanumeric() && c != '+' && c != '#');
    let useful = !role.is_empty() && role.len() <= 60 && role.split_whitespace().count() <= 6;
    (useful.then(|| role.to_string()), remote)
}

fn role(text: &str) -> (Option<String>, bool) {
    static VERB: OnceLock<Regex> = OnceLock::new();
    static FOR_AS: OnceLock<Regex> = OnceLock::new();
    static BEFORE: OnceLock<Regex> = OnceLock::new();
    let verb = re(
        &VERB,
        &format!(
            r"(?i)\b(?:find|search(?:\s+for)?|look(?:ing)?\s+for|list|show(?:\s+me)?|get\s+me|give\s+me|any|are\s+there(?:\s+any)?|is\s+there(?:\s+an?)?|suche|finde)\s+(?P<role>[^,.;!?]{{2,80}}?)\s+(?:{NOUNS})\b"
        ),
    );
    let for_as = re(
        &FOR_AS,
        &format!(
            r"(?i)\b(?:{NOUNS})\s+(?:for|as|als)\s+(?:an?\s+)?(?P<role>[^,.;!?]{{2,60}}?)(?:\s+(?:in|near|at|with|that|which|posted|paying|based|around|within|from)\b|[,.;!?]|$)"
        ),
    );
    let before = re(
        &BEFORE,
        r"(?i)(?P<role>(?:[\p{L}\d/+#.-]+\s+){0,3}[\p{L}\d/+#.-]+)\s+(?:jobs|positions|openings|vacancies|roles|job|position|role)\b",
    );
    for pattern in [verb, for_as, before] {
        if let Some(caps) = pattern.captures(text) {
            let (role, remote) = clean_role(&caps["role"]);
            if role.is_some() {
                return (role, remote);
            }
            if remote {
                return (None, true);
            }
        }
    }
    (None, false)
}

/// Words after "in" that name a field, not a place.
const NOT_PLACES: &[&str] = &[
    "ai",
    "ml",
    "it",
    "tech",
    "technology",
    "data",
    "finance",
    "fintech",
    "healthcare",
    "health",
    "engineering",
    "software",
    "startups",
    "startup",
    "research",
    "marketing",
    "sales",
    "the",
    "a",
    "an",
    "my",
    "our",
    "your",
    "this",
    "that",
    "these",
    "total",
    "person",
    "office",
    "house",
    "english",
    "german",
    "python",
    "rust",
    "java",
    "go",
    "machine",
    "cloud",
    "security",
    "product",
    "design",
    "consulting",
    "banking",
    "gaming",
    "education",
    "public",
    "government",
];

fn location(text: &str) -> Option<String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let preposition = re(
        &CELL,
        r"(?i)\b(?:in|near|around|based\s+in|located\s+in)\s+",
    );
    const STOP: &[&str] = &[
        "with",
        "that",
        "which",
        "posted",
        "paying",
        "for",
        "and",
        "or",
        "above",
        "over",
        "at",
        "from",
        "within",
        "in",
        "the",
        "last",
        "past",
        "who",
        "where",
        "salary",
        "remote",
        "hybrid",
        "on-site",
        "onsite",
        "only",
        "please",
        "jobs",
        "roles",
        "positions",
    ];
    // Every "in …" is tried, so "jobs in AI in Linz" finds Linz.
    for found in preposition.find_iter(text) {
        let words: Vec<&str> = text[found.end()..]
            .split(|c: char| {
                c.is_whitespace() || matches!(c, ',' | '.' | ';' | '!' | '?' | '(' | ')')
            })
            .filter(|w| !w.is_empty())
            .take(3)
            .take_while(|w| {
                !STOP.contains(&w.to_lowercase().as_str())
                    && w.chars().next().is_some_and(char::is_alphabetic)
            })
            .collect();
        if words.is_empty() {
            continue;
        }
        let first = words[0].to_lowercase();
        if NOT_PLACES.contains(&first.as_str()) {
            continue;
        }
        let phrase = words.join(" ");
        let place = normalize::place(&phrase);
        if let Some(city) = place.city() {
            return Some(city.to_string());
        }
        if let Some(country) = place.country() {
            return Some(country.to_string());
        }
        // An unknown town: only when written as a name ("in Linz").
        let name: Vec<&str> = words
            .iter()
            .copied()
            .take_while(|w| w.chars().next().is_some_and(char::is_uppercase))
            .collect();
        let acronym = name.len() == 1 && name[0].chars().all(|c| c.is_ascii_uppercase());
        if !name.is_empty() && !acronym {
            return Some(name.join(" "));
        }
    }
    None
}

fn company(text: &str) -> Option<String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let pattern = re(
        &CELL,
        r"\bat\s+(?P<company>[A-Z][\w&.\-]*(?:\s+[A-Z][\w&.\-]*){0,3})",
    );
    let caps = pattern.captures(text)?;
    let name = caps["company"].trim().to_string();
    // "at Vienna" is a place.
    (normalize::place(&name).city().is_none() && normalize::country_name(&name).is_none())
        .then_some(name)
}

fn remote(text: &str) -> bool {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(?:remote(?:ly)?|work\s+from\s+home|wfh|home\s*office|fully\s+distributed)\b",
    )
    .is_match(text)
}

fn posted_within_days(text: &str) -> Option<u32> {
    static COUNT: OnceLock<Regex> = OnceLock::new();
    static UNIT: OnceLock<Regex> = OnceLock::new();
    static WORDS: OnceLock<Regex> = OnceLock::new();
    let days_of = |n: u32, unit: &str| -> u32 {
        let unit = unit.to_lowercase();
        if unit.starts_with("hour") {
            n.div_ceil(24).max(1)
        } else if unit.starts_with("week") {
            n * 7
        } else if unit.starts_with("month") {
            n * 30
        } else {
            n
        }
    };
    let count = re(
        &COUNT,
        r"(?i)\b(?:last|past|previous|recent)\s+(?P<n>\d{1,3})\s*(?P<unit>hours?|days?|weeks?|months?)\b|\b(?:less|fewer|not\s+more)\s+than\s+(?P<n2>\d{1,3})\s*(?P<unit2>days?|weeks?|months?)\s+(?:old|ago)\b|\b(?:up\s+to|at\s+most|max(?:imum)?)\s+(?P<n3>\d{1,3})\s*(?P<unit3>days?|weeks?)\s+(?:old|ago)\b",
    );
    if let Some(caps) = count.captures(text) {
        let (n, unit) = [("n", "unit"), ("n2", "unit2"), ("n3", "unit3")]
            .iter()
            .find_map(|(n, u)| Some((caps.name(n)?.as_str(), caps.name(u)?.as_str())))?;
        let n: u32 = n.parse().ok()?;
        return (n > 0).then(|| days_of(n, unit).min(365));
    }
    let unit = re(
        &UNIT,
        r"(?i)\b(?:last|past)\s+(?P<unit>24\s*hours|day|week|fortnight|month)\b",
    );
    if let Some(caps) = unit.captures(text) {
        let unit = caps["unit"].to_lowercase();
        return Some(match unit.as_str() {
            "day" => 1,
            "week" => 7,
            "fortnight" => 14,
            "month" => 30,
            _ => 1,
        });
    }
    let words = re(
        &WORDS,
        r"(?i)\b(?P<w>today|yesterday|this\s+week|this\s+month)\b",
    );
    let caps = words.captures(text)?;
    Some(match caps["w"].to_lowercase().split_whitespace().last()? {
        "today" => 1,
        "yesterday" => 2,
        "week" => 7,
        _ => 30,
    })
}

fn currency(text: &str) -> Option<&'static str> {
    match text.trim().to_lowercase().as_str() {
        "€" | "eur" | "euro" | "euros" => Some("EUR"),
        "$" | "usd" | "dollar" | "dollars" => Some("USD"),
        "£" | "gbp" | "pound" | "pounds" => Some("GBP"),
        "chf" => Some("CHF"),
        _ => None,
    }
}

fn min_salary(text: &str) -> Option<MinSalary> {
    static FLOOR: OnceLock<Regex> = OnceLock::new();
    static PLUS: OnceLock<Regex> = OnceLock::new();
    const NUMBER: &str = r"(?P<num>\d{1,3}(?:[.,\u{a0} ]\d{3})+|\d+(?:[.,]\d+)?)";
    const CUR: &str = r"€|eur|euros?|\$|usd|dollars?|£|gbp|pounds?|chf";
    let floor = re(
        &FLOOR,
        &format!(
            r"(?i)(?P<kw>salary|pay(?:ing|s)?|paid|compensation|earn(?:ing)?|gross|base|income)?[^\d€$£\n]{{0,24}}?\b(?:above|over|more\s+than|at\s+least|minimum(?:\s+of)?|min\.?|no\s+less\s+than|upwards\s+of|starting\s+(?:at|from)|from)\s*(?P<cur1>{CUR})?\s*{NUMBER}\s*(?P<k>k\b|tsd\.?|thousand)?\s*(?P<cur2>{CUR})?\s*(?P<per>(?:a|per|/|p\.?)\s*(?:year|yr|annum|a\b|month|mo\b)|annually|yearly|monthly|p\.\s?a\.?)?"
        ),
    );
    let plus = re(
        &PLUS,
        &format!(
            r"(?i)(?P<cur1>{CUR})?\s*(?P<num>\d{{2,3}})\s*(?P<k>k)\s*\+\s*(?P<cur2>{CUR})?\s*(?P<per>(?:a|per|/)\s*(?:year|month))?"
        ),
    );
    for (pattern, needs_keyword) in [(floor, true), (plus, false)] {
        for caps in pattern.captures_iter(text) {
            let raw = caps["num"].to_string();
            let grouped = raw.len() > 4
                && raw.chars().any(|c| matches!(c, '.' | ',' | ' ' | '\u{a0}'))
                && raw
                    .rsplit(['.', ',', ' ', '\u{a0}'])
                    .next()
                    .is_some_and(|last| last.len() == 3);
            let number: f64 = if grouped {
                raw.chars()
                    .filter(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .ok()?
            } else {
                raw.replace(',', ".").parse().ok()?
            };
            let thousands = caps.name("k").is_some();
            let cur = caps
                .name("cur1")
                .or_else(|| caps.name("cur2"))
                .and_then(|c| currency(c.as_str()));
            let keyword = caps.name("kw").is_some();
            let amount = if thousands {
                number * 1_000.0
            } else if number < 1_000.0 && (keyword || cur.is_some()) {
                // "salary above 90" means 90,000.
                number * 1_000.0
            } else {
                number
            };
            // A floor needs money context: a keyword, a currency or "k".
            if needs_keyword && !(keyword || cur.is_some() || thousands) {
                continue;
            }
            if amount < 1_000.0 {
                continue;
            }
            let period = match caps.name("per").map(|p| p.as_str().to_lowercase()) {
                Some(p) if p.contains("mo") => SalaryPeriod::Month,
                _ => SalaryPeriod::Year,
            };
            return Some(MinSalary {
                amount,
                currency: cur,
                period,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> JobQuery {
        detect(text).unwrap_or_else(|| panic!("not detected: {text}"))
    }

    #[test]
    fn recognizes_job_searches() {
        for text in [
            "Find senior AI engineering jobs in Vienna with salaray above 90K a year",
            "Find current AI Engineer jobs in Vienna, posted within the last 10 days.",
            "Show me remote data scientist jobs",
            "Any new ML positions at Google?",
            "Are there Rust jobs in Berlin?",
            "list open roles for product managers in Graz",
            "Who is hiring ML engineers in Munich?",
            "Suche Stellenangebote als Data Engineer in Wien",
            "Find AI jobs that match my CV",
        ] {
            assert!(detect(text).is_some(), "{text}");
        }
    }

    #[test]
    fn leaves_other_requests_alone() {
        for text in [
            "Which skills do data engineering roles ask for most?",
            "Draft a short cover letter for a data engineer role",
            "Help me prepare for a technical interview",
            "What roles should I target given my CV?",
            "I got a job offer, how do I negotiate?",
            "What is the average salary for AI engineers in Vienna?",
            "Summarize my week",
            "",
        ] {
            assert!(detect(text).is_none(), "{text}");
        }
    }

    #[test]
    fn reads_the_constraints() {
        let query = q("Find senior AI engineering jobs in Vienna with salaray above 90K a year");
        assert_eq!(query.role.as_deref(), Some("senior AI engineering"));
        assert_eq!(query.location.as_deref(), Some("Vienna"));
        assert_eq!(
            query.min_salary,
            Some(MinSalary {
                amount: 90_000.0,
                currency: None,
                period: SalaryPeriod::Year
            })
        );
        assert_eq!(query.posted_within_days, None);

        let query = q("Find current AI Engineer jobs in Vienna, posted within the last 10 days.");
        assert_eq!(query.role.as_deref(), Some("AI Engineer"));
        assert_eq!(query.location.as_deref(), Some("Vienna"));
        assert_eq!(query.posted_within_days, Some(10));
        assert_eq!(query.subject(), "AI Engineer in Vienna");

        let query =
            q("Show me remote data scientist jobs posted this week paying at least €70,000");
        assert!(query.remote);
        assert_eq!(query.role.as_deref(), Some("data scientist"));
        assert_eq!(query.posted_within_days, Some(7));
        let salary = query.min_salary.unwrap();
        assert_eq!((salary.amount, salary.currency), (70_000.0, Some("EUR")));

        let query = q("Any new ML positions at Google in Zurich from the past 2 weeks?");
        assert_eq!(query.company.as_deref(), Some("Google"));
        assert_eq!(query.location.as_deref(), Some("Zurich"));
        assert_eq!(query.posted_within_days, Some(14));

        // The prompts of §70 and §73: the count is not part of the role.
        assert_eq!(
            q("Find 10 currently open AI Engineer jobs in Vienna.")
                .role
                .as_deref(),
            Some("AI Engineer")
        );
        assert_eq!(
            q("Find 10 AI Engineer positions currently open in Vienna, posted recently, and cite the direct source for every role.")
                .role
                .as_deref(),
            Some("AI Engineer")
        );
        assert_eq!(
            q("Find five data engineer jobs").role.as_deref(),
            Some("data engineer")
        );

        // The scheduled task of §76: "newly posted" is not part of the role.
        let query = q("Find newly posted AI jobs in Vienna.");
        assert_eq!(query.role.as_deref(), Some("AI"));
        assert_eq!(query.location.as_deref(), Some("Vienna"));
        assert_eq!(
            q("Show me recently published data engineer positions")
                .role
                .as_deref(),
            Some("data engineer")
        );

        let query = q("Suche Stellenangebote als Data Engineer in Wien");
        assert_eq!(query.location.as_deref(), Some("Vienna"));
        assert_eq!(query.role.as_deref(), Some("Data Engineer"));

        // "in AI" is a field, "the last 10 days" a period: neither is a place.
        let query = q("Find jobs in AI in Linz from the last 3 days");
        assert_eq!(query.location.as_deref(), Some("Linz"));
        assert_eq!(query.posted_within_days, Some(3));

        let query = q("Find Python developer jobs over 5,500 € per month");
        let salary = query.min_salary.unwrap();
        assert_eq!(
            (salary.amount, salary.currency, salary.period),
            (5_500.0, Some("EUR"), SalaryPeriod::Month)
        );

        // Years of experience are not a salary.
        assert_eq!(
            q("Find ML jobs requiring more than 5 years of experience").min_salary,
            None
        );
        assert_eq!(
            q("Find backend jobs 100k+ in Berlin")
                .min_salary
                .unwrap()
                .amount,
            100_000.0
        );
    }

    #[test]
    fn recognizes_requests_to_verify_postings() {
        let query = q("Is this job still open? https://jobs.example.com/ai-engineer-123.");
        assert_eq!(
            query.verify_urls,
            vec!["https://jobs.example.com/ai-engineer-123"]
        );
        assert!(detect("Summarize https://example.com/blog/post").is_none());
    }

    #[test]
    fn stems_role_words() {
        let query = q("Find senior AI engineering jobs in Vienna");
        assert_eq!(query.role_terms(), vec!["ai", "engine"]);
        assert_eq!(stem("Engineer"), "engine");
        assert_eq!(stem("engineers"), "engine");
        assert_eq!(stem("scientist"), "scientist");
    }
}
