//! The search planner (§25, §26): what one request searches for — the role
//! and close variants, the place, remote work, the salary floor,
//! freshness, named companies, the people sought and the scopes — and the
//! hints a provider's hosted search takes (sites to stay on, an approximate
//! place). The user's sentence is never forwarded as is to every source.
//!
//! The place comes only from the request itself (never the device's
//! location); without one, no place is assumed.

use std::sync::OnceLock;

use regex::Regex;

use super::{
    registry,
    requirement::{self, Requirement, Scopes},
};
use crate::{
    analytics::normalize,
    llm::ApproxLocation,
    retrieval::{self, intent::MinSalary, JobQuery},
};

/// A place named in a request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Place {
    pub city: Option<String>,
    /// Canonical English name ("Austria").
    pub country: Option<String>,
    /// ISO 3166-1 alpha-2 ("AT").
    pub code: Option<String>,
}

impl Place {
    /// The place a phrase names ("Vienna Austria", "Wien", "Germany").
    pub fn from_text(text: &str) -> Option<Self> {
        let found = normalize::place(text);
        let city = found.city().map(str::to_string);
        let country = found.country().map(str::to_string);
        if city.is_none() && country.is_none() {
            return None;
        }
        let code = country
            .as_deref()
            .and_then(country_code)
            .map(str::to_string);
        Some(Self {
            city,
            country,
            code,
        })
    }

    /// "Vienna, Austria".
    pub fn label(&self) -> String {
        [self.city.clone(), self.country.clone()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// For a provider's localized search: only the request's own place.
    pub fn approx(&self) -> ApproxLocation {
        ApproxLocation {
            city: self.city.clone(),
            region: None,
            country: self.code.clone(),
            timezone: timezone(self.code.as_deref(), self.city.as_deref()).map(str::to_string),
        }
    }
}

/// The ISO code of a canonical country name, including countries whose
/// code is not accepted as a word in job text ("IT").
pub fn country_code(country: &str) -> Option<&'static str> {
    if country == "United Kingdom" {
        // ISO 3166-1 is "GB" ("UK" is only reserved).
        return Some("GB");
    }
    normalize::country_code(country).or(match country {
        "Italy" => Some("IT"),
        "Czechia" => Some("CZ"),
        "Slovakia" => Some("SK"),
        "Hungary" => Some("HU"),
        "Romania" => Some("RO"),
        "Bulgaria" => Some("BG"),
        "Croatia" => Some("HR"),
        "Slovenia" => Some("SI"),
        "Serbia" => Some("RS"),
        "Greece" => Some("GR"),
        "Sweden" => Some("SE"),
        "Denmark" => Some("DK"),
        "Norway" => Some("NO"),
        "Finland" => Some("FI"),
        "Estonia" => Some("EE"),
        "Latvia" => Some("LV"),
        "Lithuania" => Some("LT"),
        "Ukraine" => Some("UA"),
        "Turkey" => Some("TR"),
        "Israel" => Some("IL"),
        "Malta" => Some("MT"),
        "Cyprus" => Some("CY"),
        "United States" => Some("US"),
        "Canada" => Some("CA"),
        "Mexico" => Some("MX"),
        "Brazil" => Some("BR"),
        "Argentina" => Some("AR"),
        "India" => Some("IN"),
        "Singapore" => Some("SG"),
        "Japan" => Some("JP"),
        "China" => Some("CN"),
        "South Korea" => Some("KR"),
        "Australia" => Some("AU"),
        "New Zealand" => Some("NZ"),
        "United Arab Emirates" => Some("AE"),
        "South Africa" => Some("ZA"),
        _ => None,
    })
}

/// The IANA time zone of a place when one is certain (a single-zone
/// country, or a known city).
pub fn timezone(code: Option<&str>, city: Option<&str>) -> Option<&'static str> {
    let by_city = match city {
        Some(
            "Seattle" | "Los Angeles" | "San Jose" | "Palo Alto" | "Mountain View" | "Redmond"
            | "San Francisco",
        ) => Some("America/Los_Angeles"),
        Some("Boston" | "Miami" | "Atlanta" | "Pittsburgh" | "New York") => {
            Some("America/New_York")
        }
        Some("Chicago" | "Austin") => Some("America/Chicago"),
        Some("Denver") => Some("America/Denver"),
        Some("Toronto" | "Ottawa" | "Montreal") => Some("America/Toronto"),
        Some("Vancouver") => Some("America/Vancouver"),
        Some("Sydney") => Some("Australia/Sydney"),
        Some("Melbourne") => Some("Australia/Melbourne"),
        _ => None,
    };
    by_city.or(match code? {
        "AT" => Some("Europe/Vienna"),
        "DE" => Some("Europe/Berlin"),
        "CH" => Some("Europe/Zurich"),
        "NL" => Some("Europe/Amsterdam"),
        "BE" => Some("Europe/Brussels"),
        "LU" => Some("Europe/Luxembourg"),
        "FR" => Some("Europe/Paris"),
        "ES" => Some("Europe/Madrid"),
        "PT" => Some("Europe/Lisbon"),
        "IT" => Some("Europe/Rome"),
        "IE" => Some("Europe/Dublin"),
        "GB" | "UK" => Some("Europe/London"),
        "PL" => Some("Europe/Warsaw"),
        "CZ" => Some("Europe/Prague"),
        "SK" => Some("Europe/Bratislava"),
        "HU" => Some("Europe/Budapest"),
        "RO" => Some("Europe/Bucharest"),
        "BG" => Some("Europe/Sofia"),
        "HR" => Some("Europe/Zagreb"),
        "SI" => Some("Europe/Ljubljana"),
        "RS" => Some("Europe/Belgrade"),
        "GR" => Some("Europe/Athens"),
        "SE" => Some("Europe/Stockholm"),
        "DK" => Some("Europe/Copenhagen"),
        "NO" => Some("Europe/Oslo"),
        "FI" => Some("Europe/Helsinki"),
        "EE" => Some("Europe/Tallinn"),
        "LV" => Some("Europe/Riga"),
        "LT" => Some("Europe/Vilnius"),
        "UA" => Some("Europe/Kyiv"),
        "TR" => Some("Europe/Istanbul"),
        "IL" => Some("Asia/Jerusalem"),
        "MT" => Some("Europe/Malta"),
        "CY" => Some("Asia/Nicosia"),
        "IN" => Some("Asia/Kolkata"),
        "SG" => Some("Asia/Singapore"),
        "JP" => Some("Asia/Tokyo"),
        "KR" => Some("Asia/Seoul"),
        "CN" => Some("Asia/Shanghai"),
        "NZ" => Some("Pacific/Auckland"),
        "AE" => Some("Asia/Dubai"),
        "ZA" => Some("Africa/Johannesburg"),
        _ => None,
    })
}

