//! Requirement extraction.
//!
//! 1. Deterministic: skills columns of search results and job descriptions
//!    are matched against the skill dictionary, plus fixed patterns for years
//!    of experience, degrees, spoken languages and industries.
//! 2. Optional model extraction: the model returns JSON; every requirement
//!    must quote the description verbatim and use ReMa's kinds and
//!    categories, and known skills are renamed to their canonical names.
//!    Anything else is dropped before it reaches the analytics.

use std::{collections::HashMap, sync::OnceLock};

use regex::Regex;
use serde::Deserialize;

use super::{normalize, skills};
use crate::{
    error::{AppError, AppResult},
    jobs::extract::json_object,
    models::analytics::{
        Importance, RequirementCategory as Cat, RequirementKind as Kind, RequirementSource,
    },
};

/// A requirement ready to be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRequirement {
    pub kind: Kind,
    pub category: Cat,
    pub name: String,
    pub original: String,
    pub importance: Importance,
    pub years: Option<u32>,
    pub level: Option<String>,
    pub source: RequirementSource,
}

impl NewRequirement {
    fn new(
        kind: Kind,
        category: Cat,
        name: &str,
        original: &str,
        importance: Importance,
        source: RequirementSource,
    ) -> Self {
        Self {
            kind,
            category,
            name: name.to_string(),
            original: normalize::clip(original, 200),
            importance,
            years: None,
            level: None,
            source,
        }
    }

