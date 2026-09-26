//! Profile ↔ requirement comparison.
//!
//! Evidence comes only from the structured Profile (skills, experience,
//! education, languages, custom fields, certificate names). Each
//! requirement gets one state:
//!
//! - **Matched**: the Profile names it (or, for a generic requirement such
//!   as "Cloud computing", a skill of that family like AWS).
//! - **Partial**: only a related skill of the same family (Azure for AWS), a
//!   lower language level, fewer years, or a lower degree.
//! - **Missing**: a concrete skill, tool, certification or language the
//!   Profile does not show.
//! - **Unknown**: what the Profile cannot show either way (soft skills,
//!   industry background, education or languages left empty). Unknown is
//!   never counted as a gap.

use std::collections::HashMap;

use jiff::civil::Date;

use super::{dataset::Requirement, extract, normalize, skills};
use crate::models::{
    analytics::{MatchState, RequirementCategory, RequirementKind},
    profile::{DocumentKind, Profile, ProfileDocument},
};

#[derive(Debug, Clone, Default)]
pub struct Evidence {
    pub available: bool,
    /// Canonical skill key (lowercase) → where the Profile shows it.
    skills: HashMap<String, String>,
    /// Skill family → a Profile skill of that family.
    families: HashMap<&'static str, String>,
    /// All Profile text, lowercase, for literal matches.
    text: String,
    /// Language → level rank (None: level not stated).
    languages: HashMap<&'static str, Option<u8>>,
    languages_listed: bool,
    pub years: Option<f64>,
    degree: Option<u8>,
    has_education: bool,
}

/// CEFR-like rank of a language level.
fn level_rank(level: &str) -> Option<u8> {
    let l = level.trim().to_lowercase();
    Some(match l.as_str() {
        "a1" => 1,
        "a2" => 2,
        "b1" => 3,
        "b2" => 4,
        "c1" => 5,
        "c2" => 6,
        _ if l.contains("native") || l.contains("mother") || l.contains("muttersprach") => 7,
        _ if [
            "fluent",
            "business",
            "excellent",
            "proficien",
            "verhandlungssicher",
            "fließend",
        ]
        .iter()
        .any(|w| l.contains(w)) =>
        {
            5
        }
        _ if ["good", "working", "intermediate", "gut"]
            .iter()
            .any(|w| l.contains(w)) =>
        {
            4
        }
        _ if ["basic", "elementary", "beginner", "grund"]
            .iter()
            .any(|w| l.contains(w)) =>
        {
            2
        }
        _ => return None,
    })
}

/// A year-month from "2021", "2021-03", "03/2021", "Mar 2021", "present".
fn month_of(text: &str, today: Date) -> Option<(i32, i32)> {
    let t = text.trim().to_lowercase();
    if [
        "present", "now", "current", "today", "heute", "aktuell", "ongoing",
    ]
    .iter()
    .any(|w| t.contains(w))
    {
        return Some((i32::from(today.year()), i32::from(today.month())));
    }
    let digits: Vec<&str> = t
        .split(|c: char| !c.is_ascii_digit())
        .filter(|d| !d.is_empty())
        .collect();
    let year = digits.iter().find(|d| d.len() == 4)?.parse::<i32>().ok()?;
    if !(1950..=2100).contains(&year) {
        return None;
    }
    let month = digits
        .iter()
        .filter(|d| d.len() <= 2)
        .find_map(|d| d.parse::<i32>().ok().filter(|m| (1..=12).contains(m)))
        .or_else(|| {
            t.split(|c: char| !c.is_alphabetic()).find_map(|w| {
                const NAMES: [&str; 12] = [
                    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov",
                    "dec",
                ];
                NAMES
                    .iter()
                    .position(|n| w.starts_with(n))
                    .map(|i| i as i32 + 1)
            })
        })
        .unwrap_or(1);
    Some((year, month))
}

/// Total professional years, overlapping positions counted once.
fn experience_years(profile: &Profile, today: Date) -> Option<f64> {
    let mut spans: Vec<(i32, i32)> = profile
        .experience
        .iter()
        .filter_map(|e| {
            let (sy, sm) = month_of(&e.start, today)?;
            let (ey, em) = if e.current {
                (i32::from(today.year()), i32::from(today.month()))
            } else {
                month_of(&e.end, today)?
            };
            let (start, end) = (sy * 12 + sm - 1, ey * 12 + em);
            (end > start).then_some((start, end))
        })
        .collect();
    if spans.is_empty() {
        return None;
    }
    spans.sort_unstable();
    let mut months = 0;
    let mut current: Option<(i32, i32)> = None;
    for (s, e) in spans {
        current = match current {
            Some((cs, ce)) if s <= ce => Some((cs, ce.max(e))),
            Some((cs, ce)) => {
                months += ce - cs;
                Some((s, e))
            }
            None => Some((s, e)),
        };
    }
    if let Some((cs, ce)) = current {
        months += ce - cs;
    }
    Some(f64::from(months) / 12.0)
}

