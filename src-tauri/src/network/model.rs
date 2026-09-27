//! The professional graph as Network Connect returns it (NC §4, §19, §34,
//! §36, §37, §47): companies, jobs, people and permitted relationships,
//! each with its evidence, confidence and retrieval time. The same
//! structures go to the Network Connect page, to chat tools (after the
//! data policy) and to Business.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::policy::{DataClass, DataSource, Persistence};
use crate::models::connectors::ProviderId;

/// What a piece of evidence supports (NC §34 `supports`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Supports {
    CompanyIdentity,
    CompanyWebsite,
    CompanyLocation,
    CompanyIndustry,
    CompanySize,
    CompanyHiring,
    JobIsOpen,
    CurrentTitle,
    PersonRelevance,
    /// The posting names this person (as contact, recruiter or manager).
    NamedOnPosting,
    ProfileLink,
    Relationship,
}

impl Supports {
    pub fn label(self) -> &'static str {
        match self {
            Self::CompanyIdentity => "company",
            Self::CompanyWebsite => "website",
            Self::CompanyLocation => "location",
            Self::CompanyIndustry => "industry",
            Self::CompanySize => "size",
            Self::CompanyHiring => "current openings",
            Self::JobIsOpen => "the job is open",
            Self::CurrentTitle => "current title",
            Self::PersonRelevance => "relevance",
            Self::NamedOnPosting => "named on the posting",
            Self::ProfileLink => "profile link",
            Self::Relationship => "your connection",
        }
    }
}

/// One source for one claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub source: DataSource,
    /// "Company website", "Greenhouse", "Wikidata", "OpenAI web search".
    pub source_name: String,
    pub url: Option<String>,
    pub title: Option<String>,
    pub supports: Supports,
    /// A short excerpt (data from the source, never instructions; contact
    /// details removed).
    pub excerpt: Option<String>,
    pub retrieved_at: i64,
    pub published_at: Option<String>,
    /// ReMa read the source itself (or the search engine reported the
    /// page); false when only a model's summary names it.
    pub checked: bool,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
        }
    }
}

/// A company (NC §4, §15). Unknown values stay `None`: shown as "Unknown",
/// never guessed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Company {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub website: Option<String>,
    pub domain: Option<String>,
    pub industry: Option<String>,
    /// "about 1,200 employees".
    pub size: Option<String>,
    /// Employee count when a source states one.
    pub employees: Option<u32>,
    pub locations: Vec<String>,
    pub linkedin_url: Option<String>,
    pub xing_url: Option<String>,
    pub other_urls: Vec<String>,
    /// Why it matched the request, one reason per criterion.
    pub matched_because: Vec<String>,
    /// Criteria ReMa could not verify for this company ("size unknown").
    pub unverified: Vec<String>,
    /// Relevant openings observed in this search (a count, not a trend).
    pub relevant_openings: u32,
    pub evidence: Vec<Evidence>,
    pub last_verified_at: i64,
}

/// A job, as ReMa's job layer found it (the Jobs MCP record when it has
/// one; Network Connect keeps no job database of its own).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct JobRef {
    /// The Jobs MCP id ("rj_…") or "url:<canonical address>".
    pub id: String,
    pub title: String,
    pub company_id: Option<String>,
    pub company_name: Option<String>,
    pub location: Option<String>,
    pub work_mode: Option<String>,
    /// UTC midnight of the posting date.
    pub posted_at: Option<i64>,
    pub url: String,
    /// "Greenhouse", "Arbeitnow", "OpenAI web search".
    pub source: String,
    /// "Posting read", "Page read", "Found by search (not opened)".
    pub status: String,
    pub notes: Vec<String>,
}

/// Why a person is relevant (NC §18: "Hiring Manager" only with explicit
/// evidence).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum RelevanceType {
    /// The posting says this person is the hiring manager.
    HiringManager,
    /// The posting names this person as its recruiter or contact.
    NamedRecruiter,
    /// The posting says the role reports to this person.
    StatedManager,
    /// Leads the department the job or request belongs to.
    DepartmentLeader,
    /// Leads a team in that function.
    TeamLead,
    /// Recruits for the company (not tied to this job by a source).
    Recruiter,
    /// Company leadership.
    Executive,
    /// Relevant for another stated reason.
    RelevantContact,
}

impl RelevanceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::HiringManager => "Hiring manager (stated in the posting)",
            Self::NamedRecruiter => "Named contact on the posting",
            Self::StatedManager => "Manager of the role (stated in the posting)",
            Self::DepartmentLeader => "Likely relevant department leader",
            Self::TeamLead => "Possible team lead",
            Self::Recruiter => "Relevant recruiter",
            Self::Executive => "Company leadership",
            Self::RelevantContact => "Likely relevant contact",
        }
    }
}

