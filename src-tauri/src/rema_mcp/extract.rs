//! Deterministic extraction shared by the adapters: dates, descriptions and
//! their sections (English, German, Polish), skills, language, seniority,
//! employment terms and salaries. Nothing here guesses a value the source
//! does not state: an unstated salary period stays unknown, a single
//! amount is not widened into a range, relative dates keep their reference.

use super::contract::{
    Compensation, Description, DescriptionState, EmploymentType, ListsStatus, Location,
    SalaryBasis, SalaryBound, SalaryPeriod, Section, SectionKind, Seniority, WorkMode, WorkingTime,
};
use crate::{
    analytics::{normalize, page, skills},
    models::analytics as am,
};

pub const MAX_DESCRIPTION_CHARS: usize = page::MAX_DESCRIPTION_CHARS;

// ── Time ──────────────────────────────────────────────────────────────

/// RFC 3339 UTC, to the second.
pub fn iso(ms: i64) -> String {
    jiff::Timestamp::from_second(ms.div_euclid(1000))
        .map(|t| t.to_string())
        .unwrap_or_default()
}

pub fn iso_date(ms: i64) -> String {
    normalize::date_of(ms).to_string()
}

/// A calendar date from "2026-09-20", "2026-09-20T10:00:00+02:00" or epoch
/// milliseconds. Returns (YYYY-MM-DD, start of that day in ms).
pub fn date(text: &str) -> Option<(String, i64)> {
    let text = text.trim();
    if text.len() >= 12 && text.chars().all(|c| c.is_ascii_digit()) {
        let ms: i64 = text.parse().ok()?;
        return Some((iso_date(ms), normalize::start_of_day(ms)));
    }
    let day: jiff::civil::Date = text.get(..10)?.parse().ok()?;
    let ms = day
        .to_zoned(jiff::tz::TimeZone::UTC)
        .ok()?
        .timestamp()
        .as_millisecond();
    Some((day.to_string(), ms))
}

pub fn date_ms(ms: i64) -> (String, i64) {
    (iso_date(ms), normalize::start_of_day(ms))
}

// ── Text ──────────────────────────────────────────────────────────────

/// Clean text from an HTML fragment (entities decoded, no markup, no
/// scripts). The second value tells whether it was cut at the limit.
pub fn clean_html(html: &str) -> (String, bool) {
    let decoded = page::decode_entities(html);
    let text = page::html_to_text(&decoded);
    let truncated = text.chars().count() >= MAX_DESCRIPTION_CHARS;
    (text, truncated)
}

pub fn clip(text: &str, max: usize) -> String {
    normalize::clip(text.trim(), max)
}

fn has_any(lower: &str, words: &[&str]) -> bool {
    words.iter().any(|w| lower.contains(w))
}

const PREFERRED_HEADINGS: &[&str] = &[
    "nice to have",
    "nice-to-have",
    "preferred",
    "bonus points",
    "a plus",
    "desirable",
    "wünschenswert",
    "von vorteil",
    "pluspunkte",
    "mile widziane",
    "dodatkowym atutem",
    "atutem będzie",
];
const REQUIREMENT_HEADINGS: &[&str] = &[
    "requirement",
    "qualification",
    "what you bring",
    "what we're looking for",
    "what we are looking for",
    "your profile",
    "you have",
    "must have",
    "about you",
    "who you are",
    "skills",
    "anforderung",
    "ihr profil",
    "dein profil",
    "was du mitbringst",
    "was sie mitbringen",
    "das bringst du mit",
    "das bringen sie mit",
    "qualifikation",
    "voraussetzung",
    "wymagania",
    "oczekujemy",
    "twój profil",
    "kwalifikacje",
];
const RESPONSIBILITY_HEADINGS: &[&str] = &[
    "responsibilit",
    "what you'll do",
    "what you will do",
    "your tasks",
    "your role",
    "the role",
    "tasks",
    "aufgaben",
    "das erwartet dich",
    "das erwartet sie",
    "tätigkeit",
    "obowiązki",
    "zakres obowiązków",
    "twoje zadania",
    "zadania",
];
const BENEFIT_HEADINGS: &[&str] = &[
    "benefit",
    "what we offer",
    "we offer",
    "perks",
    "why join",
    "wir bieten",
    "was wir bieten",
    "das bieten wir",
    "unser angebot",
    "oferujemy",
    "co oferujemy",
    "benefity",
];
const ABOUT_HEADINGS: &[&str] = &[
    "about us",
    "about the company",
    "who we are",
    "über uns",
    "o nas",
    "about the role",
];

