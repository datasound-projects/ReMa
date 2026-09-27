//! Find Contract Work (B12–B14): published freelance, contract, interim,
//! B2B and project engagements through ReMa's job layer, with their terms
//! as the listing states them and strict, visible criteria:
//!
//! - "above EUR 700/day" excludes exactly 700; "at least" includes it.
//! - A range straddling the threshold, "up to …" and an unknown rate need
//!   verification; they are never confirmed matches.
//! - Hourly and daily rates are compared only with the user's own
//!   hours-per-day basis; other currencies and annual salaries never are.
//! - A duration fits a strict range only when the stated range lies
//!   within it; weeks are not silently turned into months.
//! - "Contract" in a title alone does not make a listing independent
//!   work; salaried fixed-term jobs are employment.

use std::{collections::HashMap, sync::OnceLock, time::Duration};

use futures_util::{stream, StreamExt};
use regex::Regex;
use tokio_util::sync::CancellationToken;

use super::{
    locations,
    model::{
        Comparison, ContractCriteria, ContractResult, ContractResults, ContractTerms, DurationUnit,
        Engagement, MatchStatus, RateUnit, RunStatus,
    },
    store,
    text::has_phrase,
};
use crate::{
    analytics::normalize,
    llm::Endpoint,
    rema_mcp::{
        contract::{EmploymentType, GetJobInput, JobRecord, SalaryPeriod, WorkMode},
        engine::{self, Discovery, Session},
        store as jobs_store,
    },
    retrieval::{self, JobQuery, Listing, Outcome, Progress, Verification},
    state::AppState,
    time::now_ms,
};

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// "1.200" → 1200, "95,50" → 95.5, "750" → 750.
pub fn amount(text: &str) -> Option<f64> {
    let t = text.trim();
    static THOUSANDS: OnceLock<Regex> = OnceLock::new();
    if re(&THOUSANDS, r"^\d{1,3}([.,']\d{3})+$").is_match(t) {
        return t.replace(['.', ',', '\''], "").parse().ok();
    }
    t.replace(',', ".").parse().ok()
}

fn currency_of(text: &str) -> Option<String> {
    Some(
        match text.trim().to_lowercase().as_str() {
            "€" | "eur" | "euro" | "euros" => "EUR",
            "$" | "usd" => "USD",
            "chf" | "fr." => "CHF",
            "£" | "gbp" => "GBP",
            _ => return None,
        }
        .to_string(),
    )
}

fn unit_of(text: &str) -> Option<RateUnit> {
    let t = text.trim().to_lowercase();
    Some(match t.as_str() {
        "day" | "tag" | "d" | "daily" | "tagessatz" => RateUnit::Day,
        "hour" | "h" | "std" | "stunde" | "hr" | "hourly" | "stundensatz" => RateUnit::Hour,
        "month" | "monat" | "mo" => RateUnit::Month,
        "project" | "projekt" => RateUnit::Project,
        _ => return None,
    })
}

// ── The request ──────────────────────────────────────────────────────

/// Contract criteria from a request's words (the page's fields win).
pub fn parse_criteria(text: &str) -> (ContractCriteria, Vec<String>) {
    static RATE: OnceLock<Regex> = OnceLock::new();
    static RANGE: OnceLock<Regex> = OnceLock::new();
    static AT_LEAST: OnceLock<Regex> = OnceLock::new();
    static UP_TO: OnceLock<Regex> = OnceLock::new();
    static HOURS: OnceLock<Regex> = OnceLock::new();
    let mut c = ContractCriteria::default();
    let (places, notes) = locations::parse(text);
    c.locations = places;
    if let Some(q) = retrieval::detect(text) {
        c.posted_within_days = q.posted_within_days;
        if let Some(role) = q.role {
            c.skills = role;
        }
    }
    if c.skills.is_empty() {
        static SKILLS: OnceLock<Regex> = OnceLock::new();
        if let Some(caps) = re(
            &SKILLS,
            r"(?i)\b(?:find|search|show|look for)\s+(?:me\s+)?(?:some\s+)?(?:freelance\s+|remote\s+)?(?P<s>[\w+#/ .-]{2,40}?)\s+(?:freelance\s+)?(?:contracts?|projects?|gigs?|engagements?|assignments?)\b",
        )
        .captures(text)
        {
            c.skills = caps["s"].trim().to_string();
        }
    }
    if let Some(caps) = re(
        &RATE,
        r"(?i)\b(?P<cmp>above|over|more than|greater than|at least|minimum(?: of)?|min\.?|from|ab)\s*(?P<c1>€|eur|usd|\$|chf|£|gbp)?\s*(?P<a>\d[\d.,']*)\s*(?P<c2>€|eur|euro|usd|\$|chf|£|gbp)?\s*(?:/|per|a|pro)\s*(?P<u>day|tag|hour|h|stunde|month|monat)\b|(?P<sym>>=|≥|>)\s*(?P<c3>€|eur|usd|\$|chf|£|gbp)?\s*(?P<b>\d[\d.,']*)\s*(?P<c4>€|eur|usd|\$|chf|£|gbp)?\s*(?:/|per)\s*(?P<u2>day|tag|hour|h|month)\b",
    )
    .captures(text)
    {
        let (value, cmp, cur, unit) = match caps.name("a") {
            Some(a) => (
                a.as_str(),
                caps.name("cmp").map_or("", |m| m.as_str()).to_lowercase(),
                caps.name("c1").or(caps.name("c2")).map(|m| m.as_str()),
                caps.name("u").map_or("", |m| m.as_str()),
            ),
            None => (
                caps.name("b").map_or("", |m| m.as_str()),
                caps.name("sym").map_or("", |m| m.as_str()).to_string(),
                caps.name("c3").or(caps.name("c4")).map(|m| m.as_str()),
                caps.name("u2").map_or("", |m| m.as_str()),
            ),
        };
        c.min_rate = amount(value);
        c.rate_comparison = if matches!(
            cmp.as_str(),
            "above" | "over" | "more than" | "greater than" | ">"
        ) {
            Comparison::Above
        } else {
            Comparison::AtLeast
        };
        c.currency = cur.and_then(currency_of);
        c.rate_unit = unit_of(unit).unwrap_or(RateUnit::Day);
    }
    if let Some(caps) = re(
        &RANGE,
        r"(?i)\b(\d{1,2})\s*(?:-|–|to|bis)\s*(\d{1,2})\s*(?:months?|monate?n?)\b",
    )
    .captures(text)
    {
        c.duration_min_months = caps[1].parse().ok();
        c.duration_max_months = caps[2].parse().ok();
    } else {
        if let Some(caps) = re(
            &AT_LEAST,
            r"(?i)\b(?:at least|minimum(?: of)?|min\.?|mindestens)\s*(\d{1,2})\s*(?:months?|monate?n?)\b",
        )
        .captures(text)
        {
            c.duration_min_months = caps[1].parse().ok();
        }
        if let Some(caps) = re(
            &UP_TO,
            r"(?i)\b(?:up to|at most|maximum(?: of)?|max\.?|höchstens)\s*(\d{1,2})\s*(?:months?|monate?n?)\b",
        )
        .captures(text)
        {
            c.duration_max_months = caps[1].parse().ok();
        }
    }
    if let Some(caps) = re(
        &HOURS,
        r"(?i)\b(\d{1,2}(?:[.,]\d)?)\s*(?:hours?|h|stunden)\s*(?:per|a|/|pro)\s*(?:day|tag)\b",
    )
    .captures(text)
    {
        c.hours_per_day = amount(&caps[1]);
    }
    c.remote_ok = !re(&HOURS_ONSITE, r"(?i)\b(on-?site only|no remote|vor ort)\b").is_match(text);
    (c, notes)
}