/// A permitted relationship to the user (NC §4): only from a provider
/// that shares it, never inferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    pub provider: ProviderId,
    /// 1: a first-degree connection. Nothing else exists.
    pub degree: u8,
    /// "LinkedIn first-degree connection".
    pub label: String,
    pub fetched_at: i64,
    pub persistence: Persistence,
}

/// A person in a professional role (NC §19).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: String,
    pub name: String,
    pub title: Option<String>,
    pub company_id: Option<String>,
    pub company_name: Option<String>,
    pub location: Option<String>,
    pub linkedin_url: Option<String>,
    pub xing_url: Option<String>,
    pub other_url: Option<String>,
    pub relevance: RelevanceType,
    pub relevance_reason: String,
    /// The job this person is relevant to.
    pub job_id: Option<String>,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    pub relationship: Option<Relationship>,
    /// Where the person record comes from (drives the data policy).
    pub source: DataSource,
    pub class: DataClass,
    pub persistence: Persistence,
    pub fetched_at: i64,
    /// A caveat about freshness or identity ("title as listed on the team
    /// page"; "two people with this name").
    pub caveat: Option<String>,
}

/// A first-degree connection that matched a company (session only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub name: String,
    pub headline: Option<String>,
    pub company_id: Option<String>,
    pub company_name: Option<String>,
    pub profile_url: Option<String>,
    pub relationship: Relationship,
    /// How the company match was made ("their LinkedIn headline names …").
    pub match_reason: String,
}

/// What the relationship stage could do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ConnectionsOutcome {
    /// The request did not ask about relationships.
    NotRequested,
    /// No provider shares a connection list with ReMa (the reason says
    /// exactly what is missing; never "no connections").
    Unavailable {
        reason: String,
    },
    /// The connection list was checked.
    Checked {
        provider: ProviderId,
        checked: u32,
        matched: u32,
    },
    Failed {
        reason: String,
    },
}

/// One row of the unified table (NC §21, §29).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub company_id: Option<String>,
    pub job_id: Option<String>,
    pub person_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Companies,
    Jobs,
    People,
    Connections,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Companies => "Companies",
            Self::Jobs => "Jobs",
            Self::People => "People",
            Self::Connections => "Connections",
        }
    }
}

/// What one stage did (shown under the results).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StageReport {
    pub stage: Stage,
    /// "12 companies from job postings and Wikidata".
    pub summary: String,
    /// Sources that answered.
    pub sources: Vec<String>,
    /// Sources that failed, with the reason.
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Complete,
    /// Some sources or stages failed; what was found is shown.
    Partial,
    /// Searches ran; nothing verifiable matched.
    NoVerifiedMatches,
    /// No source could be searched.
    Failed,
    Cancelled,
}

/// The criteria ReMa read from the request, shown so the user can see
/// what was searched.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Criteria {
    pub locations: Vec<String>,
    pub industries: Vec<String>,
    /// "50–500 employees".
    pub company_size: Option<String>,
    pub technologies: Vec<String>,
    pub roles: Vec<String>,
    pub seniority: Option<String>,
    pub min_openings: Option<u32>,
    pub posted_within_days: Option<u32>,
    pub people: Vec<String>,
    pub target_company: Option<String>,
    pub relationships: bool,
    pub uses_profile: bool,
    pub limit: u32,
    pub stages: Vec<Stage>,
}

/// One research answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetworkResult {
    pub query: String,
    pub criteria: Criteria,
    pub status: ResultStatus,
    pub companies: Vec<Company>,
    pub jobs: Vec<JobRef>,
    pub people: Vec<Person>,
    /// Session only: never stored, never sent to a model.
    pub connections: Vec<Connection>,
    pub connections_outcome: ConnectionsOutcome,
    pub rows: Vec<Row>,
    pub stages: Vec<StageReport>,
    /// What the user should know, such as how many companies could not be
    /// verified against a criterion.
    pub notes: Vec<String>,
    pub retrieved_at: i64,
    pub policy_version: String,
}

impl NetworkResult {
    pub fn company(&self, id: &str) -> Option<&Company> {
        self.companies.iter().find(|c| c.id == id)
    }

    pub fn job(&self, id: &str) -> Option<&JobRef> {
        self.jobs.iter().find(|j| j.id == id)
    }

    pub fn person(&self, id: &str) -> Option<&Person> {
        self.people.iter().find(|p| p.id == id)
    }
}

/// A status line of a running Network Connect request (for the page).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct NetworkProgress {
    pub run_id: String,
    pub text: String,
}

/// A request from the Network Connect page.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetworkResearchInput {
    /// Chosen by the page, to cancel the request.
    pub run_id: String,
    pub query: String,
    /// A job the request is about.
    pub job_url: Option<String>,
    /// A company the request is about.
    pub company: Option<String>,
}

/// The page's last result this session, if any.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LastNetworkResult {
    pub result: Option<NetworkResult>,
}
