//! The research planner (NC §20, §48, §49): reads a request into a
//! [`NetworkIntent`] and picks only the stages it needs — companies, jobs,
//! people, connections. Deterministic, like the career-search planner it
//! builds on; the place comes only from the request.

use std::sync::OnceLock;

use regex::Regex;

use super::model::{Criteria, Stage};
use crate::{
    analytics::normalize,
    career_search::plan::{self, Place},
};

/// Results shown unless the request asks for another number (NC §49, B8).
pub const DEFAULT_LIMIT: u32 = 20;
/// Most results one request may ask for.
pub const MAX_LIMIT: u32 = 50;

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// An employee range ("50–500 employees").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeRange {
    pub min: Option<u32>,
    pub max: Option<u32>,
}

impl SizeRange {
    pub fn contains(self, employees: u32) -> bool {
        self.min.is_none_or(|m| employees >= m) && self.max.is_none_or(|m| employees <= m)
    }

    pub fn label(self) -> String {
        match (self.min, self.max) {
            (Some(a), Some(b)) => format!("{a}–{b} employees"),
            (Some(a), None) => format!("at least {a} employees"),
            (None, Some(b)) => format!("at most {b} employees"),
            (None, None) => "any size".into(),
        }
    }
}

/// The kind of people a request looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeopleKind {
    /// Recruiters, talent acquisition, HR.
    Recruiter,
    /// The hiring side of a job: hiring manager, named contact, recruiter.
    HiringSide,
    /// Heads, directors, VPs, practice leads of a function.
    Leader,
    /// Team leads, engineering managers.
    TeamLead,
    /// CEO, CTO, founders.
    Executive,
    /// "The most relevant people", "who should I talk to".
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeopleRole {
    pub kind: PeopleKind,
    /// The function led ("Product", "AI", "Engineering").
    pub function: Option<String>,
    /// As the request put it ("Head of Product").
    pub label: String,
}

/// What a Network Connect request asks for (NC §48 NetworkIntent).
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkIntent {
    pub text: String,
    pub place: Option<Place>,
    pub remote: bool,
    pub industries: Vec<String>,
    pub size: Option<SizeRange>,
    pub technologies: Vec<String>,
    /// The job roles sought, the one asked first, then close variants.
    pub roles: Vec<String>,
    pub seniority: Option<String>,
    /// Companies must show current openings.
    pub hiring: bool,
    pub min_openings: Option<u32>,
    pub posted_within_days: Option<u32>,
    pub people: Vec<PeopleRole>,
    /// People asked for per company or job ("the five people").
    pub people_limit: Option<u32>,
    /// Whether the request asks about the user's own connections.
    pub relationships: bool,
    /// Asked for second-degree connections (unsupported: said so).
    pub second_degree: bool,
    pub limit: u32,
    pub target_company: Option<String>,
    /// A job the request is about (its address).
    pub job_url: Option<String>,
    /// The request refers to the user's own background (NC §27).
    pub uses_profile: bool,
    /// The subject is a list of companies.
    pub company_discovery: bool,
}

const NUMBER_WORDS: &[(&str, u32)] = &[
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("twelve", 12),
    ("fifteen", 15),
    ("twenty", 20),
    ("thirty", 30),
    ("forty", 40),
    ("fifty", 50),
];

fn number(word: &str) -> Option<u32> {
    let word = word.trim().to_lowercase();
    word.replace([',', '.'], "")
        .parse::<u32>()
        .ok()
        .or_else(|| {
            NUMBER_WORDS
                .iter()
                .find(|(w, _)| *w == word)
                .map(|(_, n)| *n)
        })
}

