//! ReMa Business as the interface and tools see it (Appendix B). Every
//! claim carries a status and provenance; every commercial record keeps the
//! offer version and policy versions it rests on; nothing here holds a
//! token or restricted member data.

use serde::{Deserialize, Serialize};
use specta::Type;

// ── Business Profile and offers (B3–B5) ──────────────────────────────

/// What the user is prepared to sell, beyond any one offer. Every field is
/// optional; research works without them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessProfile {
    pub business_name: String,
    pub website: String,
    pub service_area: String,
    pub languages: Vec<String>,
    pub capacity: String,
    pub availability: String,
    pub constraints: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OfferKind {
    Service,
    DigitalProduct,
    Hybrid,
}

impl OfferKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::DigitalProduct => "digital_product",
            Self::Hybrid => "hybrid",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "service" => Some(Self::Service),
            "digital_product" => Some(Self::DigitalProduct),
            "hybrid" => Some(Self::Hybrid),
            _ => None,
        }
    }
}

/// The user's own choice; never inferred from marketing language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Maturity {
    #[default]
    NotStated,
    Prototype,
    PilotReady,
    GenerallyAvailable,
}

/// Where a value stands (B4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FieldStatus {
    /// Seen on a source; not yet reviewed.
    Observed,
    UserConfirmed,
    /// An assumption to test, not a fact.
    Hypothesis,
    Unknown,
    /// Sources (or the user and a source) disagree.
    Conflicting,
}

/// One statement about the offer, with its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    pub text: String,
    pub status: FieldStatus,
    pub source_url: Option<String>,
    pub retrieved_at: Option<i64>,
    /// The words on the page that support it (data, never instructions).
    pub excerpt: Option<String>,
    /// How to read it ("a marketing claim, not a proven result").
    pub note: Option<String>,
}

impl Claim {
    pub fn user(text: &str) -> Self {
        Self {
            text: text.trim().to_string(),
            status: FieldStatus::UserConfirmed,
            source_url: None,
            retrieved_at: None,
            excerpt: None,
            note: None,
        }
    }

    pub fn unknown() -> Self {
        Self {
            text: String::new(),
            status: FieldStatus::Unknown,
            source_url: None,
            retrieved_at: None,
            excerpt: None,
            note: None,
        }
    }

    pub fn is_known(&self) -> bool {
        !self.text.trim().is_empty() && self.status != FieldStatus::Unknown
    }
}

/// How a price is billed. A monthly subscription and a daily rate are
/// never compared as if they were the same (B3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PriceUnit {
    Hourly,
    Daily,
    Project,
    SeatMonth,
    OrganizationMonth,
    Annual,
    OneTime,
    CustomQuote,
}

impl PriceUnit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hourly => "per hour",
            Self::Daily => "per day",
            Self::Project => "per project",
            Self::SeatMonth => "per seat per month",
            Self::OrganizationMonth => "per organization per month",
            Self::Annual => "per year",
            Self::OneTime => "one-time",
            Self::CustomQuote => "custom quote",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PricePoint {
    /// "49", "700–900", "from 1,200".
    pub amount: String,
    pub currency: Option<String>,
    pub unit: PriceUnit,
    pub claim: Claim,
}

/// An offer's content (one version, or the draft under review).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OfferContent {
    pub name: String,
    pub kind: OfferKind,
    pub maturity: Maturity,
    /// What it does.
    pub summary: Claim,
    pub problem: Claim,
    pub outcomes: Vec<Claim>,
    pub features: Vec<Claim>,
    pub use_cases: Vec<Claim>,
    pub customer_types: Vec<Claim>,
    pub buyer_roles: Vec<Claim>,
    pub pricing: Vec<PricePoint>,
    pub delivery_model: Claim,
    pub geography: Vec<Claim>,
    pub languages: Vec<Claim>,
    pub requirements: Vec<Claim>,
    pub integrations: Vec<Claim>,
    pub deployment_constraints: Vec<Claim>,
    pub exclusions: Vec<Claim>,
    /// Claims the user rejected (kept, so a refresh does not bring them
    /// back as new).
    pub unsupported_claims: Vec<Claim>,
    pub limitations: Vec<Claim>,
    pub website_urls: Vec<String>,
}