static HOURS_ONSITE: OnceLock<Regex> = OnceLock::new();

/// The criteria as ReMa applies them (B13: visible and strict).
pub fn normalized(c: &ContractCriteria) -> Vec<String> {
    let mut out = vec!["Engagement: freelance, contract, B2B, interim or project work".to_string()];
    if !c.skills.trim().is_empty() {
        out.push(format!("Skills: {}", c.skills.trim()));
    }
    out.push(format!(
        "Where: {}{}",
        locations::label(&c.locations),
        if c.remote_ok {
            ", or remote work for there"
        } else {
            ""
        }
    ));
    if let Some(rate) = c.min_rate {
        out.push(format!(
            "Rate: {} {} {} per {}{}",
            match c.rate_comparison {
                Comparison::Above => "strictly above",
                Comparison::AtLeast => "at least",
            },
            c.currency.as_deref().unwrap_or("(any currency)"),
            trim_amount(rate),
            unit_label(c.rate_unit),
            match c.hours_per_day {
                Some(h) => format!(
                    " (hourly rates compared at your {} hours per day)",
                    trim_amount(h)
                ),
                None => " (hourly rates are not converted without your hours-per-day basis)".into(),
            }
        ));
    }
    match (c.duration_min_months, c.duration_max_months) {
        (Some(a), Some(b)) => out.push(format!(
            "Duration: within {}–{} months (a partly overlapping range needs verification)",
            trim_amount(a),
            trim_amount(b)
        )),
        (Some(a), None) => out.push(format!("Duration: at least {} months", trim_amount(a))),
        (None, Some(b)) => out.push(format!("Duration: at most {} months", trim_amount(b))),
        (None, None) => {}
    }
    if let Some(days) = c.posted_within_days {
        out.push(format!("Posted within the last {days} days"));
    }
    out
}

fn trim_amount(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{x:.0}")
    } else {
        format!("{x}")
    }
}

fn unit_label(u: RateUnit) -> &'static str {
    match u {
        RateUnit::Hour => "hour",
        RateUnit::Day => "day",
        RateUnit::Month => "month",
        RateUnit::Project => "project",
    }
}

// ── A listing's terms ────────────────────────────────────────────────

const AGENCIES: &[&str] = &[
    "Hays",
    "GULP",
    "Randstad",
    "Etengo",
    "Computer Futures",
    "SThree",
    "Harvey Nash",
    "Michael Page",
    "Robert Half",
    "Solcom",
    "Westhouse",
    "Darwin Recruitment",
    "Austin Fraser",
    "Nigel Frank",
    "Jefferson Frank",
    "Progressive Recruitment",
    "Allgeier",
    "Modis",
    "Akkodis",
    "Experis",
    "Adecco",
    "Brunel",
    "Amoria Bond",
];

/// The engagement a listing states (B12).
pub fn engagement(
    title: &str,
    text: &str,
    record: Option<EmploymentType>,
    day_or_hour_rate: bool,
) -> Engagement {
    let t = format!("{title}\n{text}").to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| t.contains(w));
    let b2b = re(
        &B2B,
        r"(?i)\bb2b\s*(contract|basis|cooperation|umowa|kontrakt)|\bon a b2b\b|\bvia b2b\b",
    )
    .is_match(&t);
    let interim = any(&["interim"]);
    let freelance = any(&[
        "freelance",
        "freelancer",
        "freiberuf",
        "self-employed",
        "selbstständig",
        "selbststandig",
    ]) || record == Some(EmploymentType::Freelance);
    let contractor = any(&[
        "independent contractor",
        "contractor",
        "outside ir35",
        "inside ir35",
        "day rate",
        "daily rate",
        "tagessatz",
        "stundensatz",
        "hourly rate",
        "werkvertrag",
        "dienstvertrag",
    ]) || day_or_hour_rate;
    let project = any(&[
        "project-based",
        "project basis",
        "projektbasis",
        "projektarbeit",
        "projekteinsatz",
        "project assignment",
    ]);
    let fixed_term = any(&[
        "fixed-term",
        "fixed term",
        "befristet",
        "temporary employment",
        "maternity cover",
        "parental leave cover",
        "elternzeitvertretung",
        "karenzvertretung",
    ]) || record == Some(EmploymentType::Temporary);
    let permanent = any(&[
        "permanent",
        "unbefristet",
        "festanstellung",
        "full-time employment",
    ]) || record == Some(EmploymentType::Permanent);
    let salaried = any(&[
        "per annum",
        " p.a.",
        "annual salary",
        "jahresgehalt",
        "brutto/jahr",
        "gross per year",
        "salary",
        "gehalt",
        "benefits package",
        "pension",
        "altersvorsorge",
        "vacation days",
        "urlaubstage",
        "employment contract",
        "anstellung",
    ]);
    let independent = b2b || interim || freelance || contractor || project;
    let employed = fixed_term || permanent || salaried;
    match (independent, employed) {
        (true, false) => {
            if b2b {
                Engagement::B2b
            } else if interim {
                Engagement::Interim
            } else if freelance {
                Engagement::Freelance
            } else if project {
                Engagement::Project
            } else {
                Engagement::Contract
            }
        }
        (false, true) => {
            if fixed_term {
                Engagement::FixedTermEmployee
            } else if permanent {
                Engagement::PermanentEmployee
            } else {
                Engagement::Employment
            }
        }
        _ => Engagement::NeedsVerification,
    }
}

static B2B: OnceLock<Regex> = OnceLock::new();

/// The rate a listing states: (min, max, is a ceiling, currency, unit,
/// as written).
type Rate = (
    Option<f64>,
    Option<f64>,
    bool,
    Option<String>,
    Option<RateUnit>,
    Option<String>,
);