fn heading_kind(line: &str) -> Option<SectionKind> {
    let trimmed = line.trim();
    if trimmed.starts_with("- ") || trimmed.chars().count() > 60 || trimmed.ends_with('.') {
        return None;
    }
    let lower = trimmed.trim_end_matches(':').to_lowercase();
    if !(trimmed.ends_with(':') || lower.split_whitespace().count() <= 6) {
        return None;
    }
    if has_any(&lower, PREFERRED_HEADINGS) {
        Some(SectionKind::Preferred)
    } else if has_any(&lower, BENEFIT_HEADINGS) {
        Some(SectionKind::Benefits)
    } else if has_any(&lower, RESPONSIBILITY_HEADINGS) {
        Some(SectionKind::Responsibilities)
    } else if has_any(&lower, REQUIREMENT_HEADINGS) {
        Some(SectionKind::Requirements)
    } else if has_any(&lower, ABOUT_HEADINGS) {
        Some(SectionKind::About)
    } else {
        None
    }
}

/// Items in a requirement list that say they are optional.
const PREFERRED_MARKERS: &[&str] = &[
    "nice to have",
    "nice-to-have",
    "is a plus",
    "a plus",
    "preferred",
    "ideally",
    "bonus",
    "von vorteil",
    "wünschenswert",
    "idealerweise",
    "mile widziane",
    "atutem",
];

pub fn sections(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for line in text.lines() {
        if let Some(kind) = heading_kind(line) {
            out.push(Section {
                heading: clip(line.trim_end_matches(':'), 80),
                kind,
                text: String::new(),
            });
        } else if let Some(section) = out.last_mut() {
            if !section.text.is_empty() {
                section.text.push('\n');
            }
            section.text.push_str(line.trim());
        }
    }
    out.retain(|s| !s.text.is_empty());
    out
}

fn items(section: &Section) -> Vec<String> {
    let bullets: Vec<String> = section
        .text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .map(|l| clip(l, 300))
        .filter(|l| !l.is_empty())
        .collect();
    let items = if bullets.is_empty() {
        section
            .text
            .lines()
            .map(|l| clip(l, 300))
            .filter(|l| !l.is_empty())
            .collect()
    } else {
        bullets
    };
    items.into_iter().take(25).collect()
}

/// Skill names found in a text (ReMa's skill dictionary).
pub fn skill_names(text: &str) -> Vec<String> {
    skills::scan(text)
        .into_iter()
        .map(|f| skills::get(f.def).name.to_string())
        .take(40)
        .collect()
}

/// A description with its sections and lists.
pub fn description(text: Option<String>, truncated: bool, full: bool) -> Description {
    let text = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let Some(body) = text else {
        return Description {
            text: None,
            sections: Vec::new(),
            requirements: Vec::new(),
            preferred: Vec::new(),
            skills: Vec::new(),
            benefits: Vec::new(),
            state: DescriptionState::Missing,
            lists: ListsStatus::NoDescription,
            truncated: false,
        };
    };
    let sections = sections(&body);
    let mut requirements = Vec::new();
    let mut preferred = Vec::new();
    let mut benefits = Vec::new();
    for section in &sections {
        match section.kind {
            SectionKind::Requirements => {
                for item in items(section) {
                    let lower = item.to_lowercase();
                    if has_any(&lower, PREFERRED_MARKERS) {
                        preferred.push(item);
                    } else {
                        requirements.push(item);
                    }
                }
            }
            SectionKind::Preferred => preferred.extend(items(section)),
            SectionKind::Benefits => benefits.extend(items(section)),
            _ => {}
        }
    }
    let lists = if sections.is_empty() {
        ListsStatus::NoSectionsFound
    } else {
        ListsStatus::Extracted
    };
    Description {
        skills: skill_names(&body),
        state: if full && !truncated {
            DescriptionState::Full
        } else {
            DescriptionState::Partial
        },
        text: Some(body),
        sections,
        requirements,
        preferred,
        benefits,
        lists,
        truncated,
    }
}