/// Industries ReMa recognizes by name (matched as "<industry> companies",
/// "companies in <industry>", "<industry> firms").
const INDUSTRIES: &[(&str, &str)] = &[
    ("fintech", "Fintech"),
    ("financial technology", "Fintech"),
    ("insurtech", "Insurtech"),
    ("insurance", "Insurance"),
    ("banking", "Banking"),
    ("banks", "Banking"),
    ("renewable energy", "Renewable energy"),
    ("renewable-energy", "Renewable energy"),
    ("renewables", "Renewable energy"),
    ("clean energy", "Renewable energy"),
    ("cleantech", "Cleantech"),
    ("climate tech", "Climate tech"),
    ("energy", "Energy"),
    ("healthtech", "Health tech"),
    ("health tech", "Health tech"),
    ("healthcare", "Healthcare"),
    ("medtech", "Medical technology"),
    ("biotech", "Biotechnology"),
    ("pharma", "Pharmaceuticals"),
    ("pharmaceutical", "Pharmaceuticals"),
    ("e-commerce", "E-commerce"),
    ("ecommerce", "E-commerce"),
    ("retail", "Retail"),
    ("logistics", "Logistics"),
    ("automotive", "Automotive"),
    ("mobility", "Mobility"),
    ("manufacturing", "Manufacturing"),
    ("industrial", "Industrial"),
    ("aerospace", "Aerospace"),
    ("gaming", "Gaming"),
    ("game", "Gaming"),
    ("media", "Media"),
    ("telecom", "Telecommunications"),
    ("telecommunications", "Telecommunications"),
    ("cybersecurity", "Cybersecurity"),
    ("security", "Security"),
    ("edtech", "Education technology"),
    ("proptech", "Proptech"),
    ("real estate", "Real estate"),
    ("legal tech", "Legal tech"),
    ("legaltech", "Legal tech"),
    ("consulting", "Consulting"),
    ("saas", "SaaS"),
    ("software", "Software"),
    ("ai", "AI"),
    ("artificial intelligence", "AI"),
    ("robotics", "Robotics"),
    ("semiconductor", "Semiconductors"),
    ("agritech", "Agritech"),
    ("travel", "Travel"),
    ("tourism", "Tourism"),
    ("construction", "Construction"),
    ("chemical", "Chemicals"),
    ("crypto", "Crypto"),
    ("blockchain", "Blockchain"),
    ("marketing", "Marketing"),
    ("adtech", "Advertising technology"),
    ("hr tech", "HR tech"),
    ("food", "Food"),
];

fn industries(text: &str) -> Vec<String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    // "<industry> companies/firms/startups", "companies in <industry>",
    // "companies in the <industry> sector/industry".
    let names: Vec<String> = INDUSTRIES.iter().map(|(k, _)| regex::escape(k)).collect();
    let alternatives = {
        let mut sorted = names.clone();
        sorted.sort_by_key(|s| std::cmp::Reverse(s.len()));
        sorted.join("|")
    };
    let pattern = re(
        &CELL,
        &format!(
            r"(?i)\b(?P<a>{alternatives})\s+(?:compan(?:y|ies)|firms?|startups?|businesses|employers?|sector|industry|players)\b|\b(?:compan(?:y|ies)|firms?|startups?|businesses)\s+in\s+(?:the\s+)?(?P<b>{alternatives})(?:\s+(?:sector|industry|space))?\b"
        ),
    );
    let mut out: Vec<String> = Vec::new();
    for caps in pattern.captures_iter(text) {
        let found = caps
            .name("a")
            .or_else(|| caps.name("b"))
            .map(|m| m.as_str().to_lowercase())
            .unwrap_or_default();
        if let Some((_, label)) = INDUSTRIES.iter().find(|(k, _)| *k == found) {
            if !out.iter().any(|o| o == label) {
                out.push(label.to_string());
            }
        }
    }
    out
}

fn size(text: &str) -> Option<SizeRange> {
    static RANGE: OnceLock<Regex> = OnceLock::new();
    static MIN: OnceLock<Regex> = OnceLock::new();
    static MAX: OnceLock<Regex> = OnceLock::new();
    let range = re(
        &RANGE,
        r"(?i)\b(\d[\d,.]*)\s*(?:–|-|—|to|and)\s*(\d[\d,.]*)\s*(?:employees|people|staff|mitarbeiter(?:innen)?)\b",
    );
    if let Some(c) = range.captures(text) {
        let (a, b) = (number(&c[1])?, number(&c[2])?);
        return Some(SizeRange {
            min: Some(a.min(b)),
            max: Some(a.max(b)),
        });
    }
    let min = re(
        &MIN,
        r"(?i)\b(?:at\s+least|more\s+than|over|min(?:imum)?(?:\s+of)?)\s+(\d[\d,.]*)\s*(?:employees|people|staff)\b|\b(\d[\d,.]*)\+\s*employees\b",
    );
    if let Some(c) = min.captures(text) {
        let n = c
            .get(1)
            .or_else(|| c.get(2))
            .and_then(|m| number(m.as_str()))?;
        return Some(SizeRange {
            min: Some(n),
            max: None,
        });
    }
    let max = re(
        &MAX,
        r"(?i)\b(?:fewer\s+than|less\s+than|under|up\s+to|at\s+most|max(?:imum)?(?:\s+of)?)\s+(\d[\d,.]*)\s*(?:employees|people|staff)\b",
    );
    max.captures(text).and_then(|c| {
        number(&c[1]).map(|n| SizeRange {
            min: None,
            max: Some(n),
        })
    })
}

/// Words that are never a technology or a role on their own.
const GENERIC: &[&str] = &[
    "the",
    "a",
    "an",
    "and",
    "or",
    "of",
    "for",
    "in",
    "at",
    "with",
    "senior",
    "junior",
    "lead",
    "principal",
    "staff",
    "head",
    "new",
    "open",
    "relevant",
    "current",
    "currently",
    "more",
    "people",
    "roles",
    "jobs",
    "positions",
    "companies",
    "my",
    "their",
    "who",
    "that",
    "which",
    "some",
    "any",
    "many",
    "several",
    "multiple",
    "good",
    "strong",
    "experienced",
];