pub fn rate(text: &str) -> Rate {
    static AMOUNT_UNIT: OnceLock<Regex> = OnceLock::new();
    static KEYWORD: OnceLock<Regex> = OnceLock::new();
    let amount_unit = re(
        &AMOUNT_UNIT,
        r"(?i)(?P<pre>up to|bis zu|max(?:imal|imum)?\.?|from|ab|starting at)?\s*(?P<c1>€|eur|usd|\$|chf|£|gbp)?\s*(?P<a>\d{2,5}(?:[.,']\d{3})*(?:[.,]\d{1,2})?)(?:\s*(?:-|–|to|bis)\s*(?P<c2>€|eur|usd|\$|chf|£|gbp)?\s*(?P<b>\d{2,5}(?:[.,']\d{3})*(?:[.,]\d{1,2})?))?\s*(?P<c3>€|eur|euro|usd|\$|chf|£|gbp)?\s*(?:/|per|pro|an|a)\s*(?P<u>day|tag|hour|h\b|std|stunde|month|monat|project|projekt)",
    );
    let keyword = re(
        &KEYWORD,
        r"(?i)(?P<k>tagessatz|daily rate|day rate|stundensatz|hourly rate)\s*(?:of|von|:)?\s*(?P<pre>up to|bis zu|max\.?|from|ab)?\s*(?P<c1>€|eur|usd|\$|chf|£|gbp)?\s*(?P<a>\d{2,5}(?:[.,']\d{3})*(?:[.,]\d{1,2})?)(?:\s*(?:-|–|to|bis)\s*(?P<b>\d{2,5}(?:[.,']\d{3})*(?:[.,]\d{1,2})?))?\s*(?P<c3>€|eur|euro|usd|\$|chf|£|gbp)?",
    );
    let pick = |caps: &regex::Captures, unit: Option<RateUnit>| -> Rate {
        let pre = caps
            .name("pre")
            .map(|m| m.as_str().to_lowercase())
            .unwrap_or_default();
        let a = caps.name("a").and_then(|m| amount(m.as_str()));
        let b = caps.name("b").and_then(|m| amount(m.as_str()));
        let currency = ["c1", "c2", "c3"]
            .iter()
            .find_map(|k| caps.name(k).and_then(|m| currency_of(m.as_str())));
        let ceiling = pre.starts_with("up to") || pre.starts_with("bis") || pre.starts_with("max");
        let (min, max) = match (a, b) {
            (Some(a), Some(b)) => (Some(a.min(b)), Some(a.max(b))),
            (Some(a), None) if ceiling => (None, Some(a)),
            (Some(a), None) if pre == "from" || pre == "ab" || pre == "starting at" => {
                (Some(a), None)
            }
            (Some(a), None) => (Some(a), Some(a)),
            _ => (None, None),
        };
        (
            min,
            max,
            ceiling,
            currency,
            unit,
            caps.get(0).map(|m| normalize::clip(m.as_str().trim(), 80)),
        )
    };
    if let Some(caps) = keyword.captures(text) {
        let unit = match caps["k"].to_lowercase().as_str() {
            "stundensatz" | "hourly rate" => RateUnit::Hour,
            _ => RateUnit::Day,
        };
        return pick(&caps, Some(unit));
    }
    if let Some(caps) = amount_unit.captures(text) {
        let unit = caps.name("u").and_then(|m| unit_of(m.as_str()));
        return pick(&caps, unit);
    }
    (None, None, false, None, None, None)
}

/// Duration a listing states: (min, max, unit, as written).
pub fn duration(
    text: &str,
) -> (
    Option<f64>,
    Option<f64>,
    Option<DurationUnit>,
    Option<String>,
) {
    static RANGE: OnceLock<Regex> = OnceLock::new();
    static SINGLE: OnceLock<Regex> = OnceLock::new();
    let unit = |u: &str| {
        let u = u.to_lowercase();
        if u.starts_with("week") || u.starts_with("woche") {
            DurationUnit::Week
        } else {
            DurationUnit::Month
        }
    };
    if let Some(caps) = re(
        &RANGE,
        r"(?i)\b(\d{1,2})\s*(?:-|–|to|bis)\s*(\d{1,2})\s*(months?|monate?n?|weeks?|wochen)\b",
    )
    .captures(text)
    {
        return (
            caps[1].parse().ok(),
            caps[2].parse().ok(),
            Some(unit(&caps[3])),
            Some(caps[0].to_string()),
        );
    }
    if let Some(caps) = re(
        &SINGLE,
        r"(?i)\b(?P<n>\d{1,2})\s*(?P<plus>\+)?\s*(?P<u>months?|monate?n?|weeks?|wochen)\b(?P<more>\s*(?:or longer|\+|und länger|plus))?",
    )
    .captures(text)
    {
        let n: Option<f64> = caps["n"].parse().ok();
        let open = caps.name("plus").is_some() || caps.name("more").is_some();
        return (
            n,
            if open { None } else { n },
            Some(unit(&caps["u"])),
            Some(caps[0].trim().to_string()),
        );
    }
    (None, None, None, None)
}

fn start(text: &str) -> Option<String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let lower = text.to_lowercase();
    if ["asap", "ab sofort", "immediately", "immediate start"]
        .iter()
        .any(|w| lower.contains(w))
    {
        return Some("As soon as possible".into());
    }
    re(
        &CELL,
        r"(?i)\b(?:start(?:ing)?(?: date)?|beginn|projektstart|ab)\s*:?\s*(?:on\s+|am\s+)?(?P<d>\d{1,2}\.\d{1,2}\.\d{2,4}|\d{4}-\d{2}-\d{2}|(?:january|february|march|april|may|june|july|august|september|october|november|december|jänner|januar|februar|märz|mai|juni|juli|oktober|dezember)\s+\d{4})",
    )
    .captures(text)
    .map(|c| c["d"].to_string())
}

fn workload(text: &str) -> Option<String> {
    static PERCENT: OnceLock<Regex> = OnceLock::new();
    static DAYS: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = re(
        &PERCENT,
        r"(?i)\b(\d{2,3})\s?%\s*(?:workload|auslastung|capacity|fte|pensum)|(?:workload|auslastung|pensum)\s*:?\s*(\d{2,3})\s?%",
    )
    .captures(text)
    {
        let n = c.get(1).or(c.get(2)).map_or("", |m| m.as_str());
        return Some(format!("{n}% workload"));
    }
    if let Some(c) = re(
        &DAYS,
        r"(?i)\b([1-5])\s*(?:days?|tage)\s*(?:/|per|a|pro)\s*(?:week|woche)",
    )
    .captures(text)
    {
        return Some(format!("{} days per week", &c[1]));
    }
    let lower = text.to_lowercase();
    if lower.contains("full-time") || lower.contains("vollzeit") {
        return Some("Full-time".into());
    }
    if lower.contains("part-time") || lower.contains("teilzeit") {
        return Some("Part-time".into());
    }
    None
}