impl OfferContent {
    pub fn empty(name: &str, kind: OfferKind) -> Self {
        Self {
            name: name.trim().to_string(),
            kind,
            maturity: Maturity::NotStated,
            summary: Claim::unknown(),
            problem: Claim::unknown(),
            outcomes: Vec::new(),
            features: Vec::new(),
            use_cases: Vec::new(),
            customer_types: Vec::new(),
            buyer_roles: Vec::new(),
            pricing: Vec::new(),
            delivery_model: Claim::unknown(),
            geography: Vec::new(),
            languages: Vec::new(),
            requirements: Vec::new(),
            integrations: Vec::new(),
            deployment_constraints: Vec::new(),
            exclusions: Vec::new(),
            unsupported_claims: Vec::new(),
            limitations: Vec::new(),
            website_urls: Vec::new(),
        }
    }

    /// Every claim with the name of its field.
    pub fn claims(&self) -> Vec<(&'static str, &Claim)> {
        let mut out = vec![
            ("summary", &self.summary),
            ("problem", &self.problem),
            ("delivery_model", &self.delivery_model),
        ];
        for (name, list) in [
            ("outcomes", &self.outcomes),
            ("features", &self.features),
            ("use_cases", &self.use_cases),
            ("customer_types", &self.customer_types),
            ("buyer_roles", &self.buyer_roles),
            ("geography", &self.geography),
            ("languages", &self.languages),
            ("requirements", &self.requirements),
            ("integrations", &self.integrations),
            ("deployment_constraints", &self.deployment_constraints),
            ("exclusions", &self.exclusions),
            ("limitations", &self.limitations),
        ] {
            out.extend(list.iter().map(|c| (name, c)));
        }
        out.extend(self.pricing.iter().map(|p| ("pricing", &p.claim)));
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub id: String,
    pub name: String,
    pub kind: OfferKind,
    /// The newest reviewed version (None until reviewed).
    pub current_version: Option<u32>,
    /// The content being edited or reviewed.
    pub draft: Option<OfferContent>,
    /// The newest reviewed content.
    pub reviewed: Option<OfferContent>,
    pub reviewed_at: Option<i64>,
    pub archived: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub revision: u32,
}

/// "Offer: Support Workspace v2".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OfferRef {
    pub offer_id: String,
    pub version: u32,
    pub name: String,
}

// ── Offer ingestion (B4) ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PageStatus {
    Read,
    /// Read up to ReMa's size limit.
    Truncated,
    /// robots.txt or ReMa's network policy does not allow it.
    Blocked,
    Failed,
    /// Outside the product's own site (or redirected there).
    OutOfScope,
}

/// One page ReMa tried to read for an offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IngestPage {
    pub url: String,
    pub title: Option<String>,
    pub status: PageStatus,
    pub detail: Option<String>,
    pub characters: u32,
    pub retrieved_at: i64,
}

/// What to describe an offer from (B4): a public URL, pasted text or a
/// document the user picks (read like Profile documents).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DescribeInput {
    pub name: String,
    pub kind: Option<OfferKind>,
    pub url: Option<String>,
    pub text: Option<String>,
    pub document_path: Option<String>,
    /// Update this offer's draft instead of creating a new offer.
    pub offer_id: Option<String>,
    pub run_id: String,
    pub idempotency_key: Option<String>,
}

/// What a refresh found compared with the draft (B5): a proposal, never
/// an overwrite.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OfferDiff {
    /// "features: Salesforce integration".
    pub added: Vec<String>,
    /// Claims from the website that the pages no longer show.
    pub not_found: Vec<String>,
    /// The website now disagrees with what you confirmed.
    pub conflicts: Vec<String>,
    /// Claims you rejected earlier that the website still makes (kept
    /// rejected).
    pub still_rejected: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DescribeResult {
    pub run_id: String,
    pub status: RunStatus,
    pub offer: Offer,
    pub pages: Vec<IngestPage>,
    /// Required or useful information still missing.
    pub missing: Vec<String>,
    pub notes: Vec<String>,
    pub diff: Option<OfferDiff>,
}

// ── Research runs (B27, B31) ─────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    OfferIngest,
    Clients,
    Contracts,
    Gtm,
}