impl Evidence {
    pub fn from_profile(profile: &Profile, documents: &[ProfileDocument], today: Date) -> Self {
        let certificates: Vec<(String, String)> = documents
            .iter()
            .filter(|d| d.kind == DocumentKind::Certificate)
            .map(|d| {
                (
                    format!("Certificate: {}", d.name),
                    d.name.replace(['_', '-'], " "),
                )
            })
            .collect();
        Self::from_sources(profile, &certificates, today)
    }

    /// Evidence from the Custom Profile plus other Profile sources as
    /// (label, text): uploaded CV text and credentials. Any of them is
    /// enough (a CV alone works).
    pub fn from_sources(profile: &Profile, sources: &[(String, String)], today: Date) -> Self {
        let mut e = Evidence {
            available: !profile.is_empty() || sources.iter().any(|(_, t)| !t.trim().is_empty()),
            ..Evidence::default()
        };
        if !e.available {
            return e;
        }
        let note = |e: &mut Evidence, def: &skills::Def, source: &str| {
            e.skills
                .entry(def.name.to_lowercase())
                .or_insert_with(|| source.to_string());
            if let Some(family) = def.family {
                e.families
                    .entry(family)
                    .or_insert_with(|| def.name.to_string());
            }
        };
        for skill in &profile.skills {
            let defs = skills::lookup_item(skill);
            if defs.is_empty() {
                e.skills
                    .entry(skill.trim().to_lowercase())
                    .or_insert_with(|| "Skills".into());
            }
            for def in defs {
                note(&mut e, def, "Skills");
            }
        }
        let mut texts: Vec<(String, String)> = vec![
            ("Title".into(), profile.title.clone()),
            ("Summary".into(), profile.summary.clone()),
        ];
        for x in &profile.experience {
            let at = if x.company.is_empty() {
                String::new()
            } else {
                format!(" at {}", x.company)
            };
            texts.push((
                format!("Experience: {}{at}", x.title),
                format!("{} {}", x.title, x.description),
            ));
        }
        for x in &profile.education {
            texts.push((
                format!("Education: {} {}", x.degree, x.field),
                format!("{} {} {} {}", x.degree, x.field, x.school, x.description),
            ));
        }
        for f in &profile.custom_fields {
            texts.push((f.label.clone(), format!("{} {}", f.label, f.value)));
        }
        texts.extend(
            sources
                .iter()
                .filter(|(_, t)| !t.trim().is_empty())
                .cloned(),
        );
        for (source, text) in &texts {
            for found in skills::scan(text) {
                note(&mut e, found.def(), source);
            }
        }
        e.text = texts
            .iter()
            .map(|(_, t)| t.to_lowercase())
            .chain(profile.skills.iter().map(|s| s.to_lowercase()))
            .collect::<Vec<_>>()
            .join(" \n ");
        e.languages_listed = !profile.languages.is_empty();
        for lang in &profile.languages {
            let name = lang
                .name
                .split(|c: char| !c.is_alphabetic())
                .find_map(|w| skills::language(&w.to_lowercase()));
            if let Some(name) = name {
                e.languages.insert(name, level_rank(&lang.level));
            }
        }
        e.years = experience_years(profile, today);
        e.has_education = !profile.education.is_empty();
        e.degree = profile
            .education
            .iter()
            .filter_map(|x| {
                extract::degree_level(&format!("{} {}", x.degree, x.field)).filter(|l| *l > 0)
            })
            .max();
        e
    }

    /// Whether the Profile text names `phrase` as a whole word.
    fn mentions(&self, phrase: &str) -> bool {
        let p = phrase.trim().to_lowercase();
        if p.len() < 2 {
            return false;
        }
        let mut from = 0;
        while let Some(pos) = self.text[from..].find(&p).map(|i| from + i) {
            let before = self.text[..pos].chars().next_back();
            let after = self.text[pos + p.len()..].chars().next();
            let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
            if boundary(before) && boundary(after) {
                return true;
            }
            from = pos + p.len();
        }
        false
    }