fn technologies(text: &str) -> Vec<String> {
    static USING: OnceLock<Regex> = OnceLock::new();
    static EXPERIENCE: OnceLock<Regex> = OnceLock::new();
    let using = re(
        &USING,
        r"(?i)\b(?:using|use|uses|built\s+on|running|that\s+run)\s+(?P<t>[A-Za-z0-9][\w.+#/-]{1,30})",
    );
    let experience = re(
        &EXPERIENCE,
        r"(?i)\b(?:with|in)\s+(?P<t>[A-Za-z0-9][\w.+#/-]{1,30})\s+(?:experience|skills|expertise|know-how)\b",
    );
    let mut out: Vec<String> = Vec::new();
    for caps in using
        .captures_iter(text)
        .chain(experience.captures_iter(text))
    {
        let t = caps["t"].trim_end_matches(['.', ',']).to_string();
        let lower = t.to_lowercase();
        if GENERIC.contains(&lower.as_str()) || lower.len() < 2 {
            continue;
        }
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&t)) {
            out.push(t);
        }
    }
    out
}

/// "Product Managers" → "Product Manager"; "Engineers" → "Engineer".
fn singular(role: &str) -> String {
    let role = role.trim();
    let words: Vec<&str> = role.split_whitespace().collect();
    let Some((last, rest)) = words.split_last() else {
        return String::new();
    };
    let lower = last.to_lowercase();
    let last = if lower.ends_with("ies") && last.len() > 4 {
        format!("{}y", &last[..last.len() - 3])
    } else if lower.ends_with("sses") || lower.ends_with("ss") {
        last.to_string()
    } else if lower.ends_with('s') && !lower.ends_with("ops") && last.len() > 3 {
        last[..last.len() - 1].to_string()
    } else {
        last.to_string()
    };
    let mut out: Vec<String> = rest.iter().map(|w| w.to_string()).collect();
    out.push(last);
    out.join(" ")
}

/// Seniority words at the start of a role.
fn split_seniority(role: &str) -> (Option<String>, String) {
    const LEVELS: &[&str] = &[
        "senior",
        "junior",
        "lead",
        "principal",
        "staff",
        "mid-level",
    ];
    let mut words: Vec<&str> = role.split_whitespace().collect();
    if let Some(first) = words.first() {
        let lower = first.to_lowercase();
        if LEVELS.contains(&lower.as_str()) && words.len() > 1 {
            let level = first.to_string();
            words.remove(0);
            return (Some(level), words.join(" "));
        }
    }
    (None, role.to_string())
}

/// The roles a request hires for ("hiring Product Managers", "building AI
/// teams", "SAP consulting positions").
fn roles(text: &str) -> (Vec<String>, Option<String>) {
    static HIRING: OnceLock<Regex> = OnceLock::new();
    static BUILDING: OnceLock<Regex> = OnceLock::new();
    static POSITIONS: OnceLock<Regex> = OnceLock::new();
    let hiring = re(
        &HIRING,
        r"(?i)\bhiring\s+(?:for\s+)?(?:people\s+with\s+\S+\s+experience|(?P<role>(?:[\w+#./-]+\s+){0,3}?(?:engineers?|developers?|managers?|scientists?|designers?|analysts?|consultants?|architects?|specialists?|leads?|owners?|marketers?|recruiters?|administrators?|admins?|researchers?|writers?|accountants?|nurses?|technicians?|salespeople|sales\s+reps?|representatives?|associates?|officers?|directors?|heads?)))\b",
    );
    let building = re(
        &BUILDING,
        r"(?i)\b(?:building|growing|expanding|scaling)\s+(?:their\s+|an?\s+)?(?P<area>[\w+#./-]+(?:\s+[\w+#./-]+)?)\s+teams?\b",
    );
    let positions = re(
        &POSITIONS,
        r"(?i)\b(?P<area>[\w+#./-]+(?:\s+[\w+#./-]+)?)\s+(?:positions|roles|openings|vacancies|jobs)\b",
    );
    let mut roles: Vec<String> = Vec::new();
    let mut seniority = None;
    if let Some(role) = hiring
        .captures(text)
        .and_then(|c| c.name("role").map(|m| m.as_str().to_string()))
    {
        let (level, role) = split_seniority(&singular(&role));
        seniority = level;
        roles.push(role);
    } else if let Some(area) = building.captures(text).map(|c| c["area"].to_string()) {
        roles.push(area);
    } else if let Some(area) = positions.captures(text).map(|c| c["area"].to_string()) {
        let lower = area.to_lowercase();
        let first = lower
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        if !GENERIC.contains(&first.as_str()) && !GENERIC.contains(&lower.as_str()) {
            let (level, role) = split_seniority(&area);
            seniority = level;
            roles.push(role);
        }
    }
    if let Some(first) = roles.first().cloned() {
        for related in plan::related_roles(&first) {
            if !roles.iter().any(|r| r.eq_ignore_ascii_case(&related)) {
                roles.push(related);
            }
        }
    }
    roles.truncate(6);
    (roles, seniority)
}