    fn from_def(
        def: &skills::Def,
        original: &str,
        importance: Importance,
        source: RequirementSource,
    ) -> Self {
        Self::new(
            def.kind,
            def.category,
            def.name,
            original,
            importance,
            source,
        )
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

// ── Patterns ──────────────────────────────────────────────────────────

const PREFERRED_MARKERS: &[&str] = &[
    "nice to have",
    "nice-to-have",
    "a plus",
    "is a plus",
    "bonus",
    "preferred",
    "preferably",
    "ideally",
    "desirable",
    "advantage",
    "advantageous",
    "von vorteil",
    "wünschenswert",
    "optional",
    "would be great",
    "good to have",
    "not required",
];

fn is_preferred(text: &str) -> bool {
    let t = text.to_lowercase();
    PREFERRED_MARKERS.iter().any(|m| t.contains(m))
}

/// "5+ years of experience" → 5.
fn years(text: &str) -> Option<u32> {
    static YEARS: OnceLock<Regex> = OnceLock::new();
    let lower = text.to_lowercase();
    if ![
        "experience",
        "erfahrung",
        "background",
        "track record",
        "berufserfahrung",
        "working",
    ]
    .iter()
    .any(|w| lower.contains(w))
    {
        return None;
    }
    let caps = re(
        &YEARS,
        r"(?:at least|minimum of|min\.|mindestens)?\s*(\d{1,2})\s*\+?\s*(?:(?:-|–|to|bis)\s*\d{1,2}\s*\+?\s*)?(?:years?|yrs?|jahre?n?)\b",
    )
    .captures(&lower)?;
    caps[1].parse().ok().filter(|y| (1..=30).contains(y))
}

/// Degree level: 1 bachelor, 2 master, 3 doctorate (0: unspecified degree).
pub fn degree_level(text: &str) -> Option<u8> {
    static DEGREE: OnceLock<Regex> = OnceLock::new();
    let lower = text.to_lowercase();
    let r = re(
        &DEGREE,
        r"(?x)\b(?P<phd>ph\.?\s?d|doctorate|doctoral|doktor)
          |\b(?P<master>master'?s\b|masters\b|master\s+(?:degree|of|in)\b|msc|m\.\s?sc|m\.\s?eng|meng|mba|diplom\b|dipl\.|magister)
          |\b(?P<bachelor>bachelor'?s\b|bachelors\b|bachelor\s+(?:degree|of|in)\b|bsc|b\.\s?sc|b\.\s?eng|beng)
          |(?P<any>\b(?:a|university|college|academic|relevant|completed|technical)\s+degree\b|\bdegree\s+in\b|university\s+education|\bstudium\b|hochschulabschluss|universitätsabschluss)",
    );
    let mut level: Option<u8> = None;
    for caps in r.captures_iter(&lower) {
        let this = if caps.name("bachelor").is_some() {
            1
        } else if caps.name("master").is_some() {
            2
        } else if caps.name("phd").is_some() {
            3
        } else {
            0
        };
        // "Bachelor's or Master's": the lowest stated degree is the requirement.
        level = Some(match level {
            None => this,
            Some(0) => this,
            Some(l) if this == 0 => l,
            Some(l) => l.min(this),
        });
    }
    level
}

pub fn degree_name(level: u8) -> &'static str {
    match level {
        1 => "Bachelor's degree",
        2 => "Master's degree",
        3 => "PhD",
        _ => "University degree",
    }
}

/// CEFR or descriptive level of a spoken language near its mention.
fn language_level(text: &str) -> Option<String> {
    static CEFR: OnceLock<Regex> = OnceLock::new();
    let lower = text.to_lowercase();
    if let Some(c) = re(&CEFR, r"\b([abc][12])\b").captures(&lower) {
        return Some(c[1].to_uppercase());
    }
    let has = |w: &[&str]| w.iter().any(|x| lower.contains(x));
    if has(&["native", "mother tongue", "muttersprach"]) {
        Some("Native".into())
    } else if has(&[
        "fluent",
        "fluency",
        "business",
        "verhandlungssicher",
        "fließend",
        "fliessend",
        "excellent",
        "sehr gut",
        "proficien",
    ]) {
        Some("Fluent".into())
    } else if has(&["basic", "grundkenntnisse"]) {
        Some("Basic".into())
    } else if has(&["good", "gute", "working"]) {
        Some("Good".into())
    } else {
        None
    }
}

const LANGUAGE_CONTEXT: &[&str] = &[
    "fluent",
    "fluency",
    "proficien",
    "native",
    "business",
    "level",
    "written",
    "spoken",
    "skills",
    "knowledge",
    "kenntnisse",
    "sprach",
    "mother tongue",
    "verhandlungssicher",
    "fließend",
    "speak",
    "command of",
    "language",
];

/// The spoken language a word names ("Deutsch", "Deutschkenntnisse").
fn language_word(word: &str) -> Option<&'static str> {
    skills::language(word).or_else(|| {
        ["kenntnisse", "skills"]
            .iter()
            .find_map(|suffix| word.strip_suffix(suffix))
            .and_then(skills::language)
    })
}

/// Spoken languages a text requires ("fluent German (C1)"). A level after a
/// language (C1, "fluent") belongs to it; before it, only descriptive words
/// ("Fluent German") count, so "German (C1) and English" gives English no level.
fn languages(text: &str, list_item: bool) -> Vec<(&'static str, Option<String>)> {
    let lower = text.to_lowercase();
    static CEFR: OnceLock<Regex> = OnceLock::new();
    let context = list_item
        || LANGUAGE_CONTEXT.iter().any(|w| lower.contains(w))
        || re(&CEFR, r"\b[abc][12]\b").is_match(&lower);
    if !context {
        return Vec::new();
    }
    // (language, start, end) of every mention.
    let mut mentions: Vec<(&'static str, usize, usize)> = Vec::new();
    let mut word_start: Option<usize> = None;
    for (i, c) in lower
        .char_indices()
        .chain(std::iter::once((lower.len(), ' ')))
    {
        if c.is_alphabetic() && i < lower.len() {
            word_start.get_or_insert(i);
        } else if let Some(s) = word_start.take() {
            if let Some(name) = language_word(&lower[s..i]) {
                mentions.push((name, s, i));
            }
        }
    }
    let mut out: Vec<(&'static str, Option<String>)> = Vec::new();
    for (i, &(name, start, end)) in mentions.iter().enumerate() {
        if out.iter().any(|(n, _)| *n == name) {
            continue;
        }
        let next = mentions.get(i + 1).map_or(lower.len(), |m| m.1);
        let previous_end = if i == 0 { 0 } else { mentions[i - 1].2 };
        let after = &lower[end.min(next)..next];
        let before = &lower[previous_end.min(start)..start];
        let level = language_level(after).or_else(|| {
            language_level(before)
                .filter(|l| !(l.len() == 2 && l.chars().nth(1).is_some_and(|c| c.is_ascii_digit())))
        });
        out.push((name, level));
    }
    out
}

fn industries(text: &str) -> Vec<&'static str> {
    let lower = text.to_lowercase();
    if ![
        "experience in",
        "experience with",
        "background in",
        "industry",
        "domain",
        "sector",
        "branche",
    ]
    .iter()
    .any(|w| lower.contains(w))
    {
        return Vec::new();
    }
    skills::INDUSTRIES
        .iter()
        .filter(|(_, aliases)| {
            aliases.iter().any(|a| {
                lower
                    .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '.'))
                    .collect::<Vec<_>>()
                    .join(" ")
                    .contains(a)
            })
        })
        .map(|(name, _)| *name)
        .collect()
}

/// Requirements a single sentence or list item states.
fn from_text(
    text: &str,
    importance: Importance,
    source: RequirementSource,
    list_item: bool,
) -> Vec<NewRequirement> {
    let mut out = Vec::new();
    let original = normalize::clip(text, 200);
    if let Some(y) = years(text) {
        let mut r = NewRequirement::new(
            Kind::Experience,
            Cat::ProfessionalExperience,
            &format!("{y}+ years experience"),
            &original,
            importance,
            source,
        );
        r.years = Some(y);
        out.push(r);
    }
    if let Some(level) = degree_level(text) {
        let mut r = NewRequirement::new(
            Kind::Education,
            Cat::Education,
            degree_name(level),
            &original,
            if text.to_lowercase().contains("or equivalent") {
                Importance::Preferred
            } else {
                importance
            },
            source,
        );
        r.level = Some(degree_name(level).into());
        out.push(r);
    }
    for (name, level) in languages(text, list_item) {
        let mut r = NewRequirement::new(
            Kind::Language,
            Cat::Languages,
            name,
            &original,
            importance,
            source,
        );
        r.level = level;
        out.push(r);
    }
    for industry in industries(text) {
        out.push(NewRequirement::new(
            Kind::Experience,
            Cat::IndustryExperience,
            &format!("{industry} industry experience"),
            &original,
            importance,
            source,
        ));
    }
    let defs = if list_item {
        skills::lookup_item(text)
    } else {
        skills::scan(text).iter().map(skills::Found::def).collect()
    };
    for def in defs {
        out.push(NewRequirement::from_def(def, &original, importance, source));
    }
    out
}

/// Keeps one entry per (kind, name); a required mention wins over a
/// preferred one.
pub fn dedupe(requirements: Vec<NewRequirement>) -> Vec<NewRequirement> {
    let mut out: Vec<NewRequirement> = Vec::new();
    let mut index: HashMap<(Kind, String), usize> = HashMap::new();
    for r in requirements {
        let key = (r.kind, r.name.to_lowercase());
        match index.get(&key) {
            Some(&i) => {
                if r.importance == Importance::Required {
                    out[i].importance = Importance::Required;
                }
                if out[i].level.is_none() {
                    out[i].level = r.level;
                }
            }
            None => {
                index.insert(key, out.len());
                out.push(r);
            }
        }
    }
    out
}

/// Requirements from a search result's skills column. Items the dictionary
/// does not know are kept under their own (cleaned) name.
pub fn from_items(items: &[String]) -> Vec<NewRequirement> {
    let mut out = Vec::new();
    for item in items {
        let importance = if is_preferred(item) {
            Importance::Preferred
        } else {
            Importance::Required
        };
        let found = from_text(item, importance, RequirementSource::Search, true);
        if found.is_empty() {
            let name = normalize::clip(
                item.trim_matches(|c: char| c.is_ascii_punctuation() && c != '+' && c != '#'),
                60,
            );
            if name.is_empty() || name.chars().count() < 2 {
                continue;
            }
            let mut chars = name.chars();
            let name: String = chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default()
                + chars.as_str();
            let long = name.split_whitespace().count() > 4;
            out.push(NewRequirement::new(
                if long { Kind::Other } else { Kind::Skill },
                Cat::Other,
                &name,
                item,
                importance,
                RequirementSource::Search,
            ));
        } else {
            out.extend(found);
        }
    }
    dedupe(out)
}

const BENEFIT_HEADINGS: &[&str] = &[
    "benefit",
    "what we offer",
    "we offer",
    "our offer",
    "perks",
    "wir bieten",
    "what you get",
    "why join",
    "why us",
    "unser angebot",
];

/// A section heading ("Your profile:", "## Nice to have", "What we offer").
/// Short lines that name skills ("Python, SQL") are content, not headings.
fn is_heading(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('#')
        || (t.ends_with(':') && t.split_whitespace().count() <= 8)
        || (t.split_whitespace().count() <= 5
            && !t.ends_with(['.', ','])
            && !t.starts_with(['-', '*', '•'])
            && t.chars().next().is_some_and(char::is_uppercase)
            && skills::scan(t).is_empty()
            && languages(t, false).is_empty())
}

/// What a job description states: requirements and offered benefits.
#[derive(Debug, Default, PartialEq)]
pub struct DescriptionFacts {
    pub requirements: Vec<NewRequirement>,
    pub benefits: Vec<String>,
}

/// Deterministic extraction from a job description.
pub fn from_description(description: &str) -> DescriptionFacts {
    let mut facts = DescriptionFacts::default();
    let mut section_preferred = false;
    let mut in_benefits = false;
    for line in description.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_heading(trimmed) {
            let lower = trimmed.to_lowercase();
            in_benefits = BENEFIT_HEADINGS.iter().any(|h| lower.contains(h));
            section_preferred = is_preferred(&lower);
            // A heading like "Nice to have: Rust, Go" also lists items.
            if let Some((_, rest)) = trimmed.split_once(':') {
                if !rest.trim().is_empty() && !in_benefits {
                    let importance = if section_preferred {
                        Importance::Preferred
                    } else {
                        Importance::Required
                    };
                    facts.requirements.extend(from_text(
                        rest,
                        importance,
                        RequirementSource::Description,
                        false,
                    ));
                }
            }
            continue;
        }
        let item = trimmed
            .trim_start_matches(['-', '*', '•', '–', '·', ' '])
            .trim();
        if in_benefits {
            if facts.benefits.len() < 12 && trimmed.starts_with(['-', '*', '•', '–', '·']) {
                facts.benefits.push(normalize::clip(item, 120));
            }
            continue;
        }
        for sentence in item
            .split_inclusive(['.', ';'])
            .filter(|s| !s.trim().is_empty())
        {
            let importance = if section_preferred || is_preferred(sentence) {
                Importance::Preferred
            } else {
                Importance::Required
            };
            facts.requirements.extend(from_text(
                sentence,
                importance,
                RequirementSource::Description,
                false,
            ));
        }
    }
    facts.requirements = dedupe(std::mem::take(&mut facts.requirements));
    facts
}

