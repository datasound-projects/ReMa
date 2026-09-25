//! Job analytics: the structured job model, the analytical query (scope,
//! filters, ranking) and the deterministic views computed from it.
//!
//! Every number in these views is calculated in Rust from stored job
//! records. Values that are not known stay `None`; they are never filled
//! with guesses or zeros.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::text_enum;

// ── Structured job fields ─────────────────────────────────────────────

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkMode {
    Remote,
    Hybrid,
    Onsite,
}

text_enum!(WorkMode { Remote => "remote", Hybrid => "hybrid", Onsite => "onsite" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EmploymentType {
    FullTime,
    PartTime,
    Contract,
    Freelance,
    Temporary,
    Internship,
}

text_enum!(EmploymentType {
    FullTime => "full_time",
    PartTime => "part_time",
    Contract => "contract",
    Freelance => "freelance",
    Temporary => "temporary",
    Internship => "internship",
});

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Seniority {
    Intern,
    Entry,
    Mid,
    Senior,
    Lead,
    Executive,
}

text_enum!(Seniority {
    Intern => "intern",
    Entry => "entry",
    Mid => "mid",
    Senior => "senior",
    Lead => "lead",
    Executive => "executive",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SalaryPeriod {
    Year,
    Month,
    Week,
    Day,
    Hour,
}

text_enum!(SalaryPeriod {
    Year => "year",
    Month => "month",
    Week => "week",
    Day => "day",
    Hour => "hour",
});

/// What kind of requirement a job states.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum RequirementKind {
    Skill,
    Experience,
    Education,
    Certification,
    Language,
    SoftSkill,
    Other,
}

text_enum!(RequirementKind {
    Skill => "skill",
    Experience => "experience",
    Education => "education",
    Certification => "certification",
    Language => "language",
    SoftSkill => "soft_skill",
    Other => "other",
});

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum RequirementCategory {
    TechnicalSkills,
    ProgrammingLanguages,
    Frameworks,
    CloudInfrastructure,
    AiMl,
    DataEngineering,
    Databases,
    ProfessionalExperience,
    IndustryExperience,
    Education,
    Certifications,
    Languages,
    SoftSkills,
    Other,
}

text_enum!(RequirementCategory {
    TechnicalSkills => "technical_skills",
    ProgrammingLanguages => "programming_languages",
    Frameworks => "frameworks",
    CloudInfrastructure => "cloud_infrastructure",
    AiMl => "ai_ml",
    DataEngineering => "data_engineering",
    Databases => "databases",
    ProfessionalExperience => "professional_experience",
    IndustryExperience => "industry_experience",
    Education => "education",
    Certifications => "certifications",
    Languages => "languages",
    SoftSkills => "soft_skills",
    Other => "other",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Importance {
    Required,
    Preferred,
}

text_enum!(Importance { Required => "required", Preferred => "preferred" });

/// Where a requirement was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RequirementSource {
    /// A column of the search result (e.g. "Key skills").
    Search,
    /// Found deterministically in the job description.
    Description,
    /// Extracted by the model and validated against the description.
    Model,
}

text_enum!(RequirementSource {
    Search => "search",
    Description => "description",
    Model => "model",
});

/// How a search run reached ReMa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunSource {
    Chat,
    Task,
    Manual,
    Tool,
}

text_enum!(RunSource { Chat => "chat", Task => "task", Manual => "manual", Tool => "tool" });

/// Progress of the background job-details reader for one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DetailsStatus {
    Pending,
    Done,
    Failed,
    Skipped,
}

text_enum!(DetailsStatus {
    Pending => "pending",
    Done => "done",
    Failed => "failed",
    Skipped => "skipped",
});

/// How the user's Profile relates to one requirement.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum MatchState {
    Matched,
    Partial,
    Missing,
    /// The Profile neither shows nor rules out the requirement.
    Unknown,
}

/// One cell of the job × skill matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CellState {
    Matched,
    Partial,
    Missing,
    Unknown,
    NotRequired,
    /// ReMa has no requirement data for this job.
    NoData,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum FrequencyClass {
    VeryCommon,
    Common,
    Occasional,
    Rare,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum PriorityLevel {
    High,
    Medium,
    Low,
}

// ── Analytical query ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    /// Every stored job search (the default).
    #[default]
    All,
    Searches,
    Jobs,
}

/// Which stored jobs are analyzed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DataScope {
    pub kind: ScopeKind,
    /// For `Searches`.
    pub run_ids: Vec<i64>,
    /// For `Jobs`.
    pub job_ids: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FilterField {
    Company,
    Title,
    /// The normalized role (title without seniority and gender markers).
    Role,
    Location,
    Country,
    City,
    Source,
    WorkMode,
    EmploymentType,
    Seniority,
    Salary,
    DatePosted,
    DateDiscovered,
    /// A requirement the job states (skill, technology, …).
    Skill,
    /// A requirement the job states that the Profile does not cover.
    MissingSkill,
    /// Profile match in percent.
    Match,
    /// Number of requirements missing from the Profile.
    SkillGap,
    Language,
    Certification,
    SearchRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    /// Equal to any of `values`.
    Is,
    /// Equal to none of `values` (unknown values are kept).
    IsNot,
    Contains,
    NotContains,
    AtLeast,
    AtMost,
    WithinDays,
    OlderThanDays,
    /// `values[0]` is a date, `YYYY-MM-DD`.
    OnOrAfter,
    Before,
    HasAny,
    HasAll,
    HasNone,
    /// The value is known (e.g. "has a salary").
    Known,
    Unknown,
}

/// One condition. All conditions must hold (AND); `values` are
/// alternatives within a condition (OR).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FilterCondition {
    pub field: FilterField,
    pub op: FilterOp,
    #[serde(default)]
    pub values: Vec<String>,
    /// Threshold for numeric operators (salary, days, percent, count).
    #[serde(default)]
    pub number: Option<f64>,
    /// Currency of a salary threshold (ISO code, e.g. "EUR").
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    /// Remote → hybrid → on-site.
    WorkMode,
    Salary,
    Match,
    /// Number of missing requirements.
    SkillGap,
    DatePosted,
    DateDiscovered,
    Company,
    Title,
    Seniority,
    /// Jobs in `value` (a country) first.
    PreferCountry,
    PreferCity,
    /// Jobs whose title contains `value` first.
    PreferRole,
    PreferCompany,
    /// Jobs requiring the skill `value` first.
    PreferSkill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Asc,
    Desc,
}