impl RunKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OfferIngest => "offer_ingest",
            Self::Clients => "clients",
            Self::Contracts => "contracts",
            Self::Gtm => "gtm",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "offer_ingest" => Self::OfferIngest,
            "contracts" => Self::Contracts,
            "gtm" => Self::Gtm,
            _ => Self::Clients,
        }
    }
}

/// A research run as the page lists it (its result is loaded separately).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessRun {
    pub id: String,
    pub kind: RunKind,
    pub offer: Option<OfferRef>,
    pub query: String,
    pub status: RunStatus,
    /// "OpenAI · gpt-…" (never a key or token).
    pub model: Option<String>,
    pub sources: Vec<String>,
    pub failures: Vec<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

/// A record that data was removed, without the removed data (B29).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Redaction {
    pub id: String,
    pub opportunity_id: Option<String>,
    pub kind: String,
    pub detail: String,
    pub created_at: i64,
}

/// Distinguishable outcomes (B27): a failure is never a zero-result search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Complete,
    Partial,
    NoVerifiedMatches,
    NeedsReview,
    CapabilityUnavailable,
    Offline,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::NoVerifiedMatches => "no_verified_matches",
            Self::NeedsReview => "needs_review",
            Self::CapabilityUnavailable => "capability_unavailable",
            Self::Offline => "offline",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "complete" => Self::Complete,
            "partial" => Self::Partial,
            "no_verified_matches" => Self::NoVerifiedMatches,
            "needs_review" => Self::NeedsReview,
            "capability_unavailable" => Self::CapabilityUnavailable,
            "offline" => Self::Offline,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

/// A status line of a running Business request (for the page).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct BusinessProgress {
    pub run_id: String,
    pub text: String,
}

/// Business records changed (offers, pipeline, plans).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct BusinessChanged;

// ── Locations (B7) ────────────────────────────────────────────────────

/// Places a search covers: within a list, any may match (OR).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Locations {
    /// Canonical English country names; "DACH" expands to Germany,
    /// Austria and Switzerland.
    pub countries: Vec<String>,
    pub regions: Vec<String>,
    pub cities: Vec<String>,
    /// A radius needs reliable coordinates ReMa does not have: stated,
    /// never applied by guessing.
    pub radius_km: Option<u32>,
}

// ── Requests from the page ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClientSearchInput {
    pub run_id: String,
    pub offer_id: String,
    /// A reviewed version (the newest when not given).
    pub offer_version: Option<u32>,
    pub query: String,
    pub criteria: ClientCriteria,
    /// Look for named, permitted professional contacts (slower).
    pub find_people: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContractSearchInput {
    pub run_id: String,
    pub query: String,
    /// The edited criteria; read from the query when not given.
    pub criteria: Option<ContractCriteria>,
}

// ── Find Clients (B8–B11) ─────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClientCriteria {
    pub locations: Locations,
    pub industries: Vec<String>,
    pub min_employees: Option<u32>,
    pub max_employees: Option<u32>,
    /// Companies or industries to leave out.
    pub exclusions: Vec<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HardResult {
    Pass,
    Fail,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HardCheck {
    pub name: String,
    pub result: HardResult,
    pub detail: String,
}

/// One soft criterion of the fit policy (B10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CriterionScore {
    pub id: String,
    pub label: String,
    pub weight: f64,
    pub applicable: bool,
    /// 0..1, or None when unknown (unknown is not zero).
    pub value: Option<f64>,
    pub reason: String,
    /// Indexes into the assessment's evidence.
    pub evidence: Vec<u32>,
}

/// A source line behind an assessment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceNote {
    pub label: String,
    pub url: Option<String>,
    pub excerpt: Option<String>,
    pub retrieved_at: i64,
    /// Supporting or contrary.
    pub contrary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FitAssessment {
    pub id: String,
    pub company_key: String,
    pub company_name: String,
    pub offer: OfferRef,
    pub policy_version: String,
    pub hard: Vec<HardCheck>,
    pub criteria: Vec<CriterionScore>,
    /// Sum of the weights of criteria with a known value (0..1).
    pub coverage: f64,
    /// 0..100; None when coverage is zero.
    pub score: Option<f64>,
    /// Whether the score may be shown (coverage at or above the threshold).
    pub score_shown: bool,
    pub evidence: Vec<SourceNote>,
    pub created_at: i64,
}