fn people(text: &str) -> Vec<PeopleRole> {
    static LEADER: OnceLock<Regex> = OnceLock::new();
    static LEADERS_OF: OnceLock<Regex> = OnceLock::new();
    static PRACTICE: OnceLock<Regex> = OnceLock::new();
    static TECHNICAL: OnceLock<Regex> = OnceLock::new();
    let leader = re(
        &LEADER,
        r"\b(?i:head|vp|vice\s+president|director|chief)\s+of\s+(?P<f>[A-Za-z&/]+(?:\s+(?:[A-Z][A-Za-z&]*|&\s*[A-Za-z]+))?)",
    );
    let leaders_of = re(
        &LEADERS_OF,
        r"(?i)\b(?P<f>[a-z&/]+(?:\s+(?:or|and|/)\s+[a-z&/]+)?)\s+(?:leaders?|leadership|heads)\b",
    );
    let practice = re(&PRACTICE, r"(?i)\bpractice[- ]leads?\b");
    let technical = re(&TECHNICAL, r"(?i)\btechnical\b[^.?!]{0,20}\bcontacts?\b");
    let lower = text.to_lowercase();
    let mut out: Vec<PeopleRole> = Vec::new();
    let mut push = |kind: PeopleKind, function: Option<String>, label: &str| {
        let role = PeopleRole {
            kind,
            function,
            label: label.to_string(),
        };
        if !out.contains(&role) {
            out.push(role);
        }
    };
    for c in leader.captures_iter(text) {
        let function = c["f"]
            .split(" or ")
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        push(
            PeopleKind::Leader,
            Some(function.clone()),
            &format!("Head of {function}"),
        );
    }
    for c in leaders_of.captures_iter(text) {
        for function in c["f"]
            .split([' ', '/'])
            .filter(|w| !["or", "and", ""].contains(w))
        {
            if GENERIC.contains(&function)
                || ["team", "the", "most", "relevant"].contains(&function)
            {
                continue;
            }
            let function = if function.len() <= 3 {
                function.to_uppercase()
            } else {
                let mut c = function.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            };
            push(
                PeopleKind::Leader,
                Some(function.clone()),
                &format!("{function} leaders"),
            );
        }
    }
    if practice.is_match(text) {
        push(PeopleKind::Leader, None, "Practice lead");
    }
    if technical.is_match(text) {
        push(
            PeopleKind::Leader,
            Some("Engineering".into()),
            "Technical contact",
        );
    }
    if lower.contains("recruit")
        || lower.contains("talent acquisition")
        || lower.contains("talent partner")
    {
        push(PeopleKind::Recruiter, None, "Recruiter");
    }
    if lower.contains("hiring manager")
        || lower.contains("hiring-side")
        || lower.contains("hiring side")
        || lower.contains("hiring/recruiting")
        || lower.contains("job poster")
    {
        push(PeopleKind::HiringSide, None, "Hiring-side contact");
    }
    if lower.contains("team lead")
        || lower.contains("engineering manager")
        || lower.contains("lead the team")
        || lower.contains("leads the team")
    {
        push(PeopleKind::TeamLead, None, "Team lead");
    }
    for exec in ["ceo", "cto", "cpo", "cfo", "coo", "founder"] {
        if Regex::new(&format!(r"(?i)\b{exec}s?\b"))
            .map(|r| r.is_match(text))
            .unwrap_or(false)
        {
            push(PeopleKind::Executive, None, &exec.to_uppercase());
        }
    }
    static ANY: OnceLock<Regex> = OnceLock::new();
    let any = re(
        &ANY,
        r"(?i)\b(?:(?:most\s+)?relevant\s+(?:people|persons?|contacts?|hiring-side\s+contacts?)|people\s+(?:i\s+)?(?:should|could|to)\s+(?:know|contact|talk\s+to|reach\s+out\s+to|meet)|who\s+(?:should|could|can|do)\s+i\s+(?:contact|talk\s+to|reach\s+out\s+to|know|speak\s+(?:to|with))|who\s+(?:are|is)\s+the\s+(?:\w+\s+)?(?:most\s+)?relevant|people\s+behind|who\s+appears?\s+to\s+lead|who\s+leads)\b",
    );
    if any.is_match(text) {
        push(PeopleKind::Any, None, "Relevant people");
    }
    out
}