/// One ranking priority. Criteria apply in order; unknown values always
/// rank after known ones.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SortCriterion {
    pub key: SortKey,
    pub direction: SortDirection,
    #[serde(default)]
    pub value: Option<String>,
}

/// The analytical dataset: scope → filters → ranking → optional top N.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsQuery {
    pub scope: DataScope,
    #[serde(default)]
    pub filters: Vec<FilterCondition>,
    #[serde(default)]
    pub ranking: Vec<SortCriterion>,
    /// Keep only the first N jobs of the ranking.
    #[serde(default)]
    pub limit: Option<u32>,
}

// ── Preferences ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum JobColumn {
    Rank,
    Company,
    Role,
    Location,
    WorkMode,
    Salary,
    Seniority,
    Match,
    SkillGap,
    Posted,
    Discovered,
    Source,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DashboardTab {
    #[default]
    Jobs,
    SkillGap,
    Requirements,
    Learning,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillGapOptions {
    /// Higher-ranked jobs count more (1.0 for the first, 0.5 for the last).
    pub weight_by_rank: bool,
}

/// Which gaps are worth researching learning resources for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LearningCriteria {
    /// Only gaps required by at least this share of the jobs (0–100).
    pub min_percent: f64,
    /// Ignore requirements that appear in a single job.
    pub ignore_single: bool,
    pub include_partial: bool,
    /// At most this many gaps.
    pub max_gaps: u32,
    /// Only these requirements (empty: all).
    #[serde(default)]
    pub focus: Vec<String>,
}

impl Default for LearningCriteria {
    fn default() -> Self {
        Self {
            min_percent: 0.0,
            ignore_single: true,
            include_partial: true,
            max_gaps: 8,
            focus: Vec::new(),
        }
    }
}

/// The dashboard's working state, restored when it opens again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsPreferences {
    pub query: AnalyticsQuery,
    pub columns: Vec<JobColumn>,
    pub tab: DashboardTab,
    pub gap: SkillGapOptions,
    pub learning: LearningCriteria,
    /// Read job pages in the background for descriptions and details.
    pub read_pages: bool,
    /// Let the default model extract requirements from descriptions.
    pub use_model: bool,
}

// ── Search runs ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct JobSearchRun {
    pub id: i64,
    pub title: String,
    pub query: String,
    pub source: RunSource,
    pub task_id: Option<i64>,
    pub conversation_id: Option<i64>,
    pub created_at: i64,
    /// Distinct jobs in this run.
    pub result_count: u32,
}