    /// The Profile's state for one requirement, with the evidence used.
    pub fn state(&self, r: &Requirement) -> (MatchState, Option<String>) {
        if !self.available {
            return (MatchState::Unknown, None);
        }
        match r.kind {
            RequirementKind::Language => self.language(r),
            RequirementKind::Education => self.education(r),
            RequirementKind::Experience
                if r.category == RequirementCategory::IndustryExperience =>
            {
                let industry = r.name.trim_end_matches(" industry experience");
                let aliases = skills::INDUSTRIES
                    .iter()
                    .find(|(name, _)| *name == industry)
                    .map(|(_, a)| *a)
                    .unwrap_or_default();
                if self.mentions(industry) || aliases.iter().any(|a| self.mentions(a)) {
                    (MatchState::Matched, Some("Profile text".into()))
                } else {
                    (MatchState::Unknown, None)
                }
            }
            RequirementKind::Experience => self.years_state(r),
            RequirementKind::SoftSkill => match self.skills.get(&r.key) {
                Some(source) => (MatchState::Matched, Some(source.clone())),
                None if self.mentions(&r.name) => {
                    (MatchState::Matched, Some("Profile text".into()))
                }
                None => (MatchState::Unknown, None),
            },
            RequirementKind::Skill | RequirementKind::Certification | RequirementKind::Other => {
                self.skill(r)
            }
        }
    }

    fn skill(&self, r: &Requirement) -> (MatchState, Option<String>) {
        if let Some(source) = self.skills.get(&r.key) {
            return (MatchState::Matched, Some(source.clone()));
        }
        let def = skills::by_name(&r.name);
        if let Some(def) = def {
            if let Some(family) = def.family {
                if let Some(related) = self.families.get(family) {
                    return if def.generic {
                        (MatchState::Matched, Some(format!("{related} (Profile)")))
                    } else {
                        (MatchState::Partial, Some(format!("related: {related}")))
                    };
                }
            }
        }
        if self.mentions(&r.name) {
            return (MatchState::Matched, Some("Profile text".into()));
        }
        let concrete = def.is_some()
            || r.kind == RequirementKind::Certification
            || (r.kind == RequirementKind::Skill && r.name.split_whitespace().count() <= 4);
        if concrete {
            (MatchState::Missing, None)
        } else {
            (MatchState::Unknown, None)
        }
    }

    fn language(&self, r: &Requirement) -> (MatchState, Option<String>) {
        if !self.languages_listed {
            return (MatchState::Unknown, None);
        }
        let Some(name) = skills::language(&r.name.to_lowercase()) else {
            return (MatchState::Unknown, None);
        };
        match self.languages.get(name) {
            None => (MatchState::Missing, None),
            Some(have) => match (r.level.as_deref().and_then(level_rank), have) {
                (None, _) => (MatchState::Matched, Some("Languages".into())),
                (Some(need), Some(have)) if *have >= need => {
                    (MatchState::Matched, Some("Languages".into()))
                }
                (Some(_), Some(_)) => (MatchState::Partial, Some("Languages: lower level".into())),
                (Some(_), None) => (
                    MatchState::Unknown,
                    Some("Languages: level not stated".into()),
                ),
            },
        }
    }

    fn education(&self, r: &Requirement) -> (MatchState, Option<String>) {
        let need = extract::degree_level(&r.name).unwrap_or(0).max(1);
        match (self.has_education, self.degree) {
            (false, _) | (true, None) => (MatchState::Unknown, None),
            (true, Some(have)) if have >= need => (MatchState::Matched, Some("Education".into())),
            (true, Some(_)) => (MatchState::Partial, Some("Education: lower degree".into())),
        }
    }

    fn years_state(&self, r: &Requirement) -> (MatchState, Option<String>) {
        let (Some(need), Some(have)) = (r.years, self.years) else {
            return (MatchState::Unknown, None);
        };
        let need = f64::from(need);
        let source = Some(format!("Experience: {have:.1} years"));
        if have >= need {
            (MatchState::Matched, source)
        } else if have >= need * 0.6 {
            (MatchState::Partial, source)
        } else {
            (MatchState::Missing, source)
        }
    }
}

