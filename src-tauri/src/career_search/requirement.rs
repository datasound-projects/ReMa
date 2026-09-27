//! Whether a request needs current information, and of which kind.
//!
//! Deterministic, like the job-search detection it builds on: application
//! code decides that ReMa must search before answering, never a model.
//!
//! - **Required**: the answer depends on the state of the world now (current
//!   jobs, which companies are hiring, who holds a role now, current salary
//!   ranges). It may not come from a model's memory.
//! - **Optional**: search may help but the question is general ("companies
//!   generally known for AI infrastructure").
//! - **None**: writing, explaining, preparing.

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use specta::Type;

use crate::retrieval;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Requirement {
    None,
    Optional,
    Required,
}

/// What a request searches for (several can apply, §41): JOBS, COMPANIES,
/// PEOPLE, CONTRACTS, MARKET. Contract and freelance work also searches
/// jobs (the Jobs MCP lists both).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Scopes {
    pub jobs: bool,
    pub company: bool,
    pub people: bool,
    pub market: bool,
    pub contracts: bool,
}

impl Scopes {
    pub fn any(self) -> bool {
        self.jobs || self.company || self.people || self.market || self.contracts
    }

    /// "Jobs", "Company, People".
    pub fn names(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (on, name) in [
            (self.jobs, "Jobs"),
            (self.company, "Company"),
            (self.people, "People"),
            (self.contracts, "Contracts"),
            (self.market, "Market"),
        ] {
            if on {
                out.push(name);
            }
        }
        out
    }
}

/// The classification of one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Classified {
    pub requirement: Requirement,
    pub scopes: Scopes,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// Words that ask for the present state of things.
fn current_cue() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(current(?:ly)?|now|right now|today|this (?:week|month|year)|latest|newest|recent(?:ly)?|most recent|up[- ]to[- ]date|still|open|hiring|posted|aktuell(?:e|en)?|derzeit|neueste?n?)\b",
    )
}

/// Verbs that ask ReMa to go and look.
fn search_verb() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(find|search|look (?:up|for)|show|list|which|who|research|discover|identify|get me|give me|suche|finde|zeig)\b",
    )
}

/// Contracts and freelance work (a job search the job detector may miss).
fn contract_noun() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(contracts?|freelance|freelancer|gigs?|projects? for (?:a )?(?:freelancer|contractor)|interim (?:roles?|positions?)|contract (?:roles?|work|positions?))\b",
    )
}

/// Companies that hire, build or employ.
fn company_cue() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(compan(?:y|ies)|employers?|startups?|firms?|organi[sz]ations?|unternehmen|firmen)\b",
    )
}

/// People in roles, recruiters and contacts.
fn people_cue() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(recruiters?|talent acquisition|hiring managers?|head of|vp of|director of|chief [a-z]+ officer|ceo|cto|cfo|cpo|founders?|team leads?|engineering managers?|contacts?|people at|who (?:leads|runs|manages|is the))\b",
    )
}

/// Salary and market questions.
fn market_cue() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(salar(?:y|ies)(?: ranges?| bands?| benchmarks?| levels?)?|pay (?:ranges?|levels?)|compensation|how much (?:do|does|can) .{0,40}(?:earn|make|get paid)|job market|market (?:demand|rates?)|day rates?|hourly rates?|gehalt|gehälter)\b",
    )
}

/// General-knowledge phrasing that does not ask for today's state.
fn general_cue() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(generally|in general|typically|usually|known for|historically|explain|what is|what are|how does|how do)\b",
    )
}

/// Writing and preparation help: no search needed.
fn writing_cue() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    re(
        &CELL,
        r"(?i)\b(improve|rewrite|draft|write|edit|polish|translate|summari[sz]e (?:my|this)|prepare|practice|explain|cover letter|cv|resume|bullet)\b",
    )
}