/// ISO 639-1 language of a text (English, German, Polish), or `None` when
/// the text is too short or unclear.
pub fn language(text: &str) -> Option<&'static str> {
    const EN: &[&str] = &[
        "the", "and", "you", "with", "for", "our", "we", "are", "will", "your", "of", "to",
    ];
    const DE: &[&str] = &[
        "und", "die", "der", "wir", "sie", "mit", "für", "ist", "das", "du", "ihre", "eine", "zu",
    ];
    const PL: &[&str] = &[
        "i", "w", "z", "na", "się", "oraz", "jest", "dla", "do", "nie", "będzie", "jako", "pracy",
    ];
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .take(600)
        .collect();
    if words.len() < 20 {
        return None;
    }
    let count = |list: &[&str]| words.iter().filter(|w| list.contains(w)).count();
    let mut scores = [
        ("en", count(EN)),
        (
            "de",
            count(DE) + lower.matches(['ä', 'ö', 'ü', 'ß']).count().min(10),
        ),
        (
            "pl",
            count(PL)
                + lower
                    .matches(['ą', 'ć', 'ę', 'ł', 'ń', 'ś', 'ź', 'ż'])
                    .count()
                    .min(10),
        ),
    ];
    scores.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let (best, top) = scores[0];
    let second = scores[1].1;
    (top >= 5 && top * 2 >= second * 3).then_some(best)
}

/// Lower-case role without gender markers and seniority words.
pub fn normalized_title(title: &str) -> Option<String> {
    let role = normalize::role(title).to_lowercase();
    let role = role.split_whitespace().collect::<Vec<_>>().join(" ");
    (!role.is_empty()).then_some(role)
}

pub fn seniority(title: &str) -> Option<Seniority> {
    normalize::seniority(title).map(|s| match s {
        am::Seniority::Intern => Seniority::Intern,
        am::Seniority::Entry => Seniority::Entry,
        am::Seniority::Mid => Seniority::Mid,
        am::Seniority::Senior => Seniority::Senior,
        am::Seniority::Lead => Seniority::Lead,
        am::Seniority::Executive => Seniority::Executive,
    })
}

pub fn work_mode(text: &str) -> Option<WorkMode> {
    normalize::work_mode(text).map(|m| match m {
        am::WorkMode::Remote => WorkMode::Remote,
        am::WorkMode::Hybrid => WorkMode::Hybrid,
        am::WorkMode::Onsite => WorkMode::Onsite,
    })
}

/// Contract type and working time from terms such as "FULL_TIME",
/// "Vollzeit, unbefristet", "pełny etat", "B2B".
pub fn employment(text: &str) -> (Option<EmploymentType>, Option<WorkingTime>) {
    let lower = text.to_lowercase().replace('_', "-");
    let working = if has_any(&lower, &["part-time", "part time", "teilzeit", "niepełny"]) {
        Some(WorkingTime::PartTime)
    } else if has_any(
        &lower,
        &[
            "full-time",
            "full time",
            "vollzeit",
            "pełny etat",
            "pełen etat",
        ],
    ) {
        Some(WorkingTime::FullTime)
    } else {
        None
    };
    let kind = if has_any(&lower, &["intern", "praktikum", "staż", "praktyk"]) {
        Some(EmploymentType::Internship)
    } else if has_any(&lower, &["apprentice", "lehre", "ausbildung"]) {
        Some(EmploymentType::Apprenticeship)
    } else if has_any(&lower, &["freelance", "freiberuf"]) {
        Some(EmploymentType::Freelance)
    } else if has_any(&lower, &["contractor", "contract", "b2b", "werkvertrag"]) {
        Some(EmploymentType::Contract)
    } else if has_any(
        &lower,
        &["temporary", "fixed-term", "befristet", "zlecenie"],
    ) && !lower.contains("unbefristet")
    {
        Some(EmploymentType::Temporary)
    } else if has_any(
        &lower,
        &[
            "permanent",
            "unbefristet",
            "festanstellung",
            "umowa o pracę",
        ],
    ) {
        Some(EmploymentType::Permanent)
    } else {
        None
    };
    (kind, working)
}