/// Today's date for Profile durations.
pub fn today() -> Date {
    normalize::date_of(crate::time::now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        analytics::Importance,
        profile::{Education, Experience, Language},
    };

    fn req(kind: RequirementKind, name: &str) -> Requirement {
        let category = skills::by_name(name).map_or(RequirementCategory::Other, |d| d.category);
        Requirement {
            kind,
            category,
            name: name.into(),
            key: name.to_lowercase(),
            importance: Importance::Required,
            years: None,
            level: None,
            original: name.into(),
        }
    }

    fn profile() -> Profile {
        Profile {
            title: "Data Engineer".into(),
            skills: vec![
                "Python".into(),
                "K8s".into(),
                "Azure".into(),
                "LangSmith".into(),
            ],
            experience: vec![
                Experience {
                    title: "Senior Data Engineer".into(),
                    company: "Globex".into(),
                    start: "2021-03".into(),
                    current: true,
                    description: "Led the team building Kafka pipelines in banking.".into(),
                    ..Default::default()
                },
                Experience {
                    title: "Data Engineer".into(),
                    start: "2018".into(),
                    end: "2021".into(),
                    ..Default::default()
                },
            ],
            education: vec![Education {
                degree: "MSc".into(),
                field: "Computer Science".into(),
                ..Default::default()
            }],
            languages: vec![
                Language {
                    name: "German".into(),
                    level: "Native".into(),
                },
                Language {
                    name: "English".into(),
                    level: "B2".into(),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn classifies_requirements() {
        let today = Date::new(2025, 9, 25).unwrap();
        let e = Evidence::from_profile(&profile(), &[], today);
        let state = |kind, name: &str| e.state(&req(kind, name)).0;
        use MatchState::*;
        use RequirementKind::*;
        assert_eq!(state(Skill, "Kubernetes"), Matched, "K8s in Skills");
        assert_eq!(state(Skill, "Apache Kafka"), Matched, "named in experience");
        assert_eq!(state(Skill, "AWS"), Partial, "Azure is a related cloud");
        assert_eq!(
            state(Skill, "Cloud computing"),
            Matched,
            "a cloud platform covers the generic need"
        );
        assert_eq!(state(Skill, "Terraform"), Missing);
        assert_eq!(state(Skill, "LangSmith"), Matched);
        assert_eq!(state(Certification, "CKA"), Missing);
        assert_eq!(
            state(SoftSkill, "Communication"),
            Unknown,
            "not shown, but not ruled out"
        );
        assert_eq!(
            state(
                Other,
                "Experience designing large-scale event systems for regulators"
            ),
            Unknown
        );
        assert_eq!(state(Language, "German"), Matched);
        assert_eq!(state(Language, "French"), Missing);
        let mut english = req(Language, "English");
        english.level = Some("C1".into());
        assert_eq!(e.state(&english).0, Partial);
        assert_eq!(state(Education, "Master's degree"), Matched);
        assert_eq!(state(Education, "PhD"), Partial);
        let mut years = req(Experience, "5+ years experience");
        years.years = Some(5);
        assert_eq!(e.state(&years).0, Matched, "2018–now is more than 7 years");
        years.years = Some(9);
        assert_eq!(e.state(&years).0, Partial, "7.7 of 9 years");
        years.years = Some(15);
        assert_eq!(e.state(&years).0, Missing);
        let mut industry = req(Experience, "Financial services industry experience");
        industry.category = RequirementCategory::IndustryExperience;
        assert_eq!(
            e.state(&industry).0,
            Matched,
            "banking is named in the experience"
        );
        let mut insurance = industry.clone();
        insurance.name = "Insurance industry experience".into();
        assert_eq!(
            e.state(&insurance).0,
            Unknown,
            "a Profile cannot rule out an industry"
        );
    }

    #[test]
    fn an_empty_profile_decides_nothing() {
        let e = Evidence::from_profile(&Profile::default(), &[], Date::new(2025, 1, 1).unwrap());
        assert!(!e.available);
        assert_eq!(
            e.state(&req(RequirementKind::Skill, "Python")).0,
            MatchState::Unknown
        );
    }

    #[test]
    fn an_uploaded_cv_alone_is_evidence() {
        let cv = (
            "CV: resume.pdf".to_string(),
            "Data engineer. Built pipelines with Python, Apache Kafka and Kubernetes.".to_string(),
        );
        let e = Evidence::from_sources(&Profile::default(), &[cv], Date::new(2025, 1, 1).unwrap());
        assert!(e.available);
        let state = |name: &str| e.state(&req(RequirementKind::Skill, name));
        assert_eq!(state("Python").0, MatchState::Matched);
        assert_eq!(state("Apache Kafka").1.as_deref(), Some("CV: resume.pdf"));
        assert_eq!(state("Terraform").0, MatchState::Missing);
        // Blank sources are not evidence.
        let blank = ("CV: empty.pdf".to_string(), "  ".to_string());
        let e = Evidence::from_sources(&Profile::default(), &[blank], Date::new(2025, 1, 1).unwrap());
        assert!(!e.available);
    }

    #[test]
    fn counts_overlapping_experience_once() {
        let today = Date::new(2025, 1, 1).unwrap();
        let p = Profile {
            experience: vec![
                Experience {
                    start: "2020-01".into(),
                    end: "2022-01".into(),
                    ..Default::default()
                },
                Experience {
                    start: "2021-01".into(),
                    end: "2023-01".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let years = experience_years(&p, today).unwrap();
        assert!((years - 3.0).abs() < 0.1, "{years}");
    }
}