/// What one request searches for.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchPlan {
    pub text: String,
    pub requirement: Requirement,
    pub scopes: Scopes,
    /// The role as asked, then close variants (at most five in all).
    pub roles: Vec<String>,
    pub place: Option<Place>,
    pub remote: bool,
    pub min_salary: Option<MinSalary>,
    pub posted_within_days: Option<u32>,
    /// "most recent", "newest", "latest": newest first.
    pub newest_first: bool,
    /// Companies the request names.
    pub companies: Vec<String>,
    /// The people sought ("recruiter", "Head of AI").
    pub people: Vec<String>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// Close variants of a role, so "AI jobs" also finds "Machine Learning
/// Engineer" postings (§26: related roles).
pub fn related_roles(role: &str) -> Vec<String> {
    let lower = role.to_lowercase();
    let lower = lower.trim();
    let family: &[&str] = if lower == "ai"
        || lower.contains("ai engineer")
        || lower.contains("artificial intelligence")
        || lower.contains("genai")
        || lower.contains("llm")
    {
        &[
            "AI Engineer",
            "Machine Learning Engineer",
            "ML Engineer",
            "Applied AI Engineer",
            "LLM Engineer",
        ]
    } else if lower.contains("machine learning") || lower == "ml" || lower.contains("ml engineer") {
        &[
            "Machine Learning Engineer",
            "ML Engineer",
            "AI Engineer",
            "MLOps Engineer",
        ]
    } else if lower.contains("data scien") {
        &[
            "Data Scientist",
            "Machine Learning Scientist",
            "Applied Scientist",
        ]
    } else if lower.contains("data engineer") {
        &["Data Engineer", "Analytics Engineer", "Big Data Engineer"]
    } else if lower.contains("devops") || lower.contains("site reliability") || lower == "sre" {
        &[
            "DevOps Engineer",
            "Platform Engineer",
            "Site Reliability Engineer",
        ]
    } else if lower.contains("frontend") || lower.contains("front-end") {
        &["Frontend Engineer", "Frontend Developer", "React Developer"]
    } else if lower.contains("backend") || lower.contains("back-end") {
        &["Backend Engineer", "Backend Developer", "Software Engineer"]
    } else if lower.contains("software") || lower == "developer" || lower == "engineer" {
        &[
            "Software Engineer",
            "Software Developer",
            "Backend Engineer",
            "Full Stack Engineer",
        ]
    } else if lower.contains("product manager") || lower.contains("product owner") {
        &["Product Manager", "Product Owner"]
    } else {
        &[]
    };
    family
        .iter()
        .filter(|r| !r.eq_ignore_ascii_case(role.trim()))
        .map(|r| r.to_string())
        .collect()
}