// ── Overview (jobs table) ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SalaryCoverage {
    /// Jobs stating a salary.
    pub known: u32,
    /// Jobs whose salary can be compared (reference currency, yearly or monthly).
    pub comparable: u32,
    /// Currency salary comparisons use.
    pub currency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DatasetSummary {
    /// Human description of scope, filters and limit.
    pub label: String,
    pub in_scope: u32,
    /// After filters.
    pub matching: u32,
    /// After the optional top-N limit: the analyzed jobs.
    pub analyzed: u32,
    pub runs: u32,
    /// Search results that pointed to an already known job.
    pub duplicates_merged: u32,
    pub posted_known: u32,
    pub requirements_known: u32,
    pub salary: SalaryCoverage,
    /// Explanations (missing data, ignored conditions, …).
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RankedJob {
    pub rank: u32,
    pub id: i64,
    pub title: String,
    pub company: Option<String>,
    pub location: Option<String>,
    pub work_mode: Option<WorkMode>,
    pub employment_type: Option<EmploymentType>,
    pub seniority: Option<Seniority>,
    /// Salary as ReMa understood it, e.g. "€90k–110k / year".
    pub salary: Option<String>,
    pub salary_comparable: bool,
    pub match_percent: Option<f64>,
    pub missing: Option<u32>,
    pub date_posted: Option<i64>,
    pub date_discovered: i64,
    pub source: Option<String>,
    pub url: Option<String>,
    /// Search runs that found this job.
    pub appearances: u32,
    pub details: DetailsStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FacetValue {
    pub value: String,
    pub count: u32,
}

/// Values present in the scoped data, for filter suggestions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Facets {
    pub companies: Vec<FacetValue>,
    pub roles: Vec<FacetValue>,
    pub countries: Vec<FacetValue>,
    pub cities: Vec<FacetValue>,
    pub sources: Vec<FacetValue>,
    pub skills: Vec<FacetValue>,
    pub languages: Vec<FacetValue>,
    pub certifications: Vec<FacetValue>,
    pub currencies: Vec<FacetValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStatus {
    pub jobs: u32,
    pub runs: u32,
    /// Jobs whose details are still being read.
    pub pending_details: u32,
    pub read_pages: bool,
    pub use_model: bool,
    /// The model that reads descriptions, if one is set up.
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsOverview {
    pub summary: DatasetSummary,
    pub jobs: Vec<RankedJob>,
    /// Rows beyond `jobs` that were not sent.
    pub more: u32,
    pub facets: Facets,
    pub status: PipelineStatus,
}

// ── Skill gap ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CoverageCounts {
    pub requirements: u32,
    pub matched: u32,
    pub partial: u32,
    pub missing: u32,
    pub unknown: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillDemand {
    pub name: String,
    pub kind: RequirementKind,
    pub category: RequirementCategory,
    /// Jobs requiring it.
    pub jobs: u32,
    /// Share of jobs with requirement data (0–100).
    pub percent: f64,
    pub state: Option<MatchState>,
    /// Why the state was chosen (e.g. "Skills: Kubernetes").
    pub evidence: Option<String>,
}

/// Share of all requirement mentions per Profile state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StateShare {
    pub state: MatchState,
    pub requirements: u32,
    pub mentions: u32,
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GapPriority {
    pub name: String,
    pub kind: RequirementKind,
    pub category: RequirementCategory,
    pub state: MatchState,
    pub score: f64,
    pub level: PriorityLevel,
    pub jobs: u32,
    pub percent: f64,
    /// Facts behind the priority, e.g. "Required by 22 of 35 jobs (63%)".
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MatrixRow {
    pub job_id: i64,
    pub title: String,
    pub company: Option<String>,
    pub coverage: Option<f64>,
    pub cells: Vec<CellState>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GapMatrix {
    pub skills: Vec<String>,
    /// Per skill: analyzed jobs where it is a gap (missing or partial).
    pub gap_jobs: Vec<u32>,
    pub rows: Vec<MatrixRow>,
    /// Jobs not shown as rows.
    pub more_rows: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RequirementState {
    pub name: String,
    pub kind: RequirementKind,
    pub category: RequirementCategory,
    pub importance: Importance,
    pub state: MatchState,
    pub evidence: Option<String>,
    pub original: String,
}

/// Skill gap of a single job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct JobGap {
    pub job_id: i64,
    pub title: String,
    pub company: Option<String>,
    pub coverage: Option<f64>,
    pub counts: CoverageCounts,
    pub requirements: Vec<RequirementState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillGapView {
    pub profile_available: bool,
    pub jobs: u32,
    pub jobs_with_requirements: u32,
    pub average_coverage: Option<f64>,
    /// Summed over the analyzed jobs.
    pub counts: CoverageCounts,
    pub demand: Vec<SkillDemand>,
    pub coverage: Vec<StateShare>,
    pub matrix: GapMatrix,
    pub priorities: Vec<GapPriority>,
    /// Present when exactly one job is analyzed.
    pub job: Option<JobGap>,
    pub summary: Vec<String>,
    pub notes: Vec<String>,
}

// ── Unique requirements ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RequirementSort {
    Jobs,
    Name,
    Category,
    State,
}

/// Table controls; applied in Rust.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RequirementTableQuery {
    pub search: String,
    pub categories: Vec<RequirementCategory>,
    pub classes: Vec<FrequencyClass>,
    pub states: Vec<MatchState>,
    pub sort: RequirementSort,
    pub direction: SortDirection,
}

impl Default for RequirementTableQuery {
    fn default() -> Self {
        Self {
            search: String::new(),
            categories: Vec::new(),
            classes: Vec::new(),
            states: Vec::new(),
            sort: RequirementSort::Jobs,
            direction: SortDirection::Desc,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RequirementRow {
    pub name: String,
    pub kind: RequirementKind,
    pub category: RequirementCategory,
    pub jobs: u32,
    pub percent: f64,
    pub class: FrequencyClass,
    pub required: u32,
    pub preferred: u32,
    pub state: Option<MatchState>,
    /// Most common level (languages, degrees, years).
    pub level: Option<String>,
    /// How job postings phrased it.
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CategoryCount {
    pub category: RequirementCategory,
    pub requirements: u32,
    pub mentions: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClassCount {
    pub class: FrequencyClass,
    pub requirements: u32,
    /// Lower bound of the class in percent.
    pub threshold: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RequirementsView {
    pub jobs: u32,
    pub jobs_with_requirements: u32,
    /// Distinct requirements before the table controls.
    pub total: u32,
    pub rows: Vec<RequirementRow>,
    pub categories: Vec<CategoryCount>,
    pub classes: Vec<ClassCount>,
    pub profile_available: bool,
    pub summary: Vec<String>,
}

// ── Learning ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LearningGap {
    pub name: String,
    pub kind: RequirementKind,
    pub category: RequirementCategory,
    pub state: MatchState,
    pub jobs: u32,
    pub total_jobs: u32,
    pub percent: f64,
    pub score: f64,
    pub level: PriorityLevel,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Certification,
    Course,
    University,
    Documentation,
    Book,
    Lab,
    Tutorial,
    Project,
    Program,
}

text_enum!(ResourceType {
    Certification => "certification",
    Course => "course",
    University => "university",
    Documentation => "documentation",
    Book => "book",
    Lab => "lab",
    Tutorial => "tutorial",
    Project => "project",
    Program => "program",
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LearningResource {
    pub title: String,
    pub provider: Option<String>,
    pub url: String,
    pub kind: ResourceType,
    pub level: Option<String>,
    pub cost: Option<String>,
    pub note: Option<String>,
    /// Whether ReMa could open the link when it checked (None: not checked).
    pub reachable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LearningRecommendation {
    pub skill: String,
    pub level: PriorityLevel,
    /// Deterministic facts from the selected jobs.
    pub why: Vec<String>,
    pub path: Vec<String>,
    pub resources: Vec<LearningResource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ResearchStatus {
    Running,
    Done,
    Failed,
}

text_enum!(ResearchStatus { Running => "running", Done => "done", Failed => "failed" });

/// One stored research result with the snapshot it was based on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LearningResearch {
    pub id: i64,
    pub dataset_label: String,
    pub job_count: u32,
    pub generated_at: i64,
    pub status: ResearchStatus,
    pub error: Option<String>,
    pub model: Option<String>,
    pub gaps: Vec<LearningGap>,
    pub recommendations: Vec<LearningRecommendation>,
    /// Researched for the current selection and criteria.
    pub current: bool,
    /// The selection's gaps changed since the research.
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LearningView {
    pub profile_available: bool,
    pub jobs: u32,
    pub jobs_with_requirements: u32,
    pub gaps: Vec<LearningGap>,
    /// The research for this selection, or else the latest one.
    pub research: Option<LearningResearch>,
    pub model: Option<String>,
    pub notes: Vec<String>,
}

/// A search run created from a chat answer or task run (`source_id` is the
/// message or execution id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkedRun {
    pub source_id: i64,
    pub run_id: i64,
    pub jobs: u32,
}

// ── Events ────────────────────────────────────────────────────────────

/// Job data or research changed (ingestion, background details, research).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct AnalyticsChanged;