/// Who might care about the offer at a company (B11). Never purchase
/// authority by title alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Authority {
    /// A source states the person is responsible for this area.
    VerifiedResponsibility,
    /// Their function matches the offer's buyer role.
    LikelyFunctionalContact,
    UnknownAuthority,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BuyerContact {
    /// A named person when a permitted source names one.
    pub name: Option<String>,
    pub title: Option<String>,
    /// The buyer role this contact stands for.
    pub role: String,
    pub authority: Authority,
    pub reason: String,
    pub profile_url: Option<String>,
    /// The company's own contact page when no person could be named.
    pub contact_page: Option<String>,
    pub source_url: Option<String>,
    pub suppressed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClientProspect {
    pub company_key: String,
    pub company_name: String,
    pub website: Option<String>,
    pub locations: Vec<String>,
    pub industry: Option<String>,
    pub size: Option<String>,
    pub why_it_fits: Vec<String>,
    /// What was observed (a posting, a technology), never read as intent.
    pub observed_signals: Vec<String>,
    /// Always "none observed" unless the user records otherwise.
    pub verified_buying_intent: String,
    /// ReMa does not decide whether contacting is lawful or welcome.
    pub permission_to_contact: String,
    pub contacts: Vec<BuyerContact>,
    pub assessment: FitAssessment,
    pub missing: Vec<String>,
    pub suppressed: bool,
    /// The saved opportunity, when this prospect is in the pipeline.
    pub opportunity_id: Option<String>,
    pub links: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClientResults {
    pub run_id: String,
    pub status: RunStatus,
    pub offer: OfferRef,
    pub criteria: ClientCriteria,
    /// Plain-words ICP hypothesis this search used.
    pub icp: Vec<String>,
    pub confirmed: Vec<ClientProspect>,
    /// Required facts unknown: to verify.
    pub needs_verification: Vec<ClientProspect>,
    /// Hard criteria failed (kept apart, never mixed in).
    pub excluded: Vec<ClientProspect>,
    pub notes: Vec<String>,
    pub sources: Vec<String>,
    pub failures: Vec<String>,
    pub retrieved_at: i64,
    pub policy_version: String,
    pub scoring_policy: String,
}

// ── Find Contract Work (B12–B14) ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Engagement {
    Freelance,
    Contract,
    B2b,
    Interim,
    Project,
    FixedTermEmployee,
    PermanentEmployee,
    /// Salaried employment whose term is not stated.
    Employment,
    NeedsVerification,
}

impl Engagement {
    pub fn label(self) -> &'static str {
        match self {
            Self::Freelance => "Freelance",
            Self::Contract => "Contract",
            Self::B2b => "B2B",
            Self::Interim => "Interim",
            Self::Project => "Project",
            Self::FixedTermEmployee => "Fixed-term employment",
            Self::PermanentEmployee => "Permanent employment",
            Self::Employment => "Employment (term not stated)",
            Self::NeedsVerification => "Engagement type needs verification",
        }
    }

    pub fn independent(self) -> bool {
        matches!(
            self,
            Self::Freelance | Self::Contract | Self::B2b | Self::Interim | Self::Project
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RateUnit {
    Hour,
    Day,
    Month,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    /// Strictly above.
    Above,
    AtLeast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DurationUnit {
    Week,
    Month,
}

/// A contract's terms as the listing states them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContractTerms {
    pub engagement: Engagement,
    pub rate_min: Option<f64>,
    pub rate_max: Option<f64>,
    /// "up to" states a ceiling only.
    pub rate_is_ceiling: bool,
    pub currency: Option<String>,
    pub rate_unit: Option<RateUnit>,
    /// As written ("EUR 700–850 per day").
    pub rate_text: Option<String>,
    pub duration_min: Option<f64>,
    pub duration_max: Option<f64>,
    pub duration_unit: Option<DurationUnit>,
    pub duration_text: Option<String>,
    pub extension_possible: Option<bool>,
    pub start: Option<String>,
    pub workload: Option<String>,
    pub work_mode: Option<String>,
    /// Stated eligibility ("EU only", "Germany"): unknown stays empty.
    pub eligibility: Vec<String>,
    pub agency: Option<String>,
    /// None: the end client is not disclosed.
    pub end_client: Option<String>,
    pub skills: Vec<String>,
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContractCriteria {
    pub skills: String,
    pub locations: Locations,
    pub remote_ok: bool,
    pub min_rate: Option<f64>,
    pub rate_comparison: Comparison,
    pub currency: Option<String>,
    pub rate_unit: RateUnit,
    /// The user's own hours-per-day basis; without it hourly and daily
    /// rates are not compared.
    pub hours_per_day: Option<f64>,
    pub duration_min_months: Option<f64>,
    pub duration_max_months: Option<f64>,
    pub posted_within_days: Option<u32>,
}

impl Default for ContractCriteria {
    fn default() -> Self {
        Self {
            skills: String::new(),
            locations: Locations::default(),
            remote_ok: true,
            min_rate: None,
            rate_comparison: Comparison::Above,
            currency: None,
            rate_unit: RateUnit::Day,
            hours_per_day: None,
            duration_min_months: None,
            duration_max_months: None,
            posted_within_days: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    Confirmed,
    NeedsVerification,
    NotMatching,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContractResult {
    pub key: String,
    pub title: String,
    pub company: Option<String>,
    pub location: Option<String>,
    pub url: String,
    pub source: String,
    pub posted_at: Option<i64>,
    pub verified: String,
    pub terms: ContractTerms,
    pub status: MatchStatus,
    /// Why it is not confirmed (each criterion).
    pub reasons: Vec<String>,
    pub scope: Option<String>,
    pub opportunity_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContractResults {
    pub run_id: String,
    pub status: RunStatus,
    pub criteria: ContractCriteria,
    /// The criteria as ReMa applies them ("rate strictly above EUR 700 per
    /// day").
    pub normalized: Vec<String>,
    pub confirmed: Vec<ContractResult>,
    pub needs_verification: Vec<ContractResult>,
    pub not_matching: Vec<ContractResult>,
    pub notes: Vec<String>,
    pub sources: Vec<String>,
    pub failures: Vec<String>,
    pub retrieved_at: i64,
}

// ── Pipeline (B15, B16, B23) ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStage {
    NewLead,
    Qualified,
    Contacted,
    Discussion,
    Proposal,
    Won,
    Lost,
}

impl PipelineStage {
    pub const ALL: [PipelineStage; 7] = [
        Self::NewLead,
        Self::Qualified,
        Self::Contacted,
        Self::Discussion,
        Self::Proposal,
        Self::Won,
        Self::Lost,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::NewLead => "new_lead",
            Self::Qualified => "qualified",
            Self::Contacted => "contacted",
            Self::Discussion => "discussion",
            Self::Proposal => "proposal",
            Self::Won => "won",
            Self::Lost => "lost",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NewLead => "New Lead",
            Self::Qualified => "Qualified",
            Self::Contacted => "Contacted",
            Self::Discussion => "Discussion",
            Self::Proposal => "Proposal",
            Self::Won => "Won",
            Self::Lost => "Lost",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityKind {
    Product,
    Service,
    Contract,
}

impl OpportunityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Product => "product",
            Self::Service => "service",
            Self::Contract => "contract",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "product" => Self::Product,
            "contract" => Self::Contract,
            _ => Self::Service,
        }
    }
}

/// A person or role at the buyer (permitted public professional data only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactRef {
    pub id: String,
    pub name: Option<String>,
    pub title: Option<String>,
    pub role: String,
    pub profile_url: Option<String>,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Amount {
    /// "12,000", "700–850".
    pub value: String,
    pub currency: Option<String>,
    /// "per day", "one-time", "per year".
    pub basis: String,
    /// "advertised", "estimate", "user target", "negotiated".
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Opportunity {
    pub id: String,
    pub kind: OpportunityKind,
    pub name: String,
    pub company_key: Option<String>,
    /// None: the end client is not disclosed.
    pub company_name: Option<String>,
    pub offer: Option<OfferRef>,
    pub canonical_job_id: Option<String>,
    pub source_url: Option<String>,
    pub use_case: String,
    pub stage: PipelineStage,
    pub archived: bool,
    pub do_not_contact: bool,
    pub contacts: Vec<ContactRef>,
    pub evidence: Vec<SourceNote>,
    pub amount: Option<Amount>,
    pub contract: Option<ContractTerms>,
    /// "open", "closed", "not reachable" (for saved listings).
    pub listing_status: Option<String>,
    pub next_step: String,
    pub notes: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_researched_at: Option<i64>,
    pub last_commercial_activity_at: Option<i64>,
    pub revision: u32,
    /// The latest fit assessment (clients).
    pub assessment: Option<FitAssessment>,
    pub assessments: u32,
    pub activities: Vec<Activity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    Contact,
    Reply,
    PositiveReply,
    MeetingHeld,
    ProposalSent,
    Won,
    Lost,
    StageChange,
    Note,
}

impl ActivityType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contact => "contact",
            Self::Reply => "reply",
            Self::PositiveReply => "positive_reply",
            Self::MeetingHeld => "meeting_held",
            Self::ProposalSent => "proposal_sent",
            Self::Won => "won",
            Self::Lost => "lost",
            Self::StageChange => "stage_change",
            Self::Note => "note",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "contact" => Self::Contact,
            "reply" => Self::Reply,
            "positive_reply" => Self::PositiveReply,
            "meeting_held" => Self::MeetingHeld,
            "proposal_sent" => Self::ProposalSent,
            "won" => Self::Won,
            "lost" => Self::Lost,
            "stage_change" => Self::StageChange,
            _ => Self::Note,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Contact => "Contacted",
            Self::Reply => "Reply received",
            Self::PositiveReply => "Positive reply",
            Self::MeetingHeld => "Meeting held",
            Self::ProposalSent => "Proposal sent",
            Self::Won => "Won",
            Self::Lost => "Lost",
            Self::StageChange => "Stage changed",
            Self::Note => "Note",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: String,
    pub opportunity_id: String,
    pub kind: ActivityType,
    pub person: Option<String>,
    pub occurred_at: i64,
    pub recorded_at: i64,
    /// Always user reported in this version.
    pub source: String,
    pub detail: Option<String>,
    pub from_stage: Option<PipelineStage>,
    pub to_stage: Option<PipelineStage>,
    pub experiment_id: Option<String>,
    pub variant: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Suppression {
    pub id: String,
    pub scope: String,
    pub key: String,
    pub label: String,
    pub reason: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    pub opportunities: Vec<Opportunity>,
    /// Per stage, archived excluded.
    pub counts: Vec<(PipelineStage, u32)>,
    pub suppressions: Vec<Suppression>,
}

/// A local message draft. There is no sending action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: String,
    pub opportunity_id: Option<String>,
    pub plan_id: Option<String>,
    pub experiment_id: Option<String>,
    pub variant: Option<String>,
    pub offer: Option<OfferRef>,
    /// A named person or a role ("Head of Operations").
    pub recipient: String,
    pub channel: String,
    pub subject: Option<String>,
    pub body: String,
    pub evidence: Vec<SourceNote>,
    pub created_at: i64,
    pub updated_at: i64,
    pub revision: u32,
}

// ── Go-to-Market Studio (B17–B23) ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SegmentStatus {
    Hypothesis,
    UnderTest,
    SupportedByObservations,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: String,
    pub name: String,
    pub organization_type: String,
    pub geography: String,
    pub size_band: String,
    pub use_case: String,
    pub pain_hypothesis: String,
    pub prerequisites: Vec<String>,
    pub buyer_roles: Vec<String>,
    pub likely_objections: Vec<String>,
    pub observable_signals: Vec<String>,
    pub disqualifiers: Vec<String>,
    pub supporting_evidence: Vec<SourceNote>,
    pub counterevidence: Vec<SourceNote>,
    pub unknowns: Vec<String>,
    pub validation_questions: Vec<String>,
    pub status: SegmentStatus,
    /// Chosen by the user for target-account discovery.
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Alternative {
    pub name: String,
    /// "direct product", "adjacent product", "internal development",
    /// "agency or service", "doing nothing / manual process".
    pub kind: String,
    pub summary: String,
    /// As published, with its billing unit; "unknown" when not found.
    pub pricing: String,
    pub evidence: Vec<SourceNote>,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPlan {
    pub name: String,
    pub audience: String,
    pub why: String,
    pub entry_point: String,
    /// Access or promotion rules when retrievable; "unknown" otherwise.
    pub rules: String,
    pub effort: String,
    pub costs: String,
    pub unknowns: Vec<String>,
    pub test: String,
    pub available: bool,
    pub evidence: Vec<SourceNote>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanContent {
    pub segments: Vec<Segment>,
    pub alternatives: Vec<Alternative>,
    /// The positioning draft (never changes the offer's capabilities).
    pub positioning: String,
    /// Differentiation the user claims but has not tested.
    pub untested_claims: Vec<String>,
    pub channels: Vec<ChannelPlan>,
    /// Target accounts from Find Clients (company keys and names).
    pub target_accounts: Vec<TargetAccount>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TargetAccount {
    /// The Find Clients run it came from (saving uses it).
    pub run_id: Option<String>,
    pub segment_id: Option<String>,
    pub company_key: String,
    pub company_name: String,
    pub why: String,
    pub buyer_role: String,
    pub contact: Option<String>,
    pub link: Option<String>,
    pub trigger: Option<String>,
    pub angle: String,
    pub fit: Option<f64>,
    pub coverage: f64,
    pub question: String,
    pub opportunity_id: Option<String>,
    pub suppressed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GtmPlan {
    pub id: String,
    pub offer: OfferRef,
    pub name: String,
    pub geography: String,
    pub content: PlanContent,
    pub created_at: i64,
    pub updated_at: i64,
    pub revision: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStatus {
    Draft,
    Planned,
    Running,
    Paused,
    Completed,
    Cancelled,
}

impl ExperimentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Planned => "planned",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "planned" => Self::Planned,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            "cancelled" => Self::Cancelled,
            _ => Self::Draft,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Assignment {
    /// Randomized by account, frozen before contact.
    RandomByAccount,
    Manual,
    Sequential,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub id: String,
    pub label: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CohortEntry {
    pub account_key: String,
    pub account_name: String,
    pub opportunity_id: Option<String>,
    /// Frozen once the experiment runs.
    pub variant: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentContent {
    pub hypothesis: String,
    pub channel: String,
    pub variants: Vec<Variant>,
    pub assignment: Assignment,
    pub cohort: Vec<CohortEntry>,
    /// "reply_rate", "positive_reply_rate", "meeting_rate", …
    pub primary_metric: String,
    pub success_threshold: Option<String>,
    pub planned_start: Option<i64>,
    pub planned_end: Option<i64>,
    pub observation_days: u32,
    pub sample_target: Option<u32>,
    pub budget: Option<String>,
    pub effort_budget: Option<String>,
    pub stop_conditions: Vec<String>,
    pub exclusions: Vec<String>,
    pub outcome_summary: Option<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Experiment {
    pub id: String,
    pub plan_id: String,
    pub offer: OfferRef,
    pub segment_id: Option<String>,
    pub version: u32,
    pub status: ExperimentStatus,
    pub content: ExperimentContent,
    /// When variants and assignments were frozen (before any contact).
    pub frozen_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub revision: u32,
}

/// numerator / denominator, None when the denominator is zero (B23).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Rate {
    pub numerator: u32,
    pub denominator: u32,
    pub value: Option<f64>,
    /// "3 / 20 (15%)" or "Not available".
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VariantMetrics {
    /// None: all variants together.
    pub variant: Option<String>,
    pub accounts_in_cohort: u32,
    pub accounts_contacted: u32,
    pub accounts_replied: u32,
    pub accounts_positive_reply: u32,
    pub accounts_met: u32,
    pub accounts_proposed: u32,
    pub accounts_won: u32,
    pub reply_rate: Rate,
    pub positive_reply_rate: Rate,
    pub meeting_rate: Rate,
    pub proposal_rate: Rate,
    pub win_rate: Rate,
    /// People contacted and messages, counted separately (with units).
    pub people_contacted: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentMetrics {
    pub experiment_id: String,
    pub window_start: Option<i64>,
    pub window_end: Option<i64>,
    pub overall: VariantMetrics,
    pub variants: Vec<VariantMetrics>,
    /// "User-reported activity", "small cohort", "non-random assignment".
    pub limitations: Vec<String>,
    /// Activities not counted and why.
    pub unattributed: Vec<String>,
    pub computed_at: i64,
}

// ── Overview for the page ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessOverview {
    pub profile: BusinessProfile,
    pub offers: Vec<Offer>,
    pub plans: Vec<GtmPlan>,
    pub experiments: Vec<Experiment>,
    pub drafts: Vec<Draft>,
}