// ── Model extraction ──────────────────────────────────────────────────

/// Longest description text sent to the model.
pub const MAX_MODEL_INPUT: usize = 12_000;
const MAX_MODEL_REQUIREMENTS: usize = 40;

pub const RULES: &str = r#"You read one job description for ReMa, a career app, and list the requirements it states, as JSON.

Rules:
- Include only what the description asks of candidates: skills, technologies, tools, years of experience, industry experience, education, certifications, spoken languages, soft skills.
- Do not include benefits, company facts, or tasks that are not requirements. Never add anything the text does not state.
- "quote": the exact words from the description (copied verbatim, at most one sentence) that state the requirement.
- "name": a short canonical name, e.g. "Kubernetes", "PostgreSQL", "5+ years experience", "Master's degree", "German", "AWS Certified Solutions Architect".
- "kind": one of "skill", "experience", "education", "certification", "language", "soft_skill", "other".
- "category": one of "technical_skills", "programming_languages", "frameworks", "cloud_infrastructure", "ai_ml", "data_engineering", "databases", "professional_experience", "industry_experience", "education", "certifications", "languages", "soft_skills", "other".
- "importance": "preferred" if the text marks it as nice to have / a plus / preferred; otherwise "required".
- "years": minimum years for experience requirements, else null. "level": language level (e.g. "C1", "Fluent") or degree, else null.