/// Classifies a request.
pub fn classify(text: &str) -> Classified {
    let text = text.trim();
    let mut scopes = Scopes::default();
    if text.is_empty() {
        return Classified {
            requirement: Requirement::None,
            scopes,
        };
    }
    let job_search = retrieval::detect(text).is_some();
    let asks = search_verb().is_match(text);
    let current = current_cue().is_match(text);
    let contracts = contract_noun().is_match(text) && (asks || current || job_search);
    scopes.contracts = contracts;
    scopes.jobs = job_search || contracts;
    scopes.company = company_cue().is_match(text);
    scopes.people = people_cue().is_match(text);
    scopes.market = market_cue().is_match(text);
    // "Which companies are hiring AI Engineers?" is about jobs too.
    if scopes.company && text.to_lowercase().contains("hiring") {
        scopes.jobs = true;
    }

    let requirement = if scopes.jobs {
        Requirement::Required
    } else if !scopes.any() {
        Requirement::None
    } else if general_cue().is_match(text) && !current {
        Requirement::Optional
    } else if asks || current {
        Requirement::Required
    } else if writing_cue().is_match(text) {
        Requirement::None
    } else {
        Requirement::Optional
    };
    // Writing help that only mentions a company or a recruiter.
    let requirement = if requirement == Requirement::Required
        && !scopes.jobs
        && writing_cue().is_match(text)
        && !asks
    {
        Requirement::None
    } else {
        requirement
    };
    Classified {
        requirement,
        scopes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(text: &str, requirement: Requirement) -> Scopes {
        let classified = classify(text);
        assert_eq!(classified.requirement, requirement, "{text}");
        classified.scopes
    }

    #[test]
    fn static_questions_need_no_search() {
        for text in [
            "Explain what RAG is.",
            "Improve my CV bullet.",
            "Prepare interview questions.",
            "Draft a cover letter for a data engineer role.",
            "",
        ] {
            check(text, Requirement::None);
        }
    }

    #[test]
    fn general_company_knowledge_is_optional() {
        let scopes = check(
            "What companies are generally known for AI infrastructure?",
            Requirement::Optional,
        );
        assert!(scopes.company && !scopes.jobs);
    }

    #[test]
    fn current_career_questions_require_search() {
        let jobs = check("Find jobs in Vienna.", Requirement::Required);
        assert!(jobs.jobs);
        assert!(check("Find jobs posted this week.", Requirement::Required).jobs);
        assert!(
            check(
                "Find me most recent AI jobs in Vienna Austria with salary starting from 85k a year.",
                Requirement::Required
            )
            .jobs
        );
        let hiring = check(
            "Which companies are hiring AI Engineers?",
            Requirement::Required,
        );
        assert!(hiring.company && hiring.jobs);
        let recruiters = check(
            "Find current recruiters at Company X.",
            Requirement::Required,
        );
        assert!(recruiters.people && recruiters.company);
        assert!(
            check(
                "Find current Head of AI at Company X.",
                Requirement::Required
            )
            .people
        );
        let contracts = check(
            "Find short-term AI contracts in Austria.",
            Requirement::Required,
        );
        assert!(contracts.jobs && contracts.contracts);
        assert_eq!(contracts.names(), ["Jobs", "Contracts"]);
        assert!(check("Find B2B AI contracts in Austria.", Requirement::Required).contracts);
        // Scopes combine (§41).
        let combined = check(
            "Find Vienna fintech companies hiring AI Engineers and find the relevant recruiting \
             contact.",
            Requirement::Required,
        );
        assert!(combined.company && combined.jobs && combined.people);
        assert!(!check("Find AI Engineer jobs in Vienna.", Requirement::Required).contracts);
        assert!(check("Find current salary ranges.", Requirement::Required).market);
        let building = check(
            "Find companies currently building AI teams.",
            Requirement::Required,
        );
        assert!(building.company);
        assert_eq!(building.names(), ["Company"]);
    }
}