/// A location as stated, with the city and country ReMa recognizes.
pub fn location(text: &str) -> Location {
    let place = normalize::place(text);
    Location {
        text: clip(text, 160),
        city: place.city().map(str::to_string),
        region: None,
        country: place.country().map(str::to_string),
    }
}

// ── Salary ────────────────────────────────────────────────────────────

const ESTIMATE_MARKERS: &[&str] = &[
    "estimat",
    "approx",
    "circa",
    "geschätzt",
    "szacowan",
    "glassdoor",
    "levels.fyi",
    "~",
    "≈",
];
const VARIABLE_PAY: &[&str] = &[
    "bonus",
    "equity",
    "stock",
    "rsu",
    "commission",
    "provision",
    "prämie",
    "premia",
    "options",
];
const FLOOR_MARKERS: &[&str] = &[
    "mindestgehalt",
    "mindestens",
    "überzahlung",
    "overpayment",
    "kollektivvertrag",
    "at least",
    "starting",
    "from ",
    "ab ",
    "minimum",
];

/// Polish salary wording in terms `normalize::salary` understands.
fn polish(text: &str) -> String {
    let mut lower = format!(" {} ", text.to_lowercase());
    for (from, to) in [
        ("miesięcznie", " per month "),
        ("/mies.", " per month "),
        ("/mies", " per month "),
        (" mies.", " per month "),
        ("miesiąc", " month "),
        ("rocznie", " per year "),
        ("/godz", " per hour "),
        ("godzinę", " hour "),
    ] {
        lower = lower.replace(from, to);
    }
    if lower.contains(" od ") && lower.contains(" do ") {
        lower = lower.replace(" od ", " from ").replace(" do ", " to ");
    } else if lower.contains(" do ") {
        lower = lower.replace(" do ", " up to ");
    } else if lower.contains(" od ") {
        lower = lower.replace(" od ", " from ");
    }
    lower
}

fn basis(lower: &str) -> SalaryBasis {
    if has_any(lower, &["netto", " net ", "net)", "(net"]) {
        SalaryBasis::Net
    } else if has_any(lower, &["brutto", "gross"]) {
        SalaryBasis::Gross
    } else {
        SalaryBasis::Unknown
    }
}

/// A stated salary from text. Estimates are kept (flagged) without
/// amounts; periods are taken only when stated.
pub fn compensation_from_text(text: &str) -> Option<Compensation> {
    let original = clip(text, 240);
    let lower = polish(text);
    let variable_pay_mentioned = has_any(&lower, VARIABLE_PAY);
    if has_any(&lower, ESTIMATE_MARKERS) {
        return Some(Compensation {
            text: original,
            min: None,
            max: None,
            currency: normalize::currency(&lower).map(str::to_string),
            period: None,
            basis: basis(&lower),
            bound: None,
            estimate: true,
            variable_pay_mentioned,
        });
    }
    // "4.200,-" (no cents) and payment counts ("14x", "14 Gehälter") are
    // not amounts; the count is never multiplied in.
    static COUNTS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let counts = COUNTS.get_or_init(|| {
        regex::Regex::new(
            r"(?i)\b\d{1,2}\s?(?:x|×|mal)\b|\b1[2-4]\s+(?:monats)?(?:gehälter|salaries|payments|pensje)",
        )
        .expect("valid regex")
    });
    let lower = counts
        .replace_all(&lower.replace(",-", ""), " ")
        .into_owned();
    let parsed = normalize::salary(&lower)?;
    let (mut min, mut max) = (parsed.min, parsed.max);
    if min.is_some() && min == max && has_any(&lower, FLOOR_MARKERS) {
        max = None;
    }
    if let (Some(a), Some(b)) = (min, max) {
        if a > b {
            (min, max) = (Some(b), Some(a));
        }
    }
    let bound = match (min, max) {
        (Some(a), Some(b)) if (a - b).abs() < f64::EPSILON => SalaryBound::Exact,
        (Some(_), Some(_)) => SalaryBound::Range,
        (Some(_), None) => SalaryBound::Floor,
        (None, Some(_)) => SalaryBound::Ceiling,
        (None, None) => return None,
    };
    Some(Compensation {
        text: original,
        min,
        max,
        currency: parsed.currency.map(str::to_string),
        // Only a stated period: never inferred from the amount's size.
        period: normalize::period(&lower).map(period),
        basis: basis(&lower),
        bound: Some(bound),
        estimate: false,
        variable_pay_mentioned,
    })
}