fn eligibility(text: &str, record: Option<&JobRecord>) -> Vec<String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let mut out: Vec<String> = record
        .map(|r| r.remote_eligibility.clone())
        .unwrap_or_default();
    for caps in re(
        &CELL,
        r"(?i)\b(?:(?P<a>eu|europe|germany|austria|switzerland|dach|uk|us|cet[^.;\n]{0,12})\s+only|(?:must be|need to be|to be)\s+based in\s+(?P<b>[A-Za-zäöüÄÖÜ ,]{2,40})|(?:remote|work)\s+from\s+(?P<c>[A-Za-zäöüÄÖÜ ,]{2,40}?)\s+only)\b",
    )
    .captures_iter(text)
    {
        let found = ["a", "b", "c"]
            .iter()
            .find_map(|k| caps.name(k))
            .map(|m| m.as_str().trim().to_string());
        if let Some(f) = found {
            let entry = format!("{f} only");
            if !out.iter().any(|e| e.eq_ignore_ascii_case(&entry)) {
                out.push(entry);
            }
        }
    }
    out
}

fn languages(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    for (needles, label) in [
        (&["german", "deutsch"][..], "German"),
        (&["english", "englisch"][..], "English"),
        (&["french", "französisch"][..], "French"),
        (&["italian", "italienisch"][..], "Italian"),
    ] {
        if needles.iter().any(|n| {
            lower.contains(&format!("fluent {n}"))
                || lower.contains(&format!("{n} (c1"))
                || lower.contains(&format!("{n} (c2"))
                || lower.contains(&format!("{n} c1"))
                || lower.contains(&format!("{n} language"))
                || lower.contains(&format!("{n}kenntnisse"))
                || lower.contains(&format!("sehr gute {n}"))
                || lower.contains(&format!("fließend {n}"))
        }) {
            out.push(label.to_string());
        }
    }
    out
}

/// A listing's commercial terms as stated (unknown stays empty).
pub fn terms(listing: &Listing, record: Option<&JobRecord>) -> ContractTerms {
    let description = record
        .and_then(|r| r.description.text.clone())
        .or_else(|| listing.summary.clone())
        .unwrap_or_default();
    let compensation = record
        .and_then(|r| r.compensation.as_ref())
        .map(|c| c.text.clone())
        .or_else(|| listing.salary.clone())
        .unwrap_or_default();
    let text = format!("{}\n{compensation}\n{description}", listing.title);
    let (mut min, mut max, mut ceiling, mut currency, mut unit, mut written) = rate(&text);
    // Structured day or hour rates from the posting's data.
    if min.is_none() && max.is_none() {
        if let Some(c) = record.and_then(|r| r.compensation.as_ref()) {
            let structured = match c.period {
                Some(SalaryPeriod::Day) => Some(RateUnit::Day),
                Some(SalaryPeriod::Hour) => Some(RateUnit::Hour),
                _ => None,
            };
            if structured.is_some() && !c.estimate {
                min = c.min;
                max = c.max.or(c.min);
                ceiling = c.min.is_none() && c.max.is_some();
                currency.clone_from(&c.currency);
                unit = structured;
                written = Some(c.text.clone());
            }
        }
    }
    let day_or_hour =
        matches!(unit, Some(RateUnit::Day | RateUnit::Hour)) && (min.is_some() || max.is_some());
    let (dmin, dmax, dunit, dtext) = duration(&text);
    let lower = text.to_lowercase();
    let poster = listing
        .company
        .clone()
        .or_else(|| record.and_then(|r| r.employer.name.clone()));
    let agency_text = [
        "on behalf of our client",
        "for our client",
        "für unseren kunden",
        "unser kunde",
        "our client, a",
        "im auftrag unseres kunden",
    ]
    .iter()
    .any(|w| lower.contains(w));
    let known_agency = poster
        .as_deref()
        .is_some_and(|p| AGENCIES.iter().any(|a| has_phrase(p, a)));
    let (agency, end_client) = if agency_text || known_agency {
        static CLIENT: OnceLock<Regex> = OnceLock::new();
        let named = re(
            &CLIENT,
            r"(?i)\b(?:end client|endkunde|client)\s*:\s*(?P<c>[A-Z][\w&.\- ]{1,50})",
        )
        .captures(&text)
        .map(|c| c["c"].trim().to_string());
        (poster.clone(), named)
    } else {
        (None, poster.clone())
    };
    let work_mode = record
        .and_then(|r| r.work_mode)
        .map(|m| match m {
            WorkMode::Remote => "Remote".to_string(),
            WorkMode::Hybrid => "Hybrid".to_string(),
            WorkMode::Onsite => "On-site".to_string(),
        })
        .or_else(|| listing.work_mode.clone());
    let skills = record
        .map(|r| r.description.skills.clone())
        .unwrap_or_default();
    ContractTerms {
        engagement: engagement(
            &listing.title,
            &format!("{compensation}\n{description}"),
            record.and_then(|r| r.employment_type),
            day_or_hour,
        ),
        rate_min: min,
        rate_max: max,
        rate_is_ceiling: ceiling,
        currency,
        rate_unit: unit,
        rate_text: written,
        duration_min: dmin,
        duration_max: dmax,
        duration_unit: dunit,
        duration_text: dtext,
        extension_possible: [
            "extension possible",
            "option to extend",
            "verlängerung möglich",
            "verlängerungsoption",
            "likely to extend",
        ]
        .iter()
        .any(|w| lower.contains(w))
        .then_some(true),
        start: start(&text),
        workload: workload(&text),
        work_mode,
        eligibility: eligibility(&text, record),
        agency,
        end_client,
        skills,
        languages: languages(&text),
    }
}

// ── Strict matching ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Verdict {
    Pass,
    Verify,
    Fail,
}