fn relationships(text: &str) -> (bool, bool) {
    static CELL: OnceLock<Regex> = OnceLock::new();
    static SECOND: OnceLock<Regex> = OnceLock::new();
    let pattern = re(
        &CELL,
        r"(?i)\b(?:do\s+i\s+(?:already\s+)?know\s+(?:any(?:one|body)|someone|people)|who\s+do\s+i\s+know|my\s+(?:permitted\s+)?(?:linkedin\s+|xing\s+)?(?:first[- ]degree\s+|1st[- ]degree\s+)?(?:connections|network|contacts)|first[- ]degree|1st[- ]degree|warm\s+intro(?:duction)?s?|in\s+my\s+network|mutual\s+connections?|second[- ]degree|2nd[- ]degree|know\s+anyone\s+at)\b",
    );
    let second = re(
        &SECOND,
        r"(?i)\b(?:second|2nd)[- ]degree|connections\s+of\s+(?:my\s+)?connections\b",
    );
    (pattern.is_match(text), second.is_match(text))
}

fn limits(text: &str) -> (u32, Option<u32>) {
    static COUNT: OnceLock<Regex> = OnceLock::new();
    let count = re(
        &COUNT,
        r"(?i)\b(?:find|show|list|give|get|name|identify|top|the|with)\s+(?:me\s+)?(?:the\s+)?(?:top\s+)?(?P<n>\d{1,3}|one|two|three|four|five|six|seven|eight|nine|ten|twelve|fifteen|twenty|thirty|forty|fifty)\s+(?:most\s+relevant\s+|relevant\s+|best\s+|key\s+|[\w-]+\s+){0,2}?(?P<what>compan(?:y|ies)|employers?|firms?|startups?|people|persons|contacts|recruiters|leaders|results)\b",
    );
    let mut limit = DEFAULT_LIMIT;
    let mut people_limit = None;
    for c in count.captures_iter(text) {
        let Some(n) = number(&c["n"]) else { continue };
        let what = c["what"].to_lowercase();
        if ["people", "persons", "contacts", "recruiters", "leaders"].contains(&what.as_str()) {
            people_limit = Some(n.clamp(1, MAX_LIMIT));
        } else {
            limit = n.clamp(1, MAX_LIMIT);
        }
    }
    (limit, people_limit)
}

fn hiring_people() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\bhiring(?:[- ]side|\s*/\s*recruiting)?\s+(?:managers?|contacts?|people|persons?|teams?\s+members?)\b|\bhiring[- ]side\b",
    )
}

fn hiring(text: &str) -> (bool, Option<u32>, Option<u32>) {
    static HIRING: OnceLock<Regex> = OnceLock::new();
    static MIN: OnceLock<Regex> = OnceLock::new();
    static DAYS: OnceLock<Regex> = OnceLock::new();
    let hiring = re(
        &HIRING,
        r"(?i)\b(?:hiring|recruiting\s+for|currently\s+(?:have|has|show)|open\s+(?:roles|positions|jobs)|openings|vacancies|building\s+[\w\s/-]{1,20}teams?|job\s+postings?)\b",
    );
    let min = re(
        &MIN,
        r"(?i)\b(?:at\s+least|min(?:imum)?(?:\s+of)?|more\s+than)\s+(?P<n>\d{1,3}|two|three|four|five|six|seven|eight|nine|ten)\s+(?:relevant\s+)?(?:open\s+|current\s+)?(?:roles|openings|positions|jobs|vacancies)\b|\b(?P<m>\d{1,3})\+\s+(?:open\s+)?(?:roles|openings|positions|jobs)\b|\bmultiple\s+(?:[\w/-]+\s+)?(?:openings|roles|positions)\b",
    );
    let days = re(
        &DAYS,
        r"(?i)\b(?:last|past)\s+(?:(?P<n>\d{1,3})\s+days|(?P<unit>week|month|two\s+weeks|fortnight))\b",
    );
    // "hiring manager" names a person, not hiring activity.
    let text = &hiring_people().replace_all(text, " ");
    let min_openings = min.captures(text).and_then(|c| {
        c.name("n")
            .or_else(|| c.name("m"))
            .and_then(|m| number(m.as_str()))
            .or(Some(2))
    });
    let posted = days.captures(text).and_then(|c| {
        if let Some(n) = c.name("n") {
            return number(n.as_str());
        }
        match c.name("unit").map(|m| m.as_str().to_lowercase()).as_deref() {
            Some("week") => Some(7),
            Some("month") => Some(30),
            Some(_) => Some(14),
            None => None,
        }
    });
    (
        hiring.is_match(text) || min_openings.is_some(),
        min_openings,
        posted,
    )
}