fn newest_first(text: &str) -> bool {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(most\s+recent|newest|latest|recent(?:ly)?|new(?:ly)?\s+posted|just\s+posted|fresh)\b",
    )
    .is_match(text)
}

/// Companies named after "at", "for", "of" or "with" ("recruiters at
/// Bitpanda", "Research Company X").
fn companies(text: &str, query: Option<&JobQuery>) -> Vec<String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let pattern = re(
        &CELL,
        r"\b(?:at|for|of|with|about|research|researching)\s+(?P<name>(?:[A-Z][\w&.\-]*|[A-Z]{2,})(?:\s+(?:[A-Z][\w&.\-]*|[A-Z]{2,}|X)){0,3})",
    );
    let mut out: Vec<String> = query.and_then(|q| q.company.clone()).into_iter().collect();
    for caps in pattern.captures_iter(text) {
        let name = caps["name"].trim().trim_end_matches(['.', ',']).to_string();
        let lower = name.to_lowercase();
        let place = normalize::place(&name);
        let generic = [
            "ai",
            "ml",
            "the",
            "company",
            "companies",
            "head",
            "vp",
            "cto",
            "ceo",
        ]
        .contains(&lower.as_str());
        if place.city().is_some() || place.country().is_some() || generic || name.len() < 2 {
            continue;
        }
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&name)) {
            out.push(name);
        }
    }
    out.truncate(3);
    out
}

/// People a request looks for.
fn people(text: &str) -> Vec<String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let pattern = re(
        &CELL,
        r"(?i)\b(recruiters?|talent acquisition(?: partners?| managers?)?|hiring managers?|(?:head|vp|director) of [a-z&]+(?: [a-z&]+)?|chief [a-z]+ officer|ceo|cto|cfo|cpo|founders?|team leads?|engineering managers?)\b",
    );
    let mut out: Vec<String> = Vec::new();
    for found in pattern.find_iter(text) {
        let mut role = found.as_str().to_string();
        // "Head of AI at …": the preposition is not part of the role.
        for stop in [" at", " in", " for", " with", " of", " from", " and"] {
            if role.to_lowercase().ends_with(stop) {
                role.truncate(role.len() - stop.len());
            }
        }
        if !out.iter().any(|o: &String| o.eq_ignore_ascii_case(&role)) {
            out.push(role);
        }
    }
    out
}

/// Plans a request.
pub fn plan(text: &str) -> SearchPlan {
    let classified = requirement::classify(text);
    let query = retrieval::detect(text);
    let mut plan = from_parts(
        text,
        classified.requirement,
        classified.scopes,
        query.as_ref(),
    );
    // A job search is always current.
    if query.is_some() {
        plan.scopes.jobs = true;
        plan.requirement = Requirement::Required;
    }
    plan
}

/// The plan of a job search ReMa already recognized.
pub fn for_job_query(query: &JobQuery) -> SearchPlan {
    let classified = requirement::classify(&query.text);
    let mut scopes = classified.scopes;
    scopes.jobs = true;
    from_parts(&query.text, Requirement::Required, scopes, Some(query))
}