fn rate_verdict(c: &ContractCriteria, t: &ContractTerms) -> (Verdict, Option<String>) {
    let Some(threshold) = c.min_rate else {
        return (Verdict::Pass, None);
    };
    let above = c.rate_comparison == Comparison::Above;
    let wanted = format!(
        "{} {} {}/{}",
        if above { "above" } else { "at least" },
        c.currency.as_deref().unwrap_or(""),
        trim_amount(threshold),
        unit_label(c.rate_unit)
    );
    if t.rate_min.is_none() && t.rate_max.is_none() {
        return (
            Verdict::Verify,
            Some("Rate not stated — unknown is not a match.".into()),
        );
    }
    if let (Some(want), Some(have)) = (&c.currency, &t.currency) {
        if want != have {
            return (
                Verdict::Verify,
                Some(format!(
                    "Quoted in {have}; without a sourced exchange rate ReMa does not convert it."
                )),
            );
        }
    }
    if c.currency.is_some() && t.currency.is_none() {
        return (Verdict::Verify, Some("Currency not stated.".into()));
    }
    let Some(unit) = t.rate_unit else {
        return (Verdict::Verify, Some("Rate unit not stated.".into()));
    };
    let factor = match (unit, c.rate_unit) {
        (a, b) if a == b => Some(1.0),
        (RateUnit::Hour, RateUnit::Day) => c.hours_per_day,
        (RateUnit::Day, RateUnit::Hour) => c.hours_per_day.map(|h| 1.0 / h),
        _ => None,
    };
    let Some(factor) = factor else {
        return (
            Verdict::Verify,
            Some(match (unit, c.rate_unit) {
                (RateUnit::Hour, RateUnit::Day) | (RateUnit::Day, RateUnit::Hour) => {
                    "An hourly and a daily rate are compared only with your hours-per-day basis."
                        .into()
                }
                _ => format!(
                    "Quoted per {}; not comparable with a rate per {}.",
                    unit_label(unit),
                    unit_label(c.rate_unit)
                ),
            }),
        );
    };
    let lo = t.rate_min.map(|x| x * factor);
    let hi = t.rate_max.map(|x| x * factor);
    let converted = if factor != 1.0 {
        format!(
            " (converted at your {} hours per day)",
            trim_amount(c.hours_per_day.unwrap_or(0.0))
        )
    } else {
        String::new()
    };
    let passes = |x: f64| if above { x > threshold } else { x >= threshold };
    if t.rate_is_ceiling || lo.is_none() {
        let ceiling = hi.unwrap_or(0.0);
        return if !passes(ceiling) {
            (
                Verdict::Fail,
                Some(format!(
                    "At most {}{converted}; {wanted} asked for.",
                    trim_amount(ceiling)
                )),
            )
        } else {
            (
                Verdict::Verify,
                Some(format!(
                    "Only a maximum ({}) is stated{converted}; it does not prove a rate {wanted}.",
                    trim_amount(ceiling)
                )),
            )
        };
    }
    let lo = lo.unwrap_or(0.0);
    let hi = hi.unwrap_or(f64::INFINITY);
    if passes(lo) {
        (Verdict::Pass, None)
    } else if !passes(hi) {
        (
            Verdict::Fail,
            Some(format!(
                "{}{converted} is not {wanted}.",
                if (hi - lo).abs() < f64::EPSILON {
                    trim_amount(lo)
                } else {
                    format!("{}–{}", trim_amount(lo), trim_amount(hi))
                }
            )),
        )
    } else {
        (
            Verdict::Verify,
            Some(format!(
                "The range {}–{}{converted} straddles {wanted}: possibly negotiable, not \
                 confirmed.",
                trim_amount(lo),
                if hi.is_finite() {
                    trim_amount(hi)
                } else {
                    "open".into()
                }
            )),
        )
    }
}

fn duration_verdict(c: &ContractCriteria, t: &ContractTerms) -> (Verdict, Option<String>) {
    if c.duration_min_months.is_none() && c.duration_max_months.is_none() {
        return (Verdict::Pass, None);
    }
    let min = c.duration_min_months.unwrap_or(0.0);
    let max = c.duration_max_months.unwrap_or(f64::INFINITY);
    match (t.duration_unit, t.duration_min, t.duration_max) {
        (None, _, _) | (_, None, None) => (Verdict::Verify, Some("Duration not stated.".into())),
        (Some(DurationUnit::Week), _, _) => (
            Verdict::Verify,
            Some(format!(
                "Stated in weeks ({}); ReMa does not convert weeks into months.",
                t.duration_text.clone().unwrap_or_default()
            )),
        ),
        (Some(DurationUnit::Month), a, b) => {
            let a = a.or(b).unwrap_or(0.0);
            match b {
                Some(b) if a >= min && b <= max => (Verdict::Pass, None),
                Some(b) if b < min || a > max => (
                    Verdict::Fail,
                    Some(format!(
                        "{} is outside the requested duration.",
                        t.duration_text.clone().unwrap_or_default()
                    )),
                ),
                None if a > max => (
                    Verdict::Fail,
                    Some(format!(
                        "{} is longer than requested.",
                        t.duration_text.clone().unwrap_or_default()
                    )),
                ),
                _ => (
                    Verdict::Verify,
                    Some(format!(
                        "{} only partly overlaps the requested duration.",
                        t.duration_text.clone().unwrap_or_default()
                    )),
                ),
            }
        }
    }
}

fn place_verdict(
    c: &ContractCriteria,
    listing: &Listing,
    record: Option<&JobRecord>,
    t: &ContractTerms,
) -> (Verdict, Option<String>) {
    if locations::is_empty(&c.locations) {
        return (Verdict::Pass, None);
    }
    let mut places: Vec<String> = record
        .map(|r| r.locations.iter().map(|l| l.text.clone()).collect())
        .unwrap_or_default();
    if places.is_empty() {
        places.extend(listing.location.iter().cloned());
    }
    let results: Vec<Option<bool>> = places
        .iter()
        .map(|p| locations::matches(&c.locations, p))
        .collect();
    if results.contains(&Some(true)) {
        return (Verdict::Pass, None);
    }
    let remote = t.work_mode.as_deref() == Some("Remote")
        || places.iter().any(|p| p.to_lowercase().contains("remote"));
    if remote && c.remote_ok {
        let eligible: Vec<Option<bool>> = t
            .eligibility
            .iter()
            .map(|e| locations::matches(&c.locations, e.trim_end_matches(" only")))
            .collect();
        if eligible.contains(&Some(true)) {
            return (Verdict::Pass, None);
        }
        if !eligible.is_empty() && eligible.iter().all(|e| *e == Some(false)) {
            return (
                Verdict::Fail,
                Some(format!("Remote only for {}.", t.eligibility.join(", "))),
            );
        }
        return (
            Verdict::Verify,
            Some(if t.eligibility.is_empty() {
                "Remote, but where it may be done from is not stated.".to_string()
            } else {
                format!(
                    "Remote for {}; whether that includes {} is not stated.",
                    t.eligibility.join(", "),
                    locations::label(&c.locations)
                )
            }),
        );
    }
    if !results.is_empty() && results.iter().all(|r| *r == Some(false)) {
        (
            Verdict::Fail,
            Some(format!("Located in {}.", places.join(", "))),
        )
    } else {
        (Verdict::Verify, Some("Location not stated.".into()))
    }
}