/// Words that name a department or role, not a company.
const NOT_COMPANIES: &[&str] = &[
    "product",
    "engineering",
    "ai",
    "data",
    "sales",
    "marketing",
    "operations",
    "hr",
    "people",
    "talent",
    "design",
    "finance",
    "it",
    "security",
    "research",
    "the",
    "this",
    "that",
    "my",
    "company",
    "companies",
    "each",
    "linkedin",
    "xing",
    "vienna",
    "austria",
    "germany",
    "remote",
];

/// The company a request is about ("work at Company X", "anyone at
/// Bitpanda").
fn target_company(text: &str) -> Option<String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let pattern = re(
        &CELL,
        r"\b(?:(?i:work|working|job|apply|applying|interested|interview(?:ing)?|anyone|anybody|someone|people|contacts?|connections?|know\s+\w+|position|role|opening|team|leads?)\s+(?:(?i:at|for|with|in)\s+)?(?i:at))\s+(?P<name>(?:[A-Z][\w&.\-]*|[A-Z]{2,})(?:\s+(?:[A-Z][\w&.\-]*|[A-Z]{2,}|X)){0,3})",
    );
    for caps in pattern.captures_iter(text) {
        let name = sentence_end(&caps["name"]);
        let lower = name.to_lowercase();
        let place = normalize::place(&name);
        if place.city().is_some()
            || place.country().is_some()
            || NOT_COMPANIES.contains(&lower.as_str())
            || name.len() < 2
        {
            continue;
        }
        return Some(name);
    }
    // "Company X" as the request's subject.
    plan::plan(text).companies.into_iter().find(|c| {
        let lower = c.to_lowercase();
        !NOT_COMPANIES.contains(&lower.as_str())
            && !lower.split_whitespace().all(|w| NOT_COMPANIES.contains(&w))
    })
}

/// A captured name up to the end of its sentence ("Nordlicht AI. Who" →
/// "Nordlicht AI").
pub fn sentence_end(name: &str) -> String {
    let cut = name
        .find(". ")
        .or_else(|| name.find(['?', '!', ';', ':']))
        .unwrap_or(name.len());
    name[..cut]
        .trim()
        .trim_end_matches(['.', ',', '?', '!'])
        .to_string()
}

fn job_url(text: &str) -> Option<String> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let url = re(&CELL, r#"https?://[^\s<>"')\]]+"#);
    url.find(text)
        .and_then(|m| normalize::web_url(m.as_str().trim_end_matches(['.', ','])))
}

fn uses_profile(text: &str) -> bool {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\bmy\s+(?:profile|background|cv|resume|skills|experience|target\s+roles?|[\w/-]+\s+background)\b|\bwhere\s+i\s+(?:would\s+)?fit\b|\bfor\s+me\b",
    )
    .is_match(text)
}

fn company_discovery(text: &str) -> bool {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(?:find|show|list|which|what|identify|discover|give\s+me|get\s+me|research|map)\b[^.?!]{0,60}\b(?:compan(?:ies)|employers|firms|startups|businesses|organi[sz]ations)\b",
    )
    .is_match(text)
}

/// Reads a request (every field it states; stages follow from them).
pub fn intent(text: &str) -> NetworkIntent {
    let text = text.trim();
    let base = plan::plan(text);
    let (roles, seniority) = roles(text);
    let (hiring, min_openings, posted) = hiring(text);
    let (relationships, second_degree) = relationships(text);
    let (limit, people_limit) = limits(text);
    let people = people(text);
    let mut technologies = technologies(text);
    // "hiring Kubernetes engineers": the technology is part of the role.
    if let Some(role) = roles.first() {
        let first = role.split_whitespace().next().unwrap_or_default();
        let known_family = !plan::related_roles(role).is_empty();
        if !known_family
            && role.split_whitespace().count() > 1
            && first.chars().next().is_some_and(|c| c.is_uppercase())
            && !GENERIC.contains(&first.to_lowercase().as_str())
            && ![
                "Product",
                "Software",
                "Data",
                "Backend",
                "Frontend",
                "Sales",
                "Marketing",
                "Project",
                "Account",
            ]
            .contains(&first)
            && !technologies.iter().any(|t| t.eq_ignore_ascii_case(first))
        {
            technologies.push(first.to_string());
        }
    }
    NetworkIntent {
        text: text.to_string(),
        place: base.place.clone(),
        remote: base.remote,
        industries: industries(text),
        size: size(text),
        technologies,
        roles,
        seniority,
        hiring,
        min_openings,
        posted_within_days: posted.or(base.posted_within_days),
        people,
        people_limit,
        relationships,
        second_degree,
        limit,
        target_company: target_company(text),
        job_url: job_url(text),
        uses_profile: uses_profile(text),
        company_discovery: company_discovery(text),
    }
}