pub fn period(p: am::SalaryPeriod) -> SalaryPeriod {
    match p {
        am::SalaryPeriod::Year => SalaryPeriod::Year,
        am::SalaryPeriod::Month => SalaryPeriod::Month,
        am::SalaryPeriod::Week => SalaryPeriod::Week,
        am::SalaryPeriod::Day => SalaryPeriod::Day,
        am::SalaryPeriod::Hour => SalaryPeriod::Hour,
    }
}

pub fn period_name(p: SalaryPeriod) -> &'static str {
    match p {
        SalaryPeriod::Year => "year",
        SalaryPeriod::Month => "month",
        SalaryPeriod::Week => "week",
        SalaryPeriod::Day => "day",
        SalaryPeriod::Hour => "hour",
    }
}

pub fn period_word(text: &str) -> Option<SalaryPeriod> {
    match text.trim().to_lowercase().as_str() {
        "year" | "yearly" | "annual" | "annually" | "per-year-salary" | "yr" => {
            Some(SalaryPeriod::Year)
        }
        "month" | "monthly" | "per-month-salary" | "mo" => Some(SalaryPeriod::Month),
        "week" | "weekly" => Some(SalaryPeriod::Week),
        "day" | "daily" => Some(SalaryPeriod::Day),
        "hour" | "hourly" | "per-hour-wage" => Some(SalaryPeriod::Hour),
        _ => None,
    }
}

/// A salary from structured fields (min/max/currency/period).
pub fn compensation_structured(
    min: Option<f64>,
    max: Option<f64>,
    currency: Option<&str>,
    period: Option<SalaryPeriod>,
    text: Option<&str>,
) -> Option<Compensation> {
    let min = min.filter(|v| *v > 0.0);
    let max = max.filter(|v| *v > 0.0);
    let bound = match (min, max) {
        (Some(a), Some(b)) if (a - b).abs() < f64::EPSILON => SalaryBound::Exact,
        (Some(_), Some(_)) => SalaryBound::Range,
        (Some(_), None) => SalaryBound::Floor,
        (None, Some(_)) => SalaryBound::Ceiling,
        (None, None) => return None,
    };
    let currency = currency
        .map(|c| c.trim().to_uppercase())
        .filter(|c| c.len() == 3);
    let rendered = {
        let fmt = |v: f64| {
            if v.fract() == 0.0 {
                format!("{}", v as i64)
            } else {
                format!("{v:.2}")
            }
        };
        let amount = match (min, max) {
            (Some(a), Some(b)) if bound == SalaryBound::Range => format!("{}–{}", fmt(a), fmt(b)),
            (Some(a), _) if bound == SalaryBound::Floor => format!("from {}", fmt(a)),
            (_, Some(b)) if bound == SalaryBound::Ceiling => format!("up to {}", fmt(b)),
            (Some(a), _) => fmt(a),
            _ => String::new(),
        };
        let per = period
            .map(|p| format!(" per {}", period_name(p)))
            .unwrap_or_default();
        format!(
            "{}{amount}{per}",
            currency
                .as_deref()
                .map(|c| format!("{c} "))
                .unwrap_or_default()
        )
    };
    let lower = text.unwrap_or_default().to_lowercase();
    Some(Compensation {
        text: text.map(|t| clip(t, 240)).unwrap_or(rendered),
        min,
        max,
        currency,
        period,
        basis: basis(&lower),
        bound: Some(bound),
        estimate: false,
        variable_pay_mentioned: has_any(&lower, VARIABLE_PAY),
    })
}