fn skills_verdict(
    c: &ContractCriteria,
    listing: &Listing,
    t: &ContractTerms,
    description: &str,
) -> (Verdict, Option<String>) {
    let wanted: Vec<String> = c
        .skills
        .split(|ch: char| ch == '/' || ch == ',' || ch.is_whitespace())
        .map(str::trim)
        .filter(|s| s.chars().count() >= 2)
        .filter(|s| !["and", "or", "und", "oder", "the"].contains(&s.to_lowercase().as_str()))
        .map(str::to_string)
        .collect();
    if wanted.is_empty() {
        return (Verdict::Pass, None);
    }
    let text = format!(
        "{}\n{}\n{}",
        listing.title,
        t.skills.join(", "),
        description
    );
    if wanted.iter().any(|w| has_phrase(&text, w)) {
        (Verdict::Pass, None)
    } else {
        (
            Verdict::Fail,
            Some(format!(
                "None of {} appear in the listing.",
                wanted.join(", ")
            )),
        )
    }
}

/// Checks a listing against the criteria: confirmed only when every
/// criterion is confirmed.
pub fn judge(
    c: &ContractCriteria,
    listing: &Listing,
    record: Option<&JobRecord>,
    t: &ContractTerms,
    now: i64,
) -> (MatchStatus, Vec<String>) {
    let description = record
        .and_then(|r| r.description.text.clone())
        .or_else(|| listing.summary.clone())
        .unwrap_or_default();
    let engagement = if t.engagement.independent() {
        (Verdict::Pass, None)
    } else if t.engagement == Engagement::NeedsVerification {
        (
            Verdict::Verify,
            Some(
                "Engagement type needs verification (\"contract\" in a title alone is not \
                 enough)."
                    .into(),
            ),
        )
    } else {
        (
            Verdict::Fail,
            Some(format!(
                "{}: employment, not independent contract work.",
                t.engagement.label()
            )),
        )
    };
    let fresh = match (c.posted_within_days, listing.posted) {
        (None, _) => (Verdict::Pass, None),
        (Some(days), Some(at)) if now - at <= i64::from(days) * 86_400_000 + 86_400_000 => {
            (Verdict::Pass, None)
        }
        (Some(days), Some(_)) => (
            Verdict::Fail,
            Some(format!("Posted more than {days} days ago.")),
        ),
        (Some(_), None) => (Verdict::Verify, Some("Posting date not stated.".into())),
    };
    let verdicts = [
        engagement,
        skills_verdict(c, listing, t, &description),
        place_verdict(c, listing, record, t),
        rate_verdict(c, t),
        duration_verdict(c, t),
        fresh,
    ];
    let worst = verdicts
        .iter()
        .map(|(v, _)| *v)
        .max()
        .unwrap_or(Verdict::Pass);
    let reasons: Vec<String> = verdicts
        .into_iter()
        .filter(|(v, _)| *v != Verdict::Pass)
        .filter_map(|(_, r)| r)
        .collect();
    (
        match worst {
            Verdict::Pass => MatchStatus::Confirmed,
            Verdict::Verify => MatchStatus::NeedsVerification,
            Verdict::Fail => MatchStatus::NotMatching,
        },
        reasons,
    )
}

// ── The search ───────────────────────────────────────────────────────

/// The skills of a request, one search each ("Python/AI" → Python, AI).
pub fn skill_terms(skills: &str) -> Vec<String> {
    skills
        .split(['/', ',', '&'])
        .flat_map(|part| part.split(" or ").flat_map(|p| p.split(" and ")))
        .map(str::trim)
        .filter(|s| s.chars().count() >= 2)
        .map(str::to_string)
        .fold(Vec::new(), |mut out, s| {
            if !out.iter().any(|x: &String| x.eq_ignore_ascii_case(&s)) {
                out.push(s);
            }
            out
        })
}

/// The job layer's query for contract work (never a second engine).
pub fn job_query(c: &ContractCriteria) -> JobQuery {
    let place = locations::label(&c.locations);
    let skills = if c.skills.trim().is_empty() {
        "freelance".to_string()
    } else {
        c.skills.trim().to_string()
    };
    JobQuery {
        // The places are named without "in …": the router would read one
        // place from that and filter by it; ReMa applies all of them (OR).
        text: format!(
            "Find current freelance, contract, interim or project work for {skills}.{}{}",
            if locations::is_empty(&c.locations) {
                String::new()
            } else {
                format!(" Places: {place}.")
            },
            if c.remote_ok {
                " Remote work is possible."
            } else {
                ""
            }
        ),
        role: Some(skills),
        // Several places are filtered by ReMa itself (B7 semantics); remote
        // work is acceptable, not required, so no work-mode filter.
        location: None,
        company: None,
        remote: false,
        posted_within_days: c.posted_within_days,
        min_salary: None,
        verify_urls: Vec::new(),
    }
}

/// The identity of a contract opportunity (B16): the canonical posting.
pub fn identity_key(result: &ContractResult) -> String {
    format!("contract:{}", result.key)
}