impl NetworkIntent {
    /// The stages this request needs, in order (NC §48: never every stage
    /// by default).
    pub fn stages(&self) -> Vec<Stage> {
        let mut stages = Vec::new();
        let wants_jobs = self.hiring
            || !self.roles.is_empty()
            || !self.technologies.is_empty()
            || self.job_url.is_some()
            || self.min_openings.is_some();
        if self.company_discovery
            || self.target_company.is_some()
            || !self.industries.is_empty()
            || self.size.is_some()
        {
            stages.push(Stage::Companies);
        }
        if wants_jobs {
            stages.push(Stage::Jobs);
        }
        if !self.people.is_empty() {
            stages.push(Stage::People);
        }
        if self.relationships {
            stages.push(Stage::Connections);
            // Matching connections needs companies.
            if !stages.contains(&Stage::Companies) {
                stages.insert(0, Stage::Companies);
            }
        }
        if stages.is_empty() {
            stages.push(Stage::Companies);
        }
        stages
    }

    pub fn criteria(&self) -> Criteria {
        Criteria {
            locations: self.place.iter().map(Place::label).collect(),
            industries: self.industries.clone(),
            company_size: self.size.map(SizeRange::label),
            technologies: self.technologies.clone(),
            roles: self.roles.iter().take(1).cloned().collect(),
            seniority: self.seniority.clone(),
            min_openings: self.min_openings,
            posted_within_days: self.posted_within_days,
            people: self.people.iter().map(|p| p.label.clone()).collect(),
            target_company: self.target_company.clone(),
            relationships: self.relationships,
            uses_profile: self.uses_profile,
            limit: self.limit,
            stages: self.stages(),
        }
    }

    /// People a stage looks for when the request names none but needs
    /// them (a relationship question needs none).
    pub fn wants_people(&self) -> bool {
        !self.people.is_empty()
    }
}