Return JSON only: {"requirements": [{"quote": "...", "name": "...", "kind": "...", "category": "...", "importance": "...", "years": null, "level": null}]}"#;

#[derive(Deserialize)]
struct ModelAnswer {
    #[serde(default)]
    requirements: Vec<ModelRequirement>,
}

#[derive(Deserialize)]
struct ModelRequirement {
    #[serde(default)]
    quote: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    importance: String,
    #[serde(default)]
    years: Option<f64>,
    #[serde(default)]
    level: Option<String>,
}

fn simplified(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '+' || c == '#' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Validates the model's answer against the description. Returns the
/// accepted requirements and how many were dropped.
pub fn parse_model(answer: &str, description: &str) -> AppResult<(Vec<NewRequirement>, usize)> {
    let parsed: ModelAnswer = serde_json::from_str(json_object(answer)?)
        .map_err(|_| AppError::provider("the requirements were not in the expected format"))?;
    let text = simplified(description);
    let mut accepted = Vec::new();
    let mut dropped = 0;
    for raw in parsed
        .requirements
        .into_iter()
        .take(MAX_MODEL_REQUIREMENTS * 2)
    {
        let quote = simplified(&raw.quote);
        let name = normalize::clip(&raw.name, 60);
        let (Some(kind), Some(category), Some(importance)) = (
            Kind::parse(raw.kind.trim()),
            Cat::parse(raw.category.trim()),
            Importance::parse(raw.importance.trim()),
        ) else {
            dropped += 1;
            continue;
        };
        if quote.len() < 2
            || !text.contains(&quote)
            || name.is_empty()
            || name.split_whitespace().count() > 6
        {
            dropped += 1;
            continue;
        }
        let mut r = NewRequirement::new(
            kind,
            category,
            &name,
            &raw.quote,
            importance,
            RequirementSource::Model,
        );
        // Known names win over the model's wording and category.
        let known = skills::lookup_item(&name);
        if !known.is_empty()
            && kind != Kind::Language
            && kind != Kind::Education
            && kind != Kind::Experience
        {
            for def in known {
                accepted.push(NewRequirement::from_def(
                    def,
                    &raw.quote,
                    importance,
                    RequirementSource::Model,
                ));
            }
            if accepted.len() >= MAX_MODEL_REQUIREMENTS {
                break;
            }
            continue;
        } else if kind == Kind::Language {
            match name
                .split(|c: char| !c.is_alphabetic())
                .find_map(|w| language_word(&w.to_lowercase()))
            {
                Some(lang) => r.name = lang.to_string(),
                None => {
                    dropped += 1;
                    continue;
                }
            }
        } else if kind == Kind::Education {
            r.name =
                degree_name(degree_level(&format!("{name} {}", raw.quote)).unwrap_or(0)).into();
        }
        if kind == Kind::Experience && category == Cat::ProfessionalExperience {
            match raw.years.filter(|y| (1.0..=30.0).contains(y)) {
                Some(y) => {
                    r.years = Some(y as u32);
                    r.name = format!("{}+ years experience", y as u32);
                }
                None => r.years = years(&raw.quote),
            }
        }
        r.level = raw
            .level
            .map(|l| normalize::clip(&l, 20))
            .filter(|l| !l.is_empty());
        accepted.push(r);
        if accepted.len() >= MAX_MODEL_REQUIREMENTS {
            break;
        }
    }
    Ok((dedupe(accepted), dropped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(reqs: &[NewRequirement]) -> Vec<(&str, Importance)> {
        reqs.iter()
            .map(|r| (r.name.as_str(), r.importance))
            .collect()
    }

    const DESCRIPTION: &str = "About the role\n\
We build ML platforms on Kubernetes.\n\
\n\
Your profile:\n\
- 5+ years of experience in software engineering\n\
- Strong Python and Go; experience with K8s clusters\n\
- Master's degree in Computer Science or equivalent\n\
- Fluent German (C1) and English\n\
- Experience in the banking industry\n\
\n\
Nice to have:\n\
- Terraform, Ray\n\
\n\
What we offer:\n\
- Learning budget for AWS certifications\n\
- Flexible hours\n";

    #[test]
    fn extracts_requirements_from_descriptions() {
        let facts = from_description(DESCRIPTION);
        let found = names(&facts.requirements);
        use Importance::*;
        for expected in [
            ("Kubernetes", Required),
            ("5+ years experience", Required),
            ("Python", Required),
            ("Go", Required),
            ("Master's degree", Preferred),
            ("German", Required),
            ("English", Required),
            ("Financial services industry experience", Required),
            ("Terraform", Preferred),
            ("Ray", Preferred),
        ] {
            assert!(
                found.contains(&expected),
                "missing {expected:?} in {found:?}"
            );
        }
        // Benefits are not requirements.
        assert!(!found.iter().any(|(n, _)| n.contains("AWS")));
        assert_eq!(
            facts.benefits,
            ["Learning budget for AWS certifications", "Flexible hours"]
        );
        let german = facts
            .requirements
            .iter()
            .find(|r| r.name == "German")
            .unwrap();
        assert_eq!(german.level.as_deref(), Some("C1"));
        let english = facts
            .requirements
            .iter()
            .find(|r| r.name == "English")
            .unwrap();
        assert_eq!(english.level, None, "C1 belongs to German");
        assert_eq!(
            languages("Fluent in German and English", false)[0],
            ("German", Some("Fluent".into()))
        );
        assert_eq!(
            languages("Sehr gute Deutschkenntnisse", false)[0].0,
            "German"
        );
    }

    #[test]
    fn normalizes_skills_columns() {
        let reqs = from_items(&[
            "K8s".into(),
            "Postgres".into(),
            "LangSmith".into(),
            "German C1".into(),
            "3+ years MLOps experience".into(),
            "Terraform (nice to have)".into(),
        ]);
        let found = names(&reqs);
        assert!(found.contains(&("Kubernetes", Importance::Required)));
        assert!(found.contains(&("PostgreSQL", Importance::Required)));
        assert!(found.contains(&("LangSmith", Importance::Required)));
        assert!(found.contains(&("German", Importance::Required)));
        assert!(found.contains(&("3+ years experience", Importance::Required)));
        assert!(found.contains(&("MLOps", Importance::Required)));
        assert!(found.contains(&("Terraform", Importance::Preferred)));
        let unknown = reqs.iter().find(|r| r.name == "LangSmith").unwrap();
        assert_eq!(unknown.category, Cat::Other);
    }

    #[test]
    fn reads_degree_levels() {
        assert_eq!(degree_level("Bachelor's or Master's degree"), Some(1));
        assert_eq!(degree_level("MSc or PhD in physics"), Some(2));
        assert_eq!(degree_level("University degree in CS"), Some(0));
        assert_eq!(degree_level("No relevant words"), None);
        assert_eq!(degree_level("Certified Scrum Master"), None);
        assert_eq!(degree_level("a 360 degree view"), None);
    }

    #[test]
    fn validates_model_requirements_against_the_text() {
        let description =
            "We need strong Kubernetes skills and 4 years of experience. Rust is a plus.";
        let answer = r#"```json
{"requirements": [
 {"quote": "strong Kubernetes skills", "name": "k8s", "kind": "skill", "category": "other", "importance": "required"},
 {"quote": "4 years of experience", "name": "4+ years", "kind": "experience", "category": "professional_experience", "importance": "required", "years": 4},
 {"quote": "Rust is a plus", "name": "Rust", "kind": "skill", "category": "programming_languages", "importance": "preferred"},
 {"quote": "Java experience", "name": "Java", "kind": "skill", "category": "programming_languages", "importance": "required"},
 {"quote": "strong Kubernetes skills", "name": "Magic", "kind": "wizardry", "category": "other", "importance": "required"}
]}
```"#;
        let (reqs, dropped) = parse_model(answer, description).unwrap();
        assert_eq!(
            names(&reqs),
            [
                ("Kubernetes", Importance::Required),
                ("4+ years experience", Importance::Required),
                ("Rust", Importance::Preferred)
            ]
        );
        // Kubernetes is recategorized by the dictionary, not by the model.
        assert_eq!(reqs[0].category, Cat::CloudInfrastructure);
        assert_eq!(dropped, 2, "unquoted and invalid entries are dropped");
        assert!(parse_model("no json", description).is_err());
    }
}