/// Runs a contract search.
pub async fn find(
    state: &AppState,
    run_id: &str,
    criteria: ContractCriteria,
    model: Option<(&Endpoint, &str)>,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> ContractResults {
    let now = now_ms();
    let mut criteria = criteria;
    let (places, place_notes) = locations::normalize(&criteria.locations);
    criteria.locations = places;
    let mut results = ContractResults {
        run_id: run_id.to_string(),
        status: RunStatus::Running,
        normalized: normalized(&criteria),
        criteria: criteria.clone(),
        confirmed: Vec::new(),
        needs_verification: Vec::new(),
        not_matching: Vec::new(),
        notes: Vec::new(),
        sources: Vec::new(),
        failures: Vec::new(),
        retrieved_at: now,
    };
    results.notes.extend(place_notes);
    progress.status("Finding contract work…");
    // The job layer matches every word of a role: "Python/AI" is searched
    // as Python and as AI (the model's web search once, with the whole
    // request).
    let mut skills = skill_terms(&criteria.skills);
    if skills.is_empty() {
        skills.push(String::new());
    }
    let mut listings: Vec<Listing> = Vec::new();
    let mut answered = false;
    for (i, skill) in skills.iter().take(3).enumerate() {
        let mut query = job_query(&criteria);
        if !skill.is_empty() {
            query.role = Some(skill.clone());
        }
        let outcome = crate::career_search::router::search_jobs_with(
            state,
            if i == 0 { model } else { None },
            &query,
            progress,
            cancel,
        )
        .await;
        match outcome {
            Outcome::Found(r) | Outcome::Empty(r) => {
                answered = true;
                for source in &r.sources {
                    if !results.sources.contains(source) {
                        results.sources.push(source.clone());
                    }
                }
                for f in r.fallbacks.iter().cloned().chain(
                    r.unreached
                        .iter()
                        .map(|s| format!("{s}: could not be searched this time")),
                ) {
                    if !results.failures.contains(&f) {
                        results.failures.push(f);
                    }
                }
                for listing in r.listings {
                    let key = normalize::canonical_url(&listing.url);
                    if !listings
                        .iter()
                        .any(|l| normalize::canonical_url(&l.url) == key)
                    {
                        listings.push(listing);
                    }
                }
            }
            Outcome::Failed { reasons } => {
                for f in reasons {
                    if !results.failures.contains(&f) {
                        results.failures.push(f);
                    }
                }
            }
            Outcome::Cancelled => {
                results.status = RunStatus::Cancelled;
                return results;
            }
        }
    }
    if !answered {
        results.status = RunStatus::Failed;
        return results;
    }
    // The canonical records (full descriptions and structured terms).
    let keys: Vec<(usize, String)> = listings
        .iter()
        .enumerate()
        .filter_map(|(i, l)| normalize::canonical_url(&l.url).map(|k| (i, format!("url:{k}"))))
        .collect();
    let mut records: HashMap<usize, JobRecord> = state
        .db
        .call(move |c| {
            let mut out = HashMap::new();
            for (i, key) in keys {
                if let Some(id) = jobs_store::find(c, &[key])? {
                    if let Some(record) = jobs_store::get(c, &id)? {
                        out.insert(i, record);
                    }
                }
            }
            Ok(out)
        })
        .unwrap_or_default();
    // Read listings the search only reported (bounded).
    progress.status("Checking contract terms…");
    let unread: Vec<usize> = listings
        .iter()
        .enumerate()
        .filter(|(i, l)| !records.contains_key(i) && l.verification == Verification::SearchOnly)
        .map(|(i, _)| i)
        .take(10)
        .collect();
    let session = Session {
        state: state.clone(),
        discovery: Discovery::own(),
    };
    let read: Vec<(usize, Option<JobRecord>)> = stream::iter(unread)
        .map(|i| {
            let url = listings[i].url.clone();
            let session = &session;
            async move {
                let input = GetJobInput {
                    id: None,
                    url: Some(url),
                    refresh: None,
                    description_cursor: None,
                };
                let got = tokio::time::timeout(
                    Duration::from_secs(25),
                    engine::get_job(session, &input, cancel),
                )
                .await;
                (i, got.ok().and_then(Result::ok).map(|d| d.job))
            }
        })
        .buffer_unordered(3)
        .collect()
        .await;
    for (i, record) in read {
        if let Some(record) = record {
            records.insert(i, record);
        }
    }
    if cancel.is_cancelled() {
        results.status = RunStatus::Cancelled;
        return results;
    }
    let saved: Vec<(String, String)> = state
        .db
        .call(|c| {
            Ok(store::opportunities(c)?
                .into_iter()
                .filter_map(|o| o.canonical_job_id.clone().map(|k| (k, o.id.clone())))
                .collect())
        })
        .unwrap_or_default();
    let mut seen: Vec<String> = Vec::new();
    for (i, listing) in listings.iter().enumerate() {
        let record = records.get(&i);
        let key = record.map(|r| r.id.clone()).unwrap_or_else(|| {
            format!(
                "url:{}",
                normalize::canonical_url(&listing.url).unwrap_or_else(|| listing.url.clone())
            )
        });
        // A cross-posting ReMa's job layer already merged appears once.
        if seen.contains(&key) {
            continue;
        }
        seen.push(key.clone());
        let t = terms(listing, record);
        let (status, reasons) = judge(&criteria, listing, record, &t, now);
        let scope = record
            .and_then(|r| r.description.text.clone())
            .or_else(|| listing.summary.clone())
            .map(|d| normalize::clip(&d, 400));
        let result = ContractResult {
            opportunity_id: saved
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, id)| id.clone()),
            key,
            title: listing.title.clone(),
            company: listing.company.clone(),
            location: listing.location.clone(),
            url: listing.url.clone(),
            source: listing.source.clone(),
            posted_at: listing.posted,
            verified: match (record.is_some(), listing.verification) {
                (true, _) | (_, Verification::Posting) => "Posting read".into(),
                (_, Verification::Page) => "Page read".into(),
                (_, Verification::SearchOnly) => "Found by search (not opened)".into(),
            },
            terms: t,
            status,
            reasons,
            scope,
        };
        match status {
            MatchStatus::Confirmed => results.confirmed.push(result),
            MatchStatus::NeedsVerification => results.needs_verification.push(result),
            MatchStatus::NotMatching => results.not_matching.push(result),
        }
    }
    let by_date = |a: &ContractResult, b: &ContractResult| b.posted_at.cmp(&a.posted_at);
    results.confirmed.sort_by(by_date);
    results.needs_verification.sort_by(by_date);
    results.not_matching.truncate(30);
    if results.confirmed.is_empty() && !listings.is_empty() {
        let mut blocking: Vec<String> = results
            .needs_verification
            .iter()
            .chain(&results.not_matching)
            .flat_map(|r| r.reasons.iter().cloned())
            .collect();
        blocking.sort();
        blocking.dedup();
        results.notes.push(format!(
            "No listing met every criterion. What prevented a match: {}. ReMa did not broaden \
             the criteria; change them if you want wider results.",
            blocking
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ")
        ));
    }
    results.status = if results.confirmed.is_empty() && results.needs_verification.is_empty() {
        RunStatus::NoVerifiedMatches
    } else if !results.failures.is_empty() {
        RunStatus::Partial
    } else {
        RunStatus::Complete
    };
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::SalaryStatus;

    fn listing(title: &str, summary: &str, location: &str) -> Listing {
        Listing {
            title: title.into(),
            company: Some("Client GmbH".into()),
            location: Some(location.into()),
            url: "https://jobs.example/1".into(),
            summary: Some(summary.into()),
            posted: Some(now_ms() - 86_400_000),
            salary: None,
            salary_status: SalaryStatus::NotListed,
            work_mode: None,
            verification: Verification::Posting,
            source: "Example".into(),
            notes: vec![],
        }
    }

    fn dach_python() -> ContractCriteria {
        parse_criteria(
            "Find Python/AI contracts in DACH lasting 1–6 months with rates above EUR 700/day.",
        )
        .0
    }

    #[test]
    fn the_request_becomes_visible_strict_criteria() {
        let c = dach_python();
        assert_eq!(c.min_rate, Some(700.0));
        assert_eq!(c.rate_comparison, Comparison::Above);
        assert_eq!(c.currency.as_deref(), Some("EUR"));
        assert_eq!(c.rate_unit, RateUnit::Day);
        assert_eq!(
            (c.duration_min_months, c.duration_max_months),
            (Some(1.0), Some(6.0))
        );
        assert_eq!(c.locations.countries, ["Germany", "Austria", "Switzerland"]);
        assert!(c.skills.to_lowercase().contains("python"), "{c:?}");
        let text = normalized(&c).join("\n");
        assert!(text.contains("strictly above EUR 700 per day"), "{text}");
        assert!(text.contains("(DACH)"), "{text}");
        let at_least = parse_criteria("freelance data projects at least €700 per day").0;
        assert_eq!(at_least.rate_comparison, Comparison::AtLeast);
    }

    fn rated(rate_text: &str) -> MatchStatus {
        let l = listing(
            "Freelance Python Developer (AI)",
            &format!("Freelance project in Munich, 3-5 months. {rate_text}"),
            "Munich, Germany",
        );
        let t = terms(&l, None);
        judge(&dach_python(), &l, None, &t, now_ms()).0
    }

    #[test]
    fn above_700_at_least_700_ranges_ceilings_and_unknown_differ() {
        assert_eq!(rated("Daily rate: 750 EUR"), MatchStatus::Confirmed);
        assert_eq!(
            rated("Rate: EUR 700/day"),
            MatchStatus::NotMatching,
            "700 is not above 700"
        );
        assert_eq!(rated("Rate: 650–800 €/day"), MatchStatus::NeedsVerification);
        assert_eq!(rated("up to 900 €/day"), MatchStatus::NeedsVerification);
        assert_eq!(rated("up to 650 €/day"), MatchStatus::NotMatching);
        assert_eq!(rated(""), MatchStatus::NeedsVerification);
        assert_eq!(rated("Rate: CHF 900/day"), MatchStatus::NeedsVerification);
        let mut at_least = dach_python();
        at_least.rate_comparison = Comparison::AtLeast;
        let l = listing(
            "Freelance Python Developer",
            "Freelance, 3 months, remote from Germany. Tagessatz 700 EUR",
            "Berlin, Germany",
        );
        let t = terms(&l, None);
        assert_eq!(
            judge(&at_least, &l, None, &t, now_ms()).0,
            MatchStatus::Confirmed
        );
    }

    #[test]
    fn hourly_rates_need_an_hours_per_day_basis() {
        let l = listing(
            "Freelance Python Engineer",
            "Freelance, 4 months. Stundensatz 95 €",
            "Vienna, Austria",
        );
        let t = terms(&l, None);
        assert_eq!(t.rate_unit, Some(RateUnit::Hour));
        let (status, reasons) = judge(&dach_python(), &l, None, &t, now_ms());
        assert_eq!(status, MatchStatus::NeedsVerification);
        assert!(
            reasons.iter().any(|r| r.contains("hours-per-day")),
            "{reasons:?}"
        );
        let mut with_basis = dach_python();
        with_basis.hours_per_day = Some(8.0);
        assert_eq!(
            judge(&with_basis, &l, None, &t, now_ms()).0,
            MatchStatus::Confirmed
        );
    }

    #[test]
    fn durations_must_lie_within_the_range_and_weeks_are_not_converted() {
        let judge_duration = |text: &str| {
            let l = listing(
                "Freelance Python Developer",
                &format!("Freelance. {text} Tagessatz 800 EUR"),
                "Graz, Austria",
            );
            let t = terms(&l, None);
            judge(&dach_python(), &l, None, &t, now_ms())
        };
        assert_eq!(
            judge_duration("Duration: 3-5 months.").0,
            MatchStatus::Confirmed
        );
        assert_eq!(
            judge_duration("Duration: 4-9 months.").0,
            MatchStatus::NeedsVerification
        );
        assert_eq!(
            judge_duration("Duration: 9-12 months.").0,
            MatchStatus::NotMatching
        );
        let (status, reasons) = judge_duration("Duration: 8 weeks.");
        assert_eq!(status, MatchStatus::NeedsVerification);
        assert!(reasons.iter().any(|r| r.contains("weeks")), "{reasons:?}");
        assert_eq!(judge_duration("").0, MatchStatus::NeedsVerification);
    }

    #[test]
    fn employment_is_not_contract_work_and_contract_alone_needs_verification() {
        assert_eq!(
            engagement(
                "Python Developer (Contract, 6 months)",
                "Befristet auf 6 Monate. Jahresgehalt 65.000 € brutto.",
                None,
                false
            ),
            Engagement::FixedTermEmployee
        );
        assert_eq!(
            engagement("Python Developer - Contract", "Join our team.", None, false),
            Engagement::NeedsVerification
        );
        assert_eq!(
            engagement("Senior Python Freelancer", "Remote project", None, false),
            Engagement::Freelance
        );
        assert_eq!(
            engagement("Data Engineer", "On a B2B contract basis", None, false),
            Engagement::B2b
        );
        assert_eq!(
            engagement("Interim CTO", "", None, false),
            Engagement::Interim
        );
        let l = listing(
            "Python Developer (m/w/d) - befristet",
            "Befristete Anstellung für 6 Monate, 60.000 € p.a.",
            "Vienna, Austria",
        );
        let t = terms(&l, None);
        let (status, reasons) = judge(&dach_python(), &l, None, &t, now_ms());
        assert_eq!(status, MatchStatus::NotMatching);
        assert!(reasons[0].contains("employment"), "{reasons:?}");
    }

    #[test]
    fn remote_is_not_worldwide_and_agencies_keep_the_client_undisclosed() {
        let l = listing(
            "Freelance Python Developer",
            "Freelance, 3 months, fully remote. EU only. Tagessatz 800 EUR. On behalf of our client, a bank.",
            "Remote",
        );
        let mut t = terms(&l, None);
        assert_eq!(t.eligibility, ["EU only"]);
        assert!(t.agency.is_some());
        assert_eq!(
            t.end_client, None,
            "an undisclosed client stays undisclosed"
        );
        t.work_mode = Some("Remote".into());
        // "EU only" does not say whether DACH is included: unknown.
        let (status, _) = judge(&dach_python(), &l, None, &t, now_ms());
        assert_eq!(status, MatchStatus::NeedsVerification);
        let germany = listing(
            "Freelance Python Developer",
            "Freelance, 3 months, remote, Germany only. Tagessatz 800 EUR.",
            "Remote",
        );
        let mut t = terms(&germany, None);
        t.work_mode = Some("Remote".into());
        assert_eq!(
            judge(&dach_python(), &germany, None, &t, now_ms()).0,
            MatchStatus::Confirmed
        );
        let us = listing(
            "Freelance Python Developer",
            "Freelance, 3 months, remote, US only. Day rate $900.",
            "Remote",
        );
        let mut t = terms(&us, None);
        t.work_mode = Some("Remote".into());
        assert_eq!(
            judge(&dach_python(), &us, None, &t, now_ms()).0,
            MatchStatus::NotMatching
        );
    }
}