fn from_parts(
    text: &str,
    requirement: Requirement,
    scopes: Scopes,
    query: Option<&JobQuery>,
) -> SearchPlan {
    let role = query.and_then(|q| q.role.clone());
    let mut roles: Vec<String> = role.iter().cloned().collect();
    if let Some(role) = &role {
        roles.extend(related_roles(role));
    }
    // The role and every variant of its family (at most five).
    roles.truncate(6);
    let place = query
        .and_then(|q| q.location.as_deref())
        .and_then(Place::from_text)
        .or_else(|| {
            // "in Vienna Austria", "Austria" anywhere in a research request.
            static CELL: OnceLock<Regex> = OnceLock::new();
            let pattern = re(
                &CELL,
                r"(?i)\b(?:in|near|around|based in)\s+(?P<place>[\p{L} ,.-]{2,40})",
            );
            pattern
                .captures_iter(text)
                .find_map(|caps| Place::from_text(&caps["place"]))
        });
    let companies = companies(text, query);
    let mut scopes = scopes;
    // "Recruiters at Bitpanda" is about that company too.
    if !companies.is_empty() && (scopes.people || scopes.market) {
        scopes.company = true;
    }
    SearchPlan {
        text: text.trim().to_string(),
        requirement,
        scopes,
        roles,
        place,
        remote: query.is_some_and(|q| q.remote),
        min_salary: query.and_then(|q| q.min_salary.clone()),
        posted_within_days: query.and_then(|q| q.posted_within_days),
        newest_first: newest_first(text),
        companies,
        people: people(text),
    }
}

/// What a provider's hosted search is given for a plan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hints {
    /// Sites to stay on (empty: no restriction).
    pub allowed_domains: Vec<String>,
    pub location: Option<ApproxLocation>,
}

/// Hints for a plan: the registry's sites for its scopes and country, the
/// named companies' own domains first (`company_domains`).
pub fn hints(plan: &SearchPlan, company_domains: &[String]) -> Hints {
    let code = plan.place.as_ref().and_then(|p| p.code.as_deref());
    Hints {
        allowed_domains: registry::allowed_domains(plan.scopes, code, company_domains),
        location: plan.place.as_ref().map(Place::approx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_the_screenshot_request() {
        let plan = plan(
            "Find me most recent AI jobs in Vienna Austria with salary starting from 85k a year.",
        );
        assert_eq!(plan.requirement, Requirement::Required);
        assert!(plan.scopes.jobs);
        assert!(plan.newest_first);
        let place = plan.place.clone().unwrap();
        assert_eq!(place.city.as_deref(), Some("Vienna"));
        assert_eq!(place.code.as_deref(), Some("AT"));
        let floor = plan.min_salary.clone().unwrap();
        assert_eq!(floor.amount, 85_000.0);
        assert!(
            plan.roles.iter().any(|r| r == "Machine Learning Engineer"),
            "{:?}",
            plan.roles
        );
        let hints = hints(&plan, &[]);
        assert!(hints.allowed_domains.contains(&"karriere.at".to_string()));
        assert!(hints.allowed_domains.contains(&"greenhouse.io".to_string()));
        assert_eq!(
            hints.location,
            Some(ApproxLocation {
                city: Some("Vienna".into()),
                region: None,
                country: Some("AT".into()),
                timezone: Some("Europe/Vienna".into()),
            })
        );
    }

    #[test]
    fn plans_people_and_company_research() {
        let plan = plan("Find current recruiters at Bitpanda.");
        assert_eq!(plan.requirement, Requirement::Required);
        assert!(plan.scopes.people && plan.scopes.company);
        assert_eq!(plan.companies, ["Bitpanda"]);
        assert_eq!(plan.people, ["recruiters"]);
        let head = super::plan("Find current Head of AI at Company X.");
        assert_eq!(head.companies, ["Company X"]);
        assert_eq!(head.people, ["Head of AI"]);
        // No place is assumed when the request names none.
        assert_eq!(head.place, None);
        assert_eq!(hints(&head, &[]).location, None);
    }

    #[test]
    fn places_without_a_city_and_multi_zone_countries() {
        let place = Place::from_text("Germany").unwrap();
        assert_eq!(place.approx().timezone.as_deref(), Some("Europe/Berlin"));
        let us = Place::from_text("United States").unwrap();
        assert_eq!(us.code.as_deref(), Some("US"));
        assert_eq!(us.approx().timezone, None, "several zones: none is guessed");
        assert_eq!(Place::from_text("the moon"), None);
    }
}