const SALARY_WORDS: &[&str] = &[
    "salary",
    "compensation",
    "pay ",
    "gehalt",
    "vergütung",
    "entgelt",
    "bezahlung",
    "brutto",
    "wynagrodzenie",
    "zarobki",
    "stawka",
];

/// A salary statement in a description (e.g. Austria's legally required
/// minimum salary), with the line it came from.
pub fn salary_from_description(text: &str) -> Option<Compensation> {
    for line in text.lines() {
        let lower = line.to_lowercase();
        if line.chars().count() > 400 || !line.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        if !has_any(&lower, SALARY_WORDS) {
            continue;
        }
        if normalize::currency(&polish(&lower)).is_none() {
            continue;
        }
        if let Some(found) = compensation_from_text(line) {
            return Some(found);
        }
    }
    None
}

// ── Availability ──────────────────────────────────────────────────────

/// Phrases a page shows once a posting is closed.
pub fn says_closed(text: &str) -> bool {
    const CLOSED: &[&str] = &[
        "no longer available",
        "no longer accepting applications",
        "this job has expired",
        "this posting has expired",
        "position has been filled",
        "job is closed",
        "not accepting applications",
        "nicht mehr verfügbar",
        "nicht mehr aktiv",
        "stelle ist bereits besetzt",
        "bereits vergeben",
        "oferta wygasła",
        "ogłoszenie jest nieaktualne",
        "rekrutacja zakończona",
    ];
    let lower = text.to_lowercase();
    CLOSED.iter().any(|phrase| lower.contains(phrase))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_multilingual_sections_and_keeps_preferred_apart() {
        let en = description(
            Some(
                "About us\nWe build LLM products.\nWhat you bring:\n- 3+ years of Python\n\
                 - Experience with PyTorch\n- Kubernetes is a plus\nNice to have\n- Rust\n\
                 What we offer\n- Remote days"
                    .into(),
            ),
            false,
            true,
        );
        assert_eq!(en.lists, ListsStatus::Extracted);
        assert_eq!(
            en.requirements,
            ["3+ years of Python", "Experience with PyTorch"]
        );
        assert_eq!(en.preferred, ["Kubernetes is a plus", "Rust"]);
        assert_eq!(en.benefits, ["Remote days"]);
        assert!(en.skills.contains(&"Python".to_string()));
        assert_eq!(en.state, DescriptionState::Full);

        let de = description(
            Some(
                "Deine Aufgaben\n- Modelle trainieren\nDein Profil\n- Erfahrung mit Python\n\
                 - Kenntnisse in Docker von Vorteil\nWas wir bieten\n- Gleitzeit"
                    .into(),
            ),
            false,
            true,
        );
        assert_eq!(de.requirements, ["Erfahrung mit Python"]);
        assert_eq!(de.preferred, ["Kenntnisse in Docker von Vorteil"]);
        assert_eq!(de.benefits, ["Gleitzeit"]);

        let pl = description(
            Some(
                "Zakres obowiązków\n- Budowa modeli\nWymagania\n- Python\nMile widziane\n- Rust\n\
                 Oferujemy\n- Umowa B2B"
                    .into(),
            ),
            false,
            true,
        );
        assert_eq!(pl.requirements, ["Python"]);
        assert_eq!(pl.preferred, ["Rust"]);
        assert_eq!(pl.benefits, ["Umowa B2B"]);

        let plain = description(Some("We build things. Join us.".into()), false, false);
        assert_eq!(plain.lists, ListsStatus::NoSectionsFound);
        assert_eq!(plain.state, DescriptionState::Partial);
        assert!(plain.requirements.is_empty());
        assert_eq!(
            description(None, false, false).lists,
            ListsStatus::NoDescription
        );
    }

    #[test]
    fn detects_the_posting_language() {
        assert_eq!(
            language(
                "We are looking for an engineer who will join our team and work with the \
                 platform. You will build models and you are responsible for the pipeline of \
                 our customers with your colleagues."
            ),
            Some("en")
        );
        assert_eq!(
            language(
                "Wir suchen eine Person, die mit uns an der Plattform arbeitet. Du bist für die \
                 Modelle zuständig und wir bieten dir eine spannende Aufgabe mit Zukunft und \
                 für deine Entwicklung ist gesorgt, das ist uns wichtig."
            ),
            Some("de")
        );
        assert_eq!(
            language(
                "Szukamy osoby, która będzie pracować w zespole i odpowiadać za rozwój modeli. \
                 Oferujemy pracę w firmie z doświadczeniem oraz możliwość rozwoju i dla nas jest \
                 to ważne, nie tylko na początku."
            ),
            Some("pl")
        );
        assert_eq!(language("Short text"), None);
    }

    #[test]
    fn reads_salaries_without_inventing_ranges_or_periods() {
        let floor = compensation_from_text(
            "Mindestgehalt € 4.000 brutto pro Monat, Bereitschaft zur Überzahlung",
        )
        .unwrap();
        assert_eq!(floor.min, Some(4000.0));
        assert_eq!(floor.max, None, "a floor is not a range");
        assert_eq!(floor.bound, Some(SalaryBound::Floor));
        assert_eq!(floor.period, Some(SalaryPeriod::Month));
        assert_eq!(floor.basis, SalaryBasis::Gross);
        assert_eq!(floor.currency.as_deref(), Some("EUR"));

        let range = compensation_from_text("€85,000 – €100,000 per year plus bonus").unwrap();
        assert_eq!((range.min, range.max), (Some(85_000.0), Some(100_000.0)));
        assert_eq!(range.bound, Some(SalaryBound::Range));
        assert!(range.variable_pay_mentioned);

        let no_period = compensation_from_text("EUR 70,000").unwrap();
        assert_eq!(no_period.period, None, "the period is not guessed");

        let polish =
            compensation_from_text("od 18 000 do 24 000 zł netto miesięcznie (B2B)").unwrap();
        assert_eq!((polish.min, polish.max), (Some(18_000.0), Some(24_000.0)));
        assert_eq!(polish.currency.as_deref(), Some("PLN"));
        assert_eq!(polish.period, Some(SalaryPeriod::Month));
        assert_eq!(polish.basis, SalaryBasis::Net);

        let estimate = compensation_from_text("Estimated €90k per year (Glassdoor)").unwrap();
        assert!(estimate.estimate && estimate.min.is_none() && estimate.bound.is_none());

        assert!(compensation_from_text("Competitive salary").is_none());
        let from_description = salary_from_description(
            "Deine Aufgaben\n- Modelle\nFür diese Position gilt ein Mindestgehalt von EUR 4.200,- \
             brutto pro Monat (14x jährlich).",
        )
        .unwrap();
        assert_eq!(from_description.min, Some(4200.0));
        assert_eq!(from_description.bound, Some(SalaryBound::Floor));
    }

    #[test]
    fn reads_employment_terms_and_dates() {
        assert_eq!(employment("FULL_TIME"), (None, Some(WorkingTime::FullTime)));
        assert_eq!(
            employment("Vollzeit, unbefristet"),
            (Some(EmploymentType::Permanent), Some(WorkingTime::FullTime))
        );
        assert_eq!(employment("B2B").0, Some(EmploymentType::Contract));
        assert_eq!(employment("Praktikum").0, Some(EmploymentType::Internship));
        assert_eq!(date("2026-09-20T10:00:00+02:00").unwrap().0, "2026-09-20");
        assert_eq!(date("1790380800000").unwrap().0, "2026-09-26");
        assert!(date("yesterday").is_none());
        assert!(says_closed("Diese Stelle ist bereits besetzt."));
    }
}