/// A chat message that Network Connect answers (the rest stay with job
/// search and research): relationship questions, "who should I talk to",
/// company discovery, people across companies or jobs.
pub fn detect(text: &str) -> Option<NetworkIntent> {
    static ASKS: OnceLock<Regex> = OnceLock::new();
    // Research is asked for; "explain what a hiring manager does" is not.
    let asks = re(
        &ASKS,
        r"(?i)\b(?:find|show|list|which|who|identify|research|discover|search|look\s+(?:up|for)|get\s+me|give\s+me|do\s+i\s+(?:already\s+)?know|are\s+there|map|name)\b",
    );
    if !asks.is_match(text) {
        return None;
    }
    let intent = intent(text);
    if intent.text.is_empty() {
        return None;
    }
    let people_discovery = !intent.people.is_empty()
        && (intent.company_discovery
            || intent.hiring
            || intent.job_url.is_some()
            || intent.people.iter().any(|p| p.kind == PeopleKind::Any)
            || (intent.target_company.is_none() && intent.place.is_some()));
    let company_search = intent.company_discovery
        && (!intent.industries.is_empty()
            || intent.size.is_some()
            || intent.hiring
            || !intent.technologies.is_empty()
            || intent.place.is_some()
            || intent.limit != DEFAULT_LIMIT
            || intent.uses_profile);
    (intent.relationships || people_discovery || company_search).then_some(intent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(i: &NetworkIntent) -> Vec<PeopleKind> {
        i.people.iter().map(|p| p.kind).collect()
    }

    #[test]
    fn compound_graph_query() {
        let i = detect(
            "Find fintech companies in Vienna with 50–500 employees that are hiring Product \
             Managers, then find the Head of Product or recruiting contact for each.",
        )
        .unwrap();
        assert_eq!(i.industries, ["Fintech"]);
        assert_eq!(
            i.place.as_ref().and_then(|p| p.city.as_deref()),
            Some("Vienna")
        );
        assert_eq!(
            i.size,
            Some(SizeRange {
                min: Some(50),
                max: Some(500)
            })
        );
        assert_eq!(i.roles.first().map(String::as_str), Some("Product Manager"));
        assert!(i.hiring);
        assert!(i
            .people
            .iter()
            .any(|p| p.kind == PeopleKind::Leader && p.function.as_deref() == Some("Product")));
        assert!(kinds(&i).contains(&PeopleKind::Recruiter));
        assert_eq!(
            i.target_company, None,
            "\"Head of Product\" is not a company"
        );
        assert_eq!(i.stages(), [Stage::Companies, Stage::Jobs, Stage::People]);
        assert_eq!(i.limit, DEFAULT_LIMIT);
    }

    #[test]
    fn industry_discovery_with_a_count() {
        let i =
            detect("Find 50 renewable-energy companies in Vienna and give me their company pages.")
                .unwrap();
        assert_eq!(i.industries, ["Renewable energy"]);
        assert_eq!(i.limit, 50);
        assert_eq!(i.stages(), [Stage::Companies]);
        assert!(i.people.is_empty());
        let capped = intent("Find 400 fintech companies in Vienna.");
        assert_eq!(capped.limit, MAX_LIMIT);
    }

    #[test]
    fn hiring_activity_and_team_building() {
        let i = detect("Find Vienna companies with at least three relevant open roles found during the last month.")
            .unwrap();
        assert_eq!(i.min_openings, Some(3));
        assert_eq!(i.posted_within_days, Some(30));
        assert!(i.stages().contains(&Stage::Jobs));
        let ai = detect(
            "Which Vienna companies currently appear to be building AI teams based on open roles?",
        )
        .unwrap();
        assert_eq!(ai.roles.first().map(String::as_str), Some("AI"));
        assert!(ai.roles.iter().any(|r| r == "Machine Learning Engineer"));
        assert!(ai.hiring);
        assert_eq!(ai.stages(), [Stage::Companies, Stage::Jobs]);
    }

    #[test]
    fn technology_searches_are_not_hard_coded() {
        let k8s = detect("Find companies hiring Kubernetes engineers and identify relevant technical/recruiting contacts.")
            .unwrap();
        assert_eq!(
            k8s.roles.first().map(String::as_str),
            Some("Kubernetes engineer")
        );
        assert!(k8s.technologies.iter().any(|t| t == "Kubernetes"));
        assert!(kinds(&k8s).contains(&PeopleKind::Recruiter));
        assert!(k8s
            .people
            .iter()
            .any(|p| p.function.as_deref() == Some("Engineering")));
        let sap = detect("Find companies using SAP that currently have SAP consulting positions, then identify relevant recruiting or practice-lead contacts.")
            .unwrap();
        assert!(sap.technologies.iter().any(|t| t == "SAP"));
        assert_eq!(
            sap.roles.first().map(String::as_str),
            Some("SAP consulting")
        );
        assert!(sap.people.iter().any(|p| p.label == "Practice lead"));
        let with = intent("Find companies hiring people with Salesforce experience in Vienna.");
        assert!(with.technologies.iter().any(|t| t == "Salesforce"));
    }

    #[test]
    fn people_and_relationship_requests() {
        let who = detect("I want to work at Nordlicht AI. Who are the five people I should know?")
            .unwrap();
        assert_eq!(who.target_company.as_deref(), Some("Nordlicht AI"));
        assert!(kinds(&who).contains(&PeopleKind::Any));
        assert_eq!(who.people_limit, Some(5));
        assert_eq!(who.stages(), [Stage::Companies, Stage::People]);

        let warm = detect("Do I know anyone at Nordlicht AI?").unwrap();
        assert!(warm.relationships);
        assert_eq!(warm.target_company.as_deref(), Some("Nordlicht AI"));
        assert_eq!(warm.stages(), [Stage::Companies, Stage::Connections]);

        let first = detect("Which of my permitted LinkedIn first-degree connections work at companies currently hiring for my target roles?")
            .unwrap();
        assert!(first.relationships && first.uses_profile && first.hiring);
        assert!(!first.second_degree);
        assert!(intent("Show me second-degree connections at Bitpanda.").second_degree);

        let recruiters = detect("Find recruiters in Vienna who work on AI/ML roles.").unwrap();
        assert!(kinds(&recruiters).contains(&PeopleKind::Recruiter));
        assert_eq!(recruiters.target_company, None);

        let job = detect("Here is a job: https://jobs.lever.co/donau/0f1e2d3c. Who are the most relevant people to know or contact?")
            .unwrap();
        assert_eq!(
            job.job_url.as_deref(),
            Some("https://jobs.lever.co/donau/0f1e2d3c")
        );
        assert!(job.stages().contains(&Stage::Jobs) && job.stages().contains(&Stage::People));

        let leads = detect("Find companies in Vienna building AI teams and show the relevant engineering or AI leaders.")
            .unwrap();
        let functions: Vec<_> = leads
            .people
            .iter()
            .filter_map(|p| p.function.clone())
            .collect();
        assert!(
            functions.contains(&"Engineering".to_string()) && functions.contains(&"AI".to_string()),
            "{functions:?}"
        );
    }

    #[test]
    fn job_search_and_single_facts_stay_with_their_routes() {
        for text in [
            "Find AI jobs in Vienna.",
            "Find me most recent AI jobs in Vienna Austria with salary starting from 85k a year.",
            "Find current recruiters at Nordlicht AI.",
            "Who is the CEO of Bitpanda?",
            "Explain what a hiring manager does.",
            "Write a cover letter for Bitpanda.",
        ] {
            assert!(detect(text).is_none(), "{text}");
        }
        let profile =
            detect("Find companies where my AI/data background would be relevant.").unwrap();
        assert!(profile.uses_profile);
    }
}
