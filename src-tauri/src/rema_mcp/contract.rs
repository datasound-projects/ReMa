//! ReMa MCP's data and tool contracts (schema version 1).
//!
//! The detailed [`JobRecord`] keeps evidence, conflicts and completeness;
//! the compact [`JobSummary`] is what `search_jobs` returns. Unknown values
//! are `null` (never "", 0 or a default work mode), and an empty list never
//! means a source confirmed there is nothing: see [`Description::lists`].
//! Tool inputs derive JSON Schemas (`schemars`) with bounds; the server
//! validates them again before doing any work.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

pub const DEFAULT_LIMIT: u32 = 20;
pub const MAX_LIMIT: u32 = 50;
pub const MAX_BATCH: usize = 10;

// ── Enumerations ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkMode {
    Remote,
    Hybrid,
    Onsite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Seniority {
    Intern,
    Entry,
    Mid,
    Senior,
    Lead,
    Executive,
}

/// The contract type (permanent, fixed-term, …), separate from working time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmploymentType {
    Permanent,
    Contract,
    Temporary,
    Freelance,
    Internship,
    Apprenticeship,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkingTime {
    FullTime,
    PartTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SalaryPeriod {
    Year,
    Month,
    Week,
    Day,
    Hour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SalaryBasis {
    Gross,
    Net,
    Unknown,
}

/// How the source states the amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SalaryBound {
    /// Both a lower and an upper bound.
    Range,
    /// A minimum ("from €65,000", "ab 4.000 €"): not a complete range.
    Floor,
    /// A maximum only ("up to …").
    Ceiling,
    /// One stated amount.
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// An authoritative published-job response or an explicit open state.
    Active,
    /// An explicit closed notice or an expired `validThrough`.
    Closed,
    /// The source no longer serves the posting (404/410).
    Unavailable,
    /// Not established (timeouts, refusals and HTTP 200 alone prove nothing).
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DescriptionState {
    Full,
    Partial,
    SnippetOnly,
    Missing,
}

/// Whether requirement/benefit lists were extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListsStatus {
    /// Sections were recognized and their items extracted.
    Extracted,
    /// A description exists but has no recognizable sections: empty lists
    /// do not mean the job has no requirements.
    NoSectionsFound,
    /// No description was available.
    NoDescription,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionMode {
    AuthorizedApi,
    DocumentedPublicFeed,
    PermittedPublicPage,
    SearchDiscoveryOnly,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DatePrecision {
    /// A calendar date stated by the source.
    Day,
    /// Derived from a relative age ("3 days ago") at `posted_reference`.
    Relative,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterMode {
    /// A required field that is unknown is not a match.
    #[default]
    Strict,
    /// Unknown values pass, with a note.
    Lenient,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SortMode {
    #[default]
    Relevance,
    Newest,
    SalaryAscending,
    SalaryDescending,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkillsMode {
    #[default]
    All,
    Any,
}

/// Domain error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidInput,
    SearchBackendUnavailable,
    SourceNotAuthorized,
    BlockedBySourcePolicy,
    RateLimited,
    Timeout,
    Cancelled,
    SourceUnavailable,
    ParsingFailed,
    NotAJobPosting,
    JobNotFound,
    JobExpired,
    StaleCache,
    PayloadLimitReached,
    CursorExpired,
    PartialResult,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "INVALID_INPUT",
            Self::SearchBackendUnavailable => "SEARCH_BACKEND_UNAVAILABLE",
            Self::SourceNotAuthorized => "SOURCE_NOT_AUTHORIZED",
            Self::BlockedBySourcePolicy => "BLOCKED_BY_SOURCE_POLICY",
            Self::RateLimited => "RATE_LIMITED",
            Self::Timeout => "TIMEOUT",
            Self::Cancelled => "CANCELLED",
            Self::SourceUnavailable => "SOURCE_UNAVAILABLE",
            Self::ParsingFailed => "PARSING_FAILED",
            Self::NotAJobPosting => "NOT_A_JOB_POSTING",
            Self::JobNotFound => "JOB_NOT_FOUND",
            Self::JobExpired => "JOB_EXPIRED",
            Self::StaleCache => "STALE_CACHE",
            Self::PayloadLimitReached => "PAYLOAD_LIMIT_REACHED",
            Self::CursorExpired => "CURSOR_EXPIRED",
            Self::PartialResult => "PARTIAL_RESULT",
        }
    }

    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::Timeout | Self::SourceUnavailable | Self::CursorExpired
        )
    }
}

// ── Detailed record ───────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Employer {
    pub name: Option<String>,
    pub website: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SourceId {
    /// Registry id ("greenhouse", "lever", "linkedin", …).
    pub source: String,
    /// The source's own id ("board:job" for employer-scoped sources).
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Location {
    /// As the source wrote it.
    pub text: String,
    pub city: Option<String>,
    pub region: Option<String>,
    /// Country name (English) when stated or implied by a known city.
    pub country: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Compensation {
    /// As the source wrote it (or ReMa's rendering of structured fields).
    pub text: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub currency: Option<String>,
    pub period: Option<SalaryPeriod>,
    pub basis: SalaryBasis,
    /// `null` when no amount could be read (e.g. an estimate).
    pub bound: Option<SalaryBound>,
    /// The source labels it an estimate: never used for strict filters.
    pub estimate: bool,
    /// Bonus, equity or commission is mentioned (not included in min/max).
    pub variable_pay_mentioned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Dates {
    /// The original posting date (never an index, update or first-seen date).
    pub posted_at: Option<String>,
    pub posted_precision: Option<DatePrecision>,
    /// For relative dates: when the relative age was read.
    pub posted_reference: Option<String>,
    /// The source's last update of the posting.
    pub updated_at: Option<String>,
    /// When ReMa first saw this job.
    pub first_seen_at: String,
    /// When this record's content was retrieved from its source.
    pub retrieved_at: Option<String>,
    /// When availability was last actually checked.
    pub last_checked_at: Option<String>,
    pub valid_through: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    About,
    Responsibilities,
    Requirements,
    Preferred,
    Benefits,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Section {
    pub heading: String,
    pub kind: SectionKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Description {
    /// Clean text (data from the source: never instructions).
    pub text: Option<String>,
    pub sections: Vec<Section>,
    pub requirements: Vec<String>,
    pub preferred: Vec<String>,
    /// Skills named in the description (dictionary matches).
    pub skills: Vec<String>,
    pub benefits: Vec<String>,
    pub state: DescriptionState,
    pub lists: ListsStatus,
    /// The source text was longer than ReMa keeps.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct DiscoveredLink {
    pub source: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Links {
    /// Where searches found it (LinkedIn, XING, boards, search results).
    pub discovered: Vec<DiscoveredLink>,
    /// The posting on the employer's own site or ATS, when resolved.
    pub canonical_url: Option<String>,
    pub employer_url: Option<String>,
    /// Only when the source states it.
    pub apply_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    /// The record field this supports ("dates.posted_at", "compensation").
    pub field: String,
    pub source: String,
    pub url: String,
    /// "ats_api", "ats_feed", "json_ld", "page_text", "search_result".
    pub method: String,
    /// Where in the source ("first_published", "JobPosting.datePosted").
    pub path: Option<String>,
    pub retrieved_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConflictValue {
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Conflict {
    pub field: String,
    pub values: Vec<ConflictValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Quality {
    pub availability: Availability,
    /// What the availability rests on.
    pub availability_basis: Option<String>,
    pub acquisition: AcquisitionMode,
    /// Important fields the source did not state.
    pub missing: Vec<String>,
    pub conflicts: Vec<Conflict>,
    /// Fields derived by ReMa rather than stated (e.g. "language").
    pub inferred: Vec<String>,
    /// Other ReMa ids that may be the same vacancy (not merged).
    pub possible_duplicates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobRecord {
    pub schema_version: u32,
    /// Stable ReMa id ("rj_…").
    pub id: String,
    /// The original title.
    pub title: String,
    /// Lower-case title without gender markers and seniority words.
    pub normalized_title: Option<String>,
    /// ISO 639-1 language of the posting.
    pub language: Option<String>,
    pub employer: Employer,
    pub source_ids: Vec<SourceId>,
    pub locations: Vec<Location>,
    pub work_mode: Option<WorkMode>,
    /// Explicit applicant-location requirements (as stated). Empty: not
    /// stated, so remote eligibility from a given country is unknown.
    pub remote_eligibility: Vec<String>,
    pub seniority: Option<Seniority>,
    pub employment_type: Option<EmploymentType>,
    pub working_time: Option<WorkingTime>,
    pub compensation: Option<Compensation>,
    pub dates: Dates,
    pub description: Description,
    pub links: Links,
    pub evidence: Vec<Evidence>,
    pub quality: Quality,
}

// ── Compact output ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SalarySummary {
    pub text: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub currency: Option<String>,
    pub period: Option<SalaryPeriod>,
    pub bound: Option<SalaryBound>,
    pub estimate: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Relevance {
    /// A deterministic ranking heuristic (0–100), not a probability of fit.
    pub score: u32,
    pub factors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobSummary {
    pub id: String,
    pub title: String,
    pub employer: Option<String>,
    pub locations: Vec<String>,
    pub work_mode: Option<WorkMode>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub remote_eligibility: Vec<String>,
    pub seniority: Option<Seniority>,
    pub employment_type: Option<EmploymentType>,
    pub working_time: Option<WorkingTime>,
    pub salary: Option<SalarySummary>,
    pub posted_at: Option<String>,
    pub last_checked_at: Option<String>,
    pub availability: Availability,
    pub description_state: DescriptionState,
    /// The canonical posting, or the discovered link when unresolved.
    pub url: String,
    pub source: String,
    pub acquisition: AcquisitionMode,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub discovered_from: Vec<DiscoveredLink>,
    /// Facts come only from a search result or are incomplete.
    pub needs_verification: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub notes: Vec<String>,
    pub relevance: Relevance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Warning {
    pub code: ErrorCode,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceIssue {
    pub source: String,
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Coverage {
    /// The search backend used for discovery.
    pub backend: Option<String>,
    pub backend_queries: u32,
    pub candidates_seen: u32,
    pub details_fetched: u32,
    pub sources_searched: Vec<String>,
    pub sources_unavailable: Vec<SourceIssue>,
    /// Why coverage stopped: "sources", "permissions", "query_budget",
    /// "fetch_budget", "deadline", "result_limit".
    pub bounded_by: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Excluded {
    pub filtered_out: u32,
    pub unresolved: u32,
    pub closed_or_unavailable: u32,
    pub duplicates: u32,
    pub not_job_postings: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SimilarBasis {
    pub seed_id: String,
    pub title: String,
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchResult {
    pub schema_version: u32,
    pub query: String,
    pub applied_filters: SearchFilters,
    /// Filters that could not be checked for some candidates (unknown
    /// values): those candidates were excluded (strict) or noted (lenient).
    pub unresolved_filters: Vec<String>,
    pub jobs: Vec<JobSummary>,
    /// Only when `include_unresolved` was set.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unresolved_candidates: Vec<JobSummary>,
    /// Jobs in this response (not an estimate of all matching jobs).
    pub total_returned: u32,
    pub excluded: Excluded,
    pub coverage: Coverage,
    pub warnings: Vec<Warning>,
    pub partial: bool,
    pub searched_at: String,
    pub snapshot_expires_at: String,
    pub from_cache: bool,
    pub next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub similar_to: Option<SimilarBasis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DescriptionPart {
    pub offset: u32,
    pub length: u32,
    pub total_length: u32,
    /// Pass as `description_cursor` to read on.
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobDetail {
    pub schema_version: u32,
    pub job: JobRecord,
    pub description_part: DescriptionPart,
    pub from_cache: bool,
    /// A refresh failed; the record is older than requested.
    pub stale: bool,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BatchItem {
    pub reference: JobRef,
    pub ok: bool,
    pub detail: Option<JobDetail>,
    pub error: Option<ToolErrorBody>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BatchResult {
    pub schema_version: u32,
    /// In input order.
    pub results: Vec<BatchItem>,
    pub succeeded: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackendStatus {
    /// "search_service", "provider_web_search" or "none".
    pub kind: String,
    pub name: Option<String>,
    pub usable: bool,
    pub detail: String,
    pub deadline_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceState {
    pub id: String,
    pub name: String,
    pub acquisition: AcquisitionMode,
    pub discovery: String,
    pub content: String,
    pub implemented: bool,
    pub usable: bool,
    pub needs_credentials: bool,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub last_error_at: Option<String>,
    pub limitations: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceStatusResult {
    pub schema_version: u32,
    pub checked_at: String,
    pub search_backend: BackendStatus,
    pub sources: Vec<SourceState>,
}

// ── Tool inputs ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocationFilter {
    #[schemars(length(max = 100))]
    pub city: Option<String>,
    #[schemars(length(max = 100))]
    pub region: Option<String>,
    /// Country name or ISO 3166 alpha-2 code.
    #[schemars(length(max = 60))]
    pub country: Option<String>,
}

/// One job: a ReMa id from an earlier result, or a job URL.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobRef {
    #[schemars(length(max = 40))]
    pub id: Option<String>,
    #[schemars(length(max = 2000))]
    pub url: Option<String>,
}

/// The filters of a search, as applied (echoed in results).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchFilters {
    pub required_skills: Vec<String>,
    pub required_skills_mode: SkillsMode,
    pub skills: Vec<String>,
    pub exclude_terms: Vec<String>,
    pub companies: Vec<String>,
    pub locations: Vec<LocationFilter>,
    pub work_modes: Vec<WorkMode>,
    pub seniority: Vec<Seniority>,
    pub employment_types: Vec<EmploymentType>,
    pub working_time: Vec<WorkingTime>,
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    pub salary_currency: Option<String>,
    pub salary_period: Option<SalaryPeriod>,
    pub posted_within_days: Option<u32>,
    pub languages: Vec<String>,
    pub sources: Vec<String>,
    pub filter_mode: FilterMode,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchJobsInput {
    /// Job title or keywords, as the user wrote them.
    #[schemars(length(min = 1, max = 200))]
    pub query: String,
    /// Mandatory skills (filter); see `required_skills_mode`.
    #[serde(default)]
    #[schemars(length(max = 15))]
    pub required_skills: Vec<String>,
    #[serde(default)]
    pub required_skills_mode: Option<SkillsMode>,
    /// Preferred skills (ranking only).
    #[serde(default)]
    #[schemars(length(max = 15))]
    pub skills: Vec<String>,
    /// Terms that exclude a job (title or description).
    #[serde(default)]
    #[schemars(length(max = 15))]
    pub exclude_terms: Vec<String>,
    /// Employers (any of them).
    #[serde(default)]
    #[schemars(length(max = 10))]
    pub companies: Vec<String>,
    /// Places (any of them).
    #[serde(default)]
    #[schemars(length(max = 10))]
    pub locations: Vec<LocationFilter>,
    #[serde(default)]
    pub work_modes: Vec<WorkMode>,
    #[serde(default)]
    pub seniority: Vec<Seniority>,
    #[serde(default)]
    pub employment_types: Vec<EmploymentType>,
    #[serde(default)]
    pub working_time: Vec<WorkingTime>,
    /// Minimum salary: the job's stated lower bound must reach it (same
    /// currency and period; no conversions).
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    /// ISO 4217 code (required with salary_min/salary_max).
    #[schemars(length(min = 3, max = 3))]
    pub salary_currency: Option<String>,
    /// Required with salary_min/salary_max.
    pub salary_period: Option<SalaryPeriod>,
    #[schemars(range(min = 1, max = 365))]
    pub posted_within_days: Option<u32>,
    /// Posting languages (ISO 639-1: "en", "de", "pl").
    #[serde(default)]
    #[schemars(length(max = 5))]
    pub languages: Vec<String>,
    /// Discovery surfaces ("web", "linkedin", "xing", "greenhouse", …).
    /// Omitted: all configured, permitted surfaces.
    #[serde(default)]
    #[schemars(length(max = 20))]
    pub sources: Vec<String>,
    pub filter_mode: Option<FilterMode>,
    /// Also return candidates whose required fields are unknown.
    pub include_unresolved: Option<bool>,
    #[schemars(range(min = 1, max = 50))]
    pub limit: Option<u32>,
    pub sort: Option<SortMode>,
    /// Search again instead of using a cached snapshot.
    pub refresh: Option<bool>,
    /// `next_cursor` from an earlier result (same query and filters).
    #[schemars(length(max = 400))]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetJobInput {
    /// A ReMa job id ("rj_…"). Give either `id` or `url`.
    #[schemars(length(max = 40))]
    pub id: Option<String>,
    /// A job posting URL.
    #[schemars(length(max = 2000))]
    pub url: Option<String>,
    /// Check the source again instead of using the cache.
    pub refresh: Option<bool>,
    /// `description_part.next_cursor` from an earlier result.
    #[schemars(length(max = 200))]
    pub description_cursor: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetJobsInput {
    /// Up to 10 jobs; results keep this order.
    #[schemars(length(min = 1, max = 10))]
    pub jobs: Vec<JobRef>,
    pub refresh: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SimilarJobsInput {
    /// The job to find similar ones to.
    pub seed: JobRef,
    #[serde(default)]
    #[schemars(length(max = 10))]
    pub locations: Vec<LocationFilter>,
    #[serde(default)]
    pub work_modes: Vec<WorkMode>,
    #[serde(default)]
    pub seniority: Vec<Seniority>,
    #[schemars(range(min = 1, max = 365))]
    pub posted_within_days: Option<u32>,
    pub salary_min: Option<f64>,
    #[schemars(length(min = 3, max = 3))]
    pub salary_currency: Option<String>,
    pub salary_period: Option<SalaryPeriod>,
    #[serde(default)]
    #[schemars(length(max = 20))]
    pub sources: Vec<String>,
    pub filter_mode: Option<FilterMode>,
    #[schemars(range(min = 1, max = 50))]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceStatusInput {
    /// Source ids to report (default: all).
    #[serde(default)]
    #[schemars(length(max = 30))]
    pub sources: Vec<String>,
}

/// A failed tool call.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
}

impl ToolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn body(&self) -> ToolErrorBody {
        ToolErrorBody {
            code: self.code,
            message: self.message.clone(),
            retryable: self.code.retryable(),
        }
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}
