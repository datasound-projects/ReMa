//! The job engine behind ReMa MCP's tools. One pipeline for all of them:
//!
//! discover (search backend) → classify (source policy) → reuse cached
//! checks or read each vacancy (adapters, bounded) → snippet-only records
//! for what cannot be read → store under stable ids → mark possible
//! duplicates → strict filters → deterministic ranking → snapshot → a
//! compact page with a cursor.
//!
//! Every step honors the request's deadline and cancellation; work that
//! does not finish in time is reported as partial, never dropped silently.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::{stream, StreamExt};
use tokio_util::sync::CancellationToken;

use super::{
    adapters::{self, Ctx},
    contract::*,
    extract,
    filter::{self, Verdict},
    sources::{self, Classified, Target},
    store::{self, Snapshot},
};
use crate::{
    analytics::normalize,
    llm::Endpoint,
    retrieval::{self, backend::Service, intent::JobQuery},
    state::AppState,
    time::now_ms,
};

pub const PARSER_VERSION: u32 = 1;
/// ReMa's own sources and a search service (company sites included).
const SERVICE_DEADLINE: Duration = Duration::from_secs(25);
/// One model turn with hosted searches often takes longer than 20 s.
const PROVIDER_DEADLINE: Duration = Duration::from_secs(90);
const MAX_QUERIES: usize = 8;
const CANONICAL_LOOKUPS: usize = 2;
const MAX_CANDIDATES: usize = 100;
const MAX_FETCHES: usize = 30;
const FETCH_CONCURRENCY: usize = 6;
const SNAPSHOT_TTL_MS: i64 = 10 * 60_000;
const STATUS_TTL_MS: i64 = 30 * 60_000;
pub const MAX_SEARCH_PAYLOAD: usize = 32 * 1024;
pub const DESCRIPTION_PART: usize = 12_000;
const BATCH_DESCRIPTION: usize = 5_000;

/// How vacancies are discovered in this session. ReMa's own job sources
/// (`career_search::jobs`: employer boards and public job boards, no key)
/// are always asked; a search service from Settings (optional, advanced)
/// and the chat model's own web search add to them. Discovery never
/// depends on a search service being set up.
#[derive(Clone, Default)]
pub struct Discovery {
    /// A search service set up in Settings (Brave, Tavily, SearXNG).
    pub service: Option<Arc<Service>>,
    /// The chat model's own web search (hosted, or its local server's).
    pub provider: Option<(Endpoint, String)>,
}

impl Discovery {
    /// ReMa's own sources only.
    pub fn own() -> Self {
        Self::default()
    }

    /// The chat's discovery: ReMa's sources, plus the search service when
    /// one is set up (a misconfigured one is skipped, not an error), plus
    /// the model's own web search when it has one.
    pub async fn for_chat(state: &AppState, endpoint: Option<(&Endpoint, &str)>) -> Self {
        let service = retrieval::backend::configured(state)
            .await
            .ok()
            .flatten()
            .map(Arc::new);
        let provider = endpoint
            .filter(|(e, _)| retrieval::native::supported(e))
            .map(|(e, m)| (e.clone(), m.to_string()));
        Self { service, provider }
    }

    /// "ReMa job sources + Brave Search + OpenAI web search".
    pub fn name(&self) -> Option<String> {
        let mut parts = vec![OWN_SOURCES.to_string()];
        if let Some(service) = &self.service {
            parts.push(service.name().to_string());
        }
        if let Some((endpoint, _)) = &self.provider {
            parts.push(retrieval::native::engine_name(endpoint).to_string());
        }
        Some(parts.join(" + "))
    }

    fn identity(&self) -> String {
        let mut id = "rema".to_string();
        if let Some(service) = &self.service {
            id.push_str(&format!("+service:{}", service.name()));
        }
        if let Some((endpoint, model_id)) = &self.provider {
            id.push_str(&format!("+provider:{}:{model_id}", endpoint.kind.as_str()));
        }
        id
    }

    /// Whether links of discovery-only sources (LinkedIn, XING, boards)
    /// can be found: they need a web search.
    pub fn searches_the_web(&self) -> bool {
        self.service.is_some() || self.provider.is_some()
    }

    pub fn deadline(&self) -> Duration {
        if self.provider.is_some() {
            PROVIDER_DEADLINE
        } else {
            SERVICE_DEADLINE
        }
    }
}

/// How ReMa's own job sources are named in coverage and status.
pub const OWN_SOURCES: &str = "ReMa job sources";

/// One tool session (one chat answer, or a Settings check).
#[derive(Clone)]
pub struct Session {
    pub state: AppState,
    pub discovery: Discovery,
}

impl Session {
    fn ctx<'a>(
        &'a self,
        cancel: &'a CancellationToken,
        deadline: Instant,
        refresh: bool,
    ) -> Ctx<'a> {
        let runtime = &self.state.rema_mcp;
        Ctx {
            fetcher: runtime.fetcher(&self.state.info.version),
            apis: runtime.apis(),
            feeds: runtime.feeds(),
            deadline,
            cancel,
            now: now_ms(),
            refresh,
        }
    }

    fn local(&self) -> bool {
        self.state.rema_mcp.local_fixtures()
    }
}

fn db_error(e: crate::error::AppError) -> ToolError {
    ToolError::new(ErrorCode::SourceUnavailable, format!("local cache: {e}"))
}

fn cancelled() -> ToolError {
    ToolError::new(ErrorCode::Cancelled, "the request was stopped")
}

// ── Input validation ──────────────────────────────────────────────────

fn clean_list(items: &[String], max_len: usize) -> Result<Vec<String>, ToolError> {
    let mut out = Vec::new();
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if item.chars().count() > max_len {
            return Err(ToolError::invalid(format!(
                "\"{}…\" is too long",
                extract::clip(item, 20)
            )));
        }
        if !out.iter().any(|o: &String| o.eq_ignore_ascii_case(item)) {
            out.push(item.to_string());
        }
    }
    Ok(out)
}

fn clean_locations(items: &[LocationFilter]) -> Result<Vec<LocationFilter>, ToolError> {
    let mut out = Vec::new();
    for loc in items {
        let field = |v: &Option<String>| {
            v.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| extract::clip(s, 100))
        };
        let country = field(&loc.country)
            .map(|c| normalize::country_name(&c).map(str::to_string).unwrap_or(c));
        let city =
            field(&loc.city).map(|c| normalize::city_name(&c).map(str::to_string).unwrap_or(c));
        let cleaned = LocationFilter {
            city,
            region: field(&loc.region),
            country,
        };
        if cleaned.city.is_none() && cleaned.region.is_none() && cleaned.country.is_none() {
            return Err(ToolError::invalid(
                "each location needs a city, region or country",
            ));
        }
        out.push(cleaned);
    }
    Ok(out)
}

fn clean_salary(
    min: Option<f64>,
    max: Option<f64>,
    currency: &Option<String>,
    period: Option<SalaryPeriod>,
) -> Result<(Option<String>, Option<SalaryPeriod>), ToolError> {
    for v in [min, max].into_iter().flatten() {
        if !v.is_finite() || v <= 0.0 {
            return Err(ToolError::invalid(
                "salary amounts must be positive numbers",
            ));
        }
    }
    if let (Some(a), Some(b)) = (min, max) {
        if a > b {
            return Err(ToolError::invalid("salary_min is above salary_max"));
        }
    }
    let currency = currency
        .as_deref()
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty());
    if let Some(c) = &currency {
        if c.len() != 3 || !c.chars().all(|ch| ch.is_ascii_alphabetic()) {
            return Err(ToolError::invalid(
                "salary_currency must be an ISO 4217 code such as EUR",
            ));
        }
    }
    if (min.is_some() || max.is_some()) && (currency.is_none() || period.is_none()) {
        return Err(ToolError::invalid(
            "salary_min/salary_max need salary_currency and salary_period (no conversions are made)",
        ));
    }
    Ok((currency, period))
}

fn clean_sources(items: &[String]) -> Result<Vec<String>, ToolError> {
    let mut out = Vec::new();
    for s in items {
        let id = s.trim().to_lowercase().replace(['.', '-'], "_");
        if id.is_empty() {
            continue;
        }
        if sources::get(&id).is_none() {
            let known: Vec<&str> = sources::REGISTRY.iter().map(|s| s.id).collect();
            return Err(ToolError::invalid(format!(
                "unknown source \"{s}\"; known sources: {}",
                known.join(", ")
            )));
        }
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

#[derive(Debug)]
pub struct Request {
    pub query: String,
    /// Close variants of the query's role that also count as on topic
    /// (set by ReMa's own job search; tool calls leave it empty).
    pub also: Vec<String>,
    pub filters: SearchFilters,
    pub limit: usize,
    pub sort: SortMode,
    pub include_unresolved: bool,
    pub refresh: bool,
    pub cursor: Option<String>,
}

pub fn validate(input: &SearchJobsInput) -> Result<Request, ToolError> {
    let query = input.query.trim();
    if query.is_empty() {
        return Err(ToolError::invalid("query is empty"));
    }
    if query.chars().count() > 200 {
        return Err(ToolError::invalid("query is longer than 200 characters"));
    }
    let (salary_currency, salary_period) = clean_salary(
        input.salary_min,
        input.salary_max,
        &input.salary_currency,
        input.salary_period,
    )?;
    if let Some(days) = input.posted_within_days {
        if !(1..=365).contains(&days) {
            return Err(ToolError::invalid("posted_within_days must be 1–365"));
        }
    }
    let languages = clean_list(&input.languages, 5)?;
    if languages
        .iter()
        .any(|l| l.len() != 2 || !l.chars().all(|c| c.is_ascii_alphabetic()))
    {
        return Err(ToolError::invalid(
            "languages are ISO 639-1 codes such as \"en\", \"de\", \"pl\"",
        ));
    }
    let limit = input.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ToolError::invalid(format!("limit must be 1–{MAX_LIMIT}")));
    }
    Ok(Request {
        query: extract::clip(query, 200),
        also: Vec::new(),
        filters: SearchFilters {
            required_skills: clean_list(&input.required_skills, 60)?,
            required_skills_mode: input.required_skills_mode.unwrap_or_default(),
            skills: clean_list(&input.skills, 60)?,
            exclude_terms: clean_list(&input.exclude_terms, 60)?,
            companies: clean_list(&input.companies, 100)?,
            locations: clean_locations(&input.locations)?,
            work_modes: dedup(&input.work_modes),
            seniority: dedup(&input.seniority),
            employment_types: dedup(&input.employment_types),
            working_time: dedup(&input.working_time),
            salary_min: input.salary_min,
            salary_max: input.salary_max,
            salary_currency,
            salary_period,
            posted_within_days: input.posted_within_days,
            languages: languages.iter().map(|l| l.to_lowercase()).collect(),
            sources: clean_sources(&input.sources)?,
            filter_mode: input.filter_mode.unwrap_or_default(),
        },
        limit: limit as usize,
        sort: input.sort.unwrap_or_default(),
        include_unresolved: input.include_unresolved.unwrap_or(false),
        refresh: input.refresh.unwrap_or(false),
        cursor: input.cursor.clone(),
    })
}

fn dedup<T: PartialEq + Copy>(items: &[T]) -> Vec<T> {
    let mut out = Vec::new();
    for item in items {
        if !out.contains(item) {
            out.push(*item);
        }
    }
    out
}

// ── Cursors ───────────────────────────────────────────────────────────

fn query_key(session: &Session, req: &Request, similar: Option<&str>) -> String {
    let json = serde_json::json!({
        "v": PARSER_VERSION,
        "q": req.query.to_lowercase(),
        "a": req.also,
        "f": req.filters,
        "s": req.sort,
        "u": req.include_unresolved,
        "b": session.discovery.identity(),
        "similar": similar,
    });
    store::sha(&json.to_string())
}

fn encode_cursor(snapshot: &str, offset: usize, key: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{snapshot}:{offset}:{}", &key[..16]))
}

fn decode_cursor(cursor: &str) -> Result<(String, usize, String), ToolError> {
    let bad = || ToolError::invalid("the cursor is not valid");
    let raw = URL_SAFE_NO_PAD.decode(cursor.trim()).map_err(|_| bad())?;
    let text = String::from_utf8(raw).map_err(|_| bad())?;
    let mut parts = text.split(':');
    let (Some(snapshot), Some(offset), Some(key), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(bad());
    };
    if !snapshot.starts_with("rs_") || snapshot.len() > 40 {
        return Err(bad());
    }
    Ok((
        snapshot.to_string(),
        offset.parse().map_err(|_| bad())?,
        key.to_string(),
    ))
}

// ── Discovery ─────────────────────────────────────────────────────────

/// A link a search found.
#[derive(Debug, Clone)]
struct Hit {
    url: String,
    title: String,
    snippet: Option<String>,
    /// The discovery surface that found it ("web", "linkedin", …).
    via: String,
    company: Option<String>,
    location: Option<String>,
    /// The job as a list API returned it (nothing left to read).
    record: Option<Box<JobRecord>>,
}

struct Discovered {
    hits: Vec<Hit>,
    queries: u32,
}

fn place_text(f: &SearchFilters) -> String {
    f.locations
        .first()
        .map(|l| {
            [l.city.clone(), l.region.clone(), l.country.clone()]
                .into_iter()
                .flatten()
                .next()
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

fn wants(f: &SearchFilters, id: &str) -> bool {
    f.sources.is_empty() || f.sources.iter().any(|s| s == id)
}

async fn discover(
    session: &Session,
    query: &str,
    also: &[String],
    f: &SearchFilters,
    deadline: Instant,
    cancel: &CancellationToken,
    coverage: &mut Coverage,
) -> Result<Discovered, ToolError> {
    let discovery = &session.discovery;
    let own = own_discovery(session, query, also, f, deadline, cancel);
    let service = async {
        match &discovery.service {
            Some(service) => {
                Some(service_discovery(session, service, query, f, deadline, cancel).await)
            }
            None => None,
        }
    };
    let provider = async {
        match &discovery.provider {
            Some((endpoint, model_id)) => Some(
                provider_discovery(session, endpoint, model_id, query, f, deadline, cancel).await,
            ),
            None => None,
        }
    };
    let (own, service, provider) = tokio::join!(own, service, provider);
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    coverage.backend = discovery.name();
    let mut hits = Vec::new();
    let mut queries = 0;
    let mut answered = false;
    let mut reasons = Vec::new();
    for part in [Some(own), service, provider].into_iter().flatten() {
        coverage.sources_searched.extend(part.searched);
        coverage.sources_unavailable.extend(part.unavailable);
        coverage.bounded_by.extend(part.bounded);
        match part.result {
            Ok(found) => {
                answered = true;
                queries += found.queries;
                hits.extend(found.hits);
            }
            Err(reason) => reasons.push(reason),
        }
    }
    coverage.sources_searched.dedup();
    coverage.backend_queries = queries;
    if !answered {
        return Err(ToolError::new(
            ErrorCode::SearchBackendUnavailable,
            if reasons.is_empty() {
                "no source could be searched".to_string()
            } else {
                reasons.join("; ")
            },
        ));
    }
    Ok(Discovered { hits, queries })
}

/// What one discovery route contributed.
struct Part {
    result: Result<Discovered, String>,
    searched: Vec<String>,
    unavailable: Vec<SourceIssue>,
    /// Why coverage stopped ("query_budget").
    bounded: Vec<String>,
}

/// ReMa's own job sources: complete records from list APIs.
async fn own_discovery(
    session: &Session,
    query: &str,
    also: &[String],
    f: &SearchFilters,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Part {
    let place = f.locations.first().and_then(|l| {
        let text = [l.city.clone(), l.region.clone(), l.country.clone()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
        crate::career_search::plan::Place::from_text(&text)
    });
    let mut roles = vec![query.to_string()];
    roles.extend(also.iter().cloned());
    let ask = crate::career_search::jobs::JobAsk {
        roles,
        place,
        remote: f.work_modes.contains(&WorkMode::Remote),
        companies: f.companies.clone(),
    };
    let ctx = session.ctx(cancel, deadline, false);
    let listed =
        crate::career_search::jobs::list(&session.state, &ctx, &ask, &session.state.career.health)
            .await;
    let now = now_ms();
    for source in &listed.searched {
        session.state.rema_mcp.record(source, now, Ok(()));
    }
    let unavailable: Vec<SourceIssue> = listed
        .failed
        .iter()
        .map(|(source, code, message)| {
            session
                .state
                .rema_mcp
                .record(source, now, Err(message.clone()));
            SourceIssue {
                source: source.clone(),
                code: *code,
                message: message.clone(),
            }
        })
        .chain(listed.resting.iter().map(|source| SourceIssue {
            source: source.clone(),
            code: ErrorCode::SourceUnavailable,
            message: "resting after repeated failures; asked again shortly".into(),
        }))
        .collect();
    let hits: Vec<Hit> = listed
        .records
        .into_iter()
        .filter(|(via, _)| wants(f, via) || f.sources.is_empty())
        .filter_map(|(via, record)| {
            let url = record
                .links
                .canonical_url
                .clone()
                .or_else(|| record.links.discovered.first().map(|d| d.url.clone()))?;
            Some(Hit {
                title: record.title.clone(),
                snippet: None,
                company: record.employer.name.clone(),
                location: record.locations.first().map(|l| l.text.clone()),
                via,
                url,
                record: Some(Box::new(record)),
            })
        })
        .collect();
    let result = if listed.searched.is_empty() && !listed.failed.is_empty() {
        Err(format!(
            "{OWN_SOURCES}: {}",
            listed
                .failed
                .first()
                .map(|(_, _, m)| m.clone())
                .unwrap_or_default()
        ))
    } else {
        Ok(Discovered {
            queries: listed.searched.len() as u32,
            hits,
        })
    };
    Part {
        result,
        searched: listed.searched,
        unavailable,
        bounded: Vec::new(),
    }
}

/// Site-scoped searches with the search service from Settings.
async fn service_discovery(
    session: &Session,
    service: &Arc<Service>,
    query: &str,
    f: &SearchFilters,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Part {
    let place = place_text(f);
    let base = format!("{query} {place}").trim().to_string();
    let countries: Vec<String> = f
        .locations
        .iter()
        .filter_map(|l| l.country.clone())
        .collect();
    let mut plan: Vec<(String, Option<String>, String)> = Vec::new();
    if wants(f, "web") {
        plan.push(("web".into(), None, format!("{base} job")));
    }
    // Employer boards and the big networks first, then regional boards,
    // then the remaining ATS hosts.
    let priority = |id: &str| match id {
        "greenhouse" => 0,
        "lever" => 1,
        "linkedin" => 2,
        "xing" => 3,
        "ashby" => 5,
        "personio" => 6,
        _ => 4,
    };
    let mut scoped = sources::discovery_sources(&countries);
    scoped.sort_by_key(|s| priority(s.id));
    for source in scoped {
        if wants(f, source.id) {
            plan.push((
                source.id.into(),
                source.site.map(str::to_string),
                base.clone(),
            ));
        }
    }
    let reserve = if plan.iter().any(|(id, ..)| id == "linkedin" || id == "xing") {
        CANONICAL_LOOKUPS
    } else {
        0
    };
    let mut bounded = Vec::new();
    if plan.len() > MAX_QUERIES - reserve {
        plan.truncate(MAX_QUERIES - reserve);
        bounded.push("query_budget".to_string());
    }
    let days = f.posted_within_days;
    let results: Vec<(String, Result<Vec<retrieval::backend::Hit>, String>)> =
        stream::iter(plan.into_iter().map(|(id, site, text)| {
            let service = service.clone();
            let cancel = cancel.clone();
            async move {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let result = tokio::time::timeout(
                    remaining,
                    service.search_in(&text, days, site.as_deref(), &cancel),
                )
                .await
                .unwrap_or_else(|_| Err("no answer in time".into()));
                (id, result)
            }
        }))
        .buffered(3)
        .collect()
        .await;
    let mut hits = Vec::new();
    let mut ok = 0;
    let mut searched = Vec::new();
    let mut unavailable = Vec::new();
    for (id, result) in results {
        match result {
            Ok(found) => {
                ok += 1;
                searched.push(id.clone());
                hits.extend(found.into_iter().map(|h| Hit {
                    url: h.url,
                    title: h.title,
                    snippet: h.snippet,
                    via: id.clone(),
                    company: None,
                    location: None,
                    record: None,
                }));
            }
            Err(message) => unavailable.push(SourceIssue {
                code: backend_code(&message),
                source: id,
                message: format!("{}: {message}", service.name()),
            }),
        }
    }
    let _ = session;
    let result = if ok == 0 {
        Err(unavailable
            .first()
            .map(|i| i.message.clone())
            .unwrap_or_else(|| format!("{}: no search ran", service.name())))
    } else {
        Ok(Discovered { hits, queries: ok })
    };
    Part {
        result,
        searched,
        unavailable,
        bounded,
    }
}

/// The chat model's own web search, in one bounded request.
async fn provider_discovery(
    session: &Session,
    endpoint: &Endpoint,
    model_id: &str,
    query: &str,
    f: &SearchFilters,
    deadline: Instant,
    cancel: &CancellationToken,
) -> Part {
    let place = place_text(f);
    let salary = match (f.salary_min, f.salary_currency.as_deref(), f.salary_period) {
        (Some(amount), Some(currency), Some(SalaryPeriod::Year)) => {
            Some(retrieval::intent::MinSalary {
                amount,
                currency: normalize::currency(&currency.to_lowercase()),
                period: crate::models::analytics::SalaryPeriod::Year,
            })
        }
        (Some(amount), Some(currency), Some(SalaryPeriod::Month)) => {
            Some(retrieval::intent::MinSalary {
                amount,
                currency: normalize::currency(&currency.to_lowercase()),
                period: crate::models::analytics::SalaryPeriod::Month,
            })
        }
        _ => None,
    };
    let job_query = JobQuery {
        text: format!("Find current {query} jobs {place}")
            .trim()
            .to_string(),
        role: Some(query.to_string()),
        location: (!place.is_empty()).then_some(place),
        company: f.companies.first().cloned(),
        remote: f.work_modes.contains(&WorkMode::Remote),
        posted_within_days: f.posted_within_days,
        min_salary: salary,
        verify_urls: Vec::new(),
    };
    let plan = crate::career_search::plan::for_job_query(&job_query);
    let hints = crate::career_search::plan::hints(&plan, &[]);
    let remaining = deadline.saturating_duration_since(Instant::now());
    // A bounded request with only the query's terms: no chat, no CV,
    // no tools (so ReMa MCP can never call itself).
    let found = tokio::time::timeout(
        remaining,
        retrieval::native::search(
            &session.state,
            endpoint,
            model_id,
            &job_query,
            &hints,
            &retrieval::Silent,
            cancel,
        ),
    )
    .await;
    let engine = retrieval::native::engine_name(endpoint).to_string();
    let (result, searched) = match found {
        Err(_) => (
            Err(format!(
                "{engine} did not finish within {} seconds",
                remaining.as_secs()
            )),
            Vec::new(),
        ),
        Ok(Err(reason)) => (Err(reason), Vec::new()),
        Ok(Ok(found)) => {
            let queries = found.searches as u32;
            let hits = found
                .candidates
                .into_iter()
                .map(|c| Hit {
                    url: c.url,
                    title: c.title.unwrap_or_default(),
                    snippet: c.snippet,
                    via: "web".into(),
                    company: c.company,
                    location: c.location,
                    record: None,
                })
                .collect();
            (Ok(Discovered { hits, queries }), vec!["web".to_string()])
        }
    };
    let unavailable = match &result {
        Err(reason) => vec![SourceIssue {
            source: "web".into(),
            code: backend_code(reason),
            message: reason.clone(),
        }],
        Ok(_) => Vec::new(),
    };
    Part {
        result,
        searched,
        unavailable,
        bounded: Vec::new(),
    }
}

fn backend_code(message: &str) -> ErrorCode {
    let lower = message.to_lowercase();
    if lower.contains("rate limit") || lower.contains("quota") {
        ErrorCode::RateLimited
    } else if lower.contains("in time") {
        ErrorCode::Timeout
    } else if lower.contains("stopped") {
        ErrorCode::Cancelled
    } else {
        ErrorCode::SourceUnavailable
    }
}

/// "Senior AI Engineer - Nordlicht AI - LinkedIn", "Nordlicht AI hiring
/// Senior AI Engineer in Vienna | LinkedIn" → (title, employer, place).
fn split_listing_title(title: &str, source_name: &str) -> (String, Option<String>, Option<String>) {
    let mut text = title.trim().to_string();
    for sep in [" | ", " - ", " – ", " — "] {
        if let Some(stripped) = text.strip_suffix(&format!("{sep}{source_name}")) {
            text = stripped.trim().to_string();
        }
        if let Some(pos) = text.rfind(sep) {
            if text[pos + sep.len()..]
                .to_lowercase()
                .contains(&source_name.to_lowercase())
            {
                text = text[..pos].trim().to_string();
            }
        }
    }
    if let Some((company, rest)) = text.split_once(" hiring ") {
        let (role, place) = match rest.rsplit_once(" in ") {
            Some((role, place)) => (role.trim(), Some(place.trim().to_string())),
            None => (rest.trim(), None),
        };
        return (role.to_string(), Some(company.trim().to_string()), place);
    }
    for sep in [" - ", " – ", " — ", " | ", " bei ", " at "] {
        if let Some((role, company)) = text.split_once(sep) {
            let company = company
                .split([' ', '|', '-'])
                .next()
                .is_some_and(|w| !w.is_empty())
                .then(|| {
                    company
                        .split(['|', '–', '—'])
                        .next()
                        .unwrap_or(company)
                        .trim()
                        .to_string()
                });
            return (role.trim().to_string(), company, None);
        }
    }
    (text, None, None)
}

fn snippet_record(hit: &Hit, target: &Classified, now: i64, why: Option<String>) -> JobRecord {
    let mut record = adapters::blank(target.source.mode);
    let (title, company, place) = split_listing_title(&hit.title, target.source.name);
    record.title = if title.is_empty() {
        extract::clip(&hit.url, 120)
    } else {
        extract::clip(&title, 200)
    };
    record.normalized_title = extract::normalized_title(&record.title);
    record.seniority = extract::seniority(&record.title);
    record.employer.name = hit.company.clone().or(company);
    let place = hit.location.clone().or(place);
    record.locations = place.iter().map(|p| extract::location(p)).collect();
    record.work_mode = extract::work_mode(&format!(
        "{} {}",
        record.title,
        place.as_deref().unwrap_or_default()
    ));
    record.quality.inferred = vec!["title".into(), "employer".into(), "locations".into()];
    if let Some(id) = &target.source_job_id {
        record
            .source_ids
            .push(adapters::source_id(target.source.id, id));
    }
    record.links.discovered.push(DiscoveredLink {
        source: hit.via.clone(),
        url: target.url.clone(),
    });
    record.description = match &hit.snippet {
        Some(snippet) => {
            let mut d = extract::description(Some(snippet.clone()), false, false);
            d.state = DescriptionState::SnippetOnly;
            d.lists = ListsStatus::NoDescription;
            d.requirements.clear();
            d.preferred.clear();
            d.benefits.clear();
            d.sections.clear();
            d
        }
        None => extract::description(None, false, false),
    };
    adapters::evidence(
        &mut record,
        "title",
        target.source.id,
        &target.url,
        "search_result",
        None,
        now,
    );
    record.quality.availability = Availability::Unknown;
    record.quality.availability_basis = Some(match why {
        Some(why) => format!("only seen in search results; the page could not be read ({why})"),
        None if target.target == Target::DiscoveryOnly => format!(
            "only seen in search results; {} links are discovery-only",
            target.source.name
        ),
        None => "only seen in search results".into(),
    });
    record
}

// ── Pipeline ──────────────────────────────────────────────────────────

struct Candidate {
    hit: Hit,
    target: Classified,
}

fn candidate_key(target: &Classified) -> String {
    match &target.source_job_id {
        Some(id) => format!("{}:{}", target.source.id, id.to_lowercase()),
        None => normalize::canonical_url(&target.url).unwrap_or_else(|| target.url.clone()),
    }
}

fn cached_for(session: &Session, target: &Classified) -> Option<JobRecord> {
    let mut keys = Vec::new();
    if let Some(id) = &target.source_job_id {
        keys.push(format!("src:{}:{}", target.source.id, id.to_lowercase()));
    }
    if let Some(url) = normalize::canonical_url(&target.url) {
        keys.push(format!("url:{url}"));
    }
    session
        .state
        .db
        .call(|c| match store::find(c, &keys)? {
            Some(id) => store::get(c, &id),
            None => Ok(None),
        })
        .ok()
        .flatten()
}

fn recently_checked(record: &JobRecord, now: i64) -> bool {
    record
        .dates
        .last_checked_at
        .as_deref()
        .and_then(|t| t.parse::<jiff::Timestamp>().ok())
        .is_some_and(|t| now - t.as_millisecond() < STATUS_TTL_MS)
}

/// Pages that could not be read, per source: (code, first message, count).
type Unreadable = BTreeMap<&'static str, (ErrorCode, String, u32)>;

struct Collected {
    records: Vec<JobRecord>,
    excluded: Excluded,
    fetched: u32,
    warnings: Vec<Warning>,
    unreadable: Unreadable,
    /// Candidate keys already handled, so a later pass skips them.
    keys: HashSet<String>,
    partial: bool,
}

async fn collect(
    session: &Session,
    hits: Vec<Hit>,
    f: &SearchFilters,
    deadline: Instant,
    refresh: bool,
    cancel: &CancellationToken,
    coverage: &mut Coverage,
) -> Result<Collected, ToolError> {
    let now = now_ms();
    let mut excluded = Excluded::default();
    let mut warnings = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for hit in hits {
        let Ok(target) = sources::classify_with(&hit.url, session.local()) else {
            continue;
        };
        let chosen = f.sources.is_empty()
            || f.sources.iter().any(|s| s == target.source.id)
            || (target.source.id == "web" && wants(f, "web"));
        if !chosen {
            continue;
        }
        // A job a list API returned is a posting by construction.
        if hit.record.is_none()
            && (retrieval::listings::is_search_page(&target.url)
                || (target.target == Target::Page && !normalize::is_job_specific(&target.url)))
        {
            excluded.not_job_postings += 1;
            continue;
        }
        let key = hit
            .record
            .as_ref()
            .and_then(|r| r.source_ids.first())
            .map(|s| format!("{}:{}", s.source, s.id.to_lowercase()))
            .unwrap_or_else(|| candidate_key(&target));
        if let Some(&index) = seen.get(&key) {
            let existing: &mut Candidate = &mut candidates[index];
            if existing.hit.snippet.is_none() {
                existing.hit.snippet = hit.snippet.clone();
            }
            if existing.hit.record.is_none() && hit.record.is_some() {
                existing.hit.record = hit.record;
            }
            excluded.duplicates += 1;
            continue;
        }
        if candidates.len() >= MAX_CANDIDATES {
            if !coverage
                .bounded_by
                .contains(&"candidate_budget".to_string())
            {
                coverage.bounded_by.push("candidate_budget".into());
            }
            break;
        }
        seen.insert(key, candidates.len());
        candidates.push(Candidate { hit, target });
    }
    coverage.candidates_seen += candidates.len() as u32;

    // Jobs list APIs returned complete: nothing to read.
    let mut results: HashMap<usize, Result<JobRecord, ToolError>> = HashMap::new();
    for (i, candidate) in candidates.iter_mut().enumerate() {
        if let Some(record) = candidate.hit.record.take() {
            results.insert(i, Ok(*record));
        }
    }
    // Readable candidates, ATS first; cached recent checks are reused.
    let mut readable: Vec<usize> = (0..candidates.len())
        .filter(|&i| !results.contains_key(&i))
        .filter(|&i| candidates[i].target.target != Target::DiscoveryOnly)
        .collect();
    readable.sort_by_key(|&i| (candidates[i].target.target == Target::Page, i));
    let mut to_fetch = Vec::new();
    for i in readable {
        if !refresh {
            if let Some(cached) = cached_for(session, &candidates[i].target) {
                if recently_checked(&cached, now) {
                    results.insert(i, Ok(cached));
                    continue;
                }
            }
        }
        to_fetch.push(i);
    }
    if to_fetch.len() > MAX_FETCHES {
        to_fetch.truncate(MAX_FETCHES);
        coverage.bounded_by.push("fetch_budget".into());
    }
    let ctx = session.ctx(cancel, deadline, refresh);
    let jobs: Vec<(usize, Classified, String)> = to_fetch
        .iter()
        .map(|&i| {
            (
                i,
                candidates[i].target.clone(),
                candidates[i].hit.title.clone(),
            )
        })
        .collect();
    let fetched: Vec<(usize, Result<JobRecord, ToolError>)> =
        stream::iter(jobs.into_iter().map(|(i, target, title)| {
            let ctx = &ctx;
            async move {
                if Instant::now() >= deadline {
                    return (
                        i,
                        Err(ToolError::new(
                            ErrorCode::Timeout,
                            "not checked before the deadline",
                        )),
                    );
                }
                (i, adapters::read(ctx, &target, &title).await)
            }
        }))
        .buffer_unordered(FETCH_CONCURRENCY)
        .collect()
        .await;
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    let mut late = 0;
    for (i, result) in fetched {
        let source = candidates[i].target.source.id;
        match &result {
            Ok(_) => session.state.rema_mcp.record(source, now, Ok(())),
            Err(e) if e.code == ErrorCode::Timeout => late += 1,
            Err(e) if e.code != ErrorCode::NotAJobPosting && e.code != ErrorCode::JobNotFound => {
                session
                    .state
                    .rema_mcp
                    .record(source, now, Err(e.message.clone()))
            }
            _ => {}
        }
        results.insert(i, result);
    }
    let fetched_count = to_fetch.len() as u32 - late;
    let partial = late > 0;
    if partial {
        coverage.bounded_by.push("deadline".into());
        warnings.push(Warning {
            code: ErrorCode::PartialResult,
            message: format!(
                "{late} candidate(s) were not checked within the {}-second limit",
                deadline_seconds(&session.discovery)
            ),
            source: None,
        });
    }

    let mut records = Vec::new();
    let mut unreadable = Unreadable::new();
    for (i, candidate) in candidates.iter().enumerate() {
        let record = match results.remove(&i) {
            Some(Ok(mut record)) => {
                let link = DiscoveredLink {
                    source: candidate.hit.via.clone(),
                    url: candidate.target.url.clone(),
                };
                if record
                    .links
                    .canonical_url
                    .as_deref()
                    .and_then(normalize::canonical_url)
                    != normalize::canonical_url(&link.url)
                    && !record.links.discovered.contains(&link)
                {
                    record.links.discovered.push(link);
                }
                record
            }
            Some(Err(e)) => match e.code {
                ErrorCode::NotAJobPosting => {
                    excluded.not_job_postings += 1;
                    continue;
                }
                ErrorCode::JobNotFound | ErrorCode::JobExpired => {
                    excluded.closed_or_unavailable += 1;
                    continue;
                }
                ErrorCode::Cancelled => return Err(cancelled()),
                code => {
                    let entry = unreadable.entry(candidate.target.source.id).or_insert((
                        code,
                        e.message.clone(),
                        0,
                    ));
                    entry.2 += 1;
                    snippet_record(&candidate.hit, &candidate.target, now, Some(e.message))
                }
            },
            None => snippet_record(&candidate.hit, &candidate.target, now, None),
        };
        records.push(record);
    }
    // Store under stable ids; merge sightings of the same job.
    let stored: Vec<JobRecord> = session
        .state
        .db
        .call(|c| {
            records
                .into_iter()
                .map(|r| store::upsert(c, r, now))
                .collect()
        })
        .map_err(db_error)?;
    let mut by_id: Vec<JobRecord> = Vec::new();
    for record in stored {
        if let Some(existing) = by_id.iter_mut().find(|r| r.id == record.id) {
            for link in record.links.discovered {
                if !existing.links.discovered.contains(&link) {
                    existing.links.discovered.push(link);
                }
            }
            excluded.duplicates += 1;
        } else {
            by_id.push(record);
        }
    }
    Ok(Collected {
        records: by_id,
        excluded,
        fetched: fetched_count,
        warnings,
        unreadable,
        keys: seen.into_keys().collect(),
        partial,
    })
}

/// Searches for the employer's own posting of discovery-only results
/// (at most two queries, search services only).
async fn canonical_lookups(
    session: &Session,
    records: &[JobRecord],
    deadline: Instant,
    cancel: &CancellationToken,
    coverage: &mut Coverage,
) -> Vec<Hit> {
    let Some(service) = &session.discovery.service else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    let lookups = records
        .iter()
        .filter(|r| r.quality.acquisition == AcquisitionMode::SearchDiscoveryOnly)
        .filter_map(|r| Some((r.title.clone(), r.employer.name.clone()?)))
        .take(CANONICAL_LOOKUPS);
    for (title, employer) in lookups {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || cancel.is_cancelled() {
            break;
        }
        let query = format!("\"{title}\" {employer} careers");
        if let Ok(Ok(found)) =
            tokio::time::timeout(remaining, service.search_in(&query, None, None, cancel)).await
        {
            coverage.backend_queries += 1;
            hits.extend(
                found
                    .into_iter()
                    .filter(|h| {
                        sources::classify_with(&h.url, session.local())
                            .is_ok_and(|c| c.target != Target::DiscoveryOnly)
                    })
                    .map(|h| Hit {
                        url: h.url,
                        title: h.title,
                        snippet: h.snippet,
                        via: "web".into(),
                        company: None,
                        location: None,
                        record: None,
                    }),
            );
        }
    }
    hits
}

fn deadline_seconds(d: &Discovery) -> u64 {
    d.deadline().as_secs()
}

/// Same employer, same role and same city under different ids: probably one
/// vacancy seen twice, but kept apart (a false merge would lose a job).
fn mark_possible_duplicates(session: &Session, records: &mut [JobRecord]) {
    let key = |r: &JobRecord| -> Option<String> {
        let employer = normalize::company_key(r.employer.name.as_deref()?);
        let title = r.normalized_title.clone()?;
        let city = r
            .locations
            .first()
            .and_then(|l| l.city.clone())
            .unwrap_or_default();
        (!employer.is_empty()).then(|| format!("{employer}|{title}|{}", city.to_lowercase()))
    };
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, r) in records.iter().enumerate() {
        if let Some(k) = key(r) {
            groups.entry(k).or_default().push(i);
        }
    }
    for members in groups.values().filter(|m| m.len() > 1) {
        for &a in members {
            for &b in members {
                if a != b {
                    let other = records[b].id.clone();
                    if !records[a].quality.possible_duplicates.contains(&other) {
                        records[a].quality.possible_duplicates.push(other.clone());
                        let this = records[a].id.clone();
                        let _ = session
                            .state
                            .db
                            .call(|c| store::mark_possible_duplicates(c, &this, &other));
                    }
                }
            }
        }
    }
}

fn summary(record: &JobRecord, relevance: Relevance, mut notes: Vec<String>) -> JobSummary {
    let needs_verification = matches!(
        record.description.state,
        DescriptionState::SnippetOnly | DescriptionState::Missing
    ) || record.quality.acquisition
        == AcquisitionMode::SearchDiscoveryOnly;
    if needs_verification {
        notes.push("Needs verification: only a search result was available".into());
    }
    if !record.quality.possible_duplicates.is_empty() {
        notes.push(format!(
            "May be the same vacancy as {}",
            record.quality.possible_duplicates.join(", ")
        ));
    }
    for conflict in &record.quality.conflicts {
        notes.push(format!("Sources disagree on {}", conflict.field));
    }
    let url = record
        .links
        .canonical_url
        .clone()
        .or_else(|| record.links.discovered.first().map(|d| d.url.clone()))
        .unwrap_or_default();
    let source = record
        .source_ids
        .first()
        .map(|s| s.source.clone())
        .filter(|s| sources::get(s).is_some())
        .or_else(|| record.links.discovered.first().map(|d| d.source.clone()))
        .unwrap_or_else(|| "web".into());
    JobSummary {
        id: record.id.clone(),
        title: record.title.clone(),
        employer: record.employer.name.clone(),
        locations: record.locations.iter().map(|l| l.text.clone()).collect(),
        work_mode: record.work_mode,
        remote_eligibility: record.remote_eligibility.clone(),
        seniority: record.seniority,
        employment_type: record.employment_type,
        working_time: record.working_time,
        salary: record.compensation.as_ref().map(|c| SalarySummary {
            text: c.text.clone(),
            min: c.min,
            max: c.max,
            currency: c.currency.clone(),
            period: c.period,
            bound: c.bound,
            estimate: c.estimate,
        }),
        posted_at: record.dates.posted_at.clone(),
        last_checked_at: record.dates.last_checked_at.clone(),
        availability: record.quality.availability,
        description_state: record.description.state,
        url,
        source,
        acquisition: record.quality.acquisition,
        discovered_from: record
            .links
            .discovered
            .iter()
            .filter(|d| Some(&d.url) != record.links.canonical_url.as_ref())
            .cloned()
            .collect(),
        needs_verification,
        notes,
        relevance,
    }
}

// ── search_jobs / search_similar_jobs ─────────────────────────────────

struct SimilarTo {
    basis: SimilarBasis,
    exclude: HashSet<String>,
}

pub async fn search_jobs(
    session: &Session,
    input: &SearchJobsInput,
    cancel: &CancellationToken,
) -> Result<SearchResult, ToolError> {
    let req = validate(input)?;
    run(session, req, None, cancel, true).await
}

/// ReMa's own job search (the career search router): the same pipeline as
/// the `search_jobs` tool. The MCP switch in Settings turns off the tools
/// offered to models, not ReMa's built-in job search.
pub async fn search_for_rema(
    session: &Session,
    req: Request,
    cancel: &CancellationToken,
) -> Result<SearchResult, ToolError> {
    run(session, req, None, cancel, false).await
}

async fn run(
    session: &Session,
    req: Request,
    similar: Option<SimilarTo>,
    cancel: &CancellationToken,
    tool_call: bool,
) -> Result<SearchResult, ToolError> {
    let key = query_key(
        session,
        &req,
        similar.as_ref().map(|s| s.basis.seed_id.as_str()),
    );
    let now = now_ms();
    if let Some(cursor) = &req.cursor {
        let (snapshot_id, offset, key16) = decode_cursor(cursor)?;
        if key16 != key[..16] {
            return Err(ToolError::invalid(
                "the cursor belongs to a different search; repeat the same query and filters",
            ));
        }
        return page(session, &snapshot_id, offset, req.limit, true, now);
    }
    if !req.refresh {
        let fresh = session
            .state
            .db
            .call(|c| store::fresh_snapshot(c, &key, now))
            .map_err(db_error)?;
        if let Some(id) = fresh {
            return page(session, &id, 0, req.limit, true, now);
        }
    }
    if tool_call && !super::is_enabled(&session.state) {
        return Err(disabled());
    }
    let deadline = Instant::now() + session.discovery.deadline();
    let mut coverage = Coverage::default();
    let discovered = discover(
        session,
        &req.query,
        &req.also,
        &req.filters,
        deadline,
        cancel,
        &mut coverage,
    )
    .await?;
    let _ = discovered.queries;
    let collected = collect(
        session,
        discovered.hits,
        &req.filters,
        deadline,
        req.refresh,
        cancel,
        &mut coverage,
    )
    .await?;
    let mut collected = collected;
    // LinkedIn/XING results: look for the employer's own posting (kept
    // apart; possible duplicates are marked, never merged).
    let lookups =
        canonical_lookups(session, &collected.records, deadline, cancel, &mut coverage).await;
    // Pages the first pass already handled are not read again.
    let found = lookups.len();
    let lookups: Vec<Hit> = lookups
        .into_iter()
        .filter(|hit| {
            sources::classify_with(&hit.url, session.local())
                .map_or(true, |t| !collected.keys.contains(&candidate_key(&t)))
        })
        .collect();
    collected.excluded.duplicates += (found - lookups.len()) as u32;
    if !lookups.is_empty() {
        if let Ok(extra) = collect(
            session,
            lookups,
            &req.filters,
            deadline,
            req.refresh,
            cancel,
            &mut coverage,
        )
        .await
        {
            collected.fetched += extra.fetched;
            collected.excluded.duplicates += extra.excluded.duplicates;
            collected.excluded.not_job_postings += extra.excluded.not_job_postings;
            collected.excluded.closed_or_unavailable += extra.excluded.closed_or_unavailable;
            collected.partial |= extra.partial;
            collected.warnings.extend(extra.warnings);
            for (source, (code, message, count)) in extra.unreadable {
                collected
                    .unreadable
                    .entry(source)
                    .or_insert((code, message, 0))
                    .2 += count;
            }
            for record in extra.records {
                if !collected.records.iter().any(|r| r.id == record.id) {
                    collected.records.push(record);
                }
            }
        }
    }
    mark_possible_duplicates(session, &mut collected.records);
    coverage.details_fetched = collected.fetched;
    let mut warnings = collected.warnings;
    for (source, (code, message, count)) in collected.unreadable {
        warnings.push(Warning {
            code,
            message: format!("{count} page(s) could not be read: {message}"),
            source: Some(source.to_string()),
        });
    }
    for issue in &coverage.sources_unavailable {
        warnings.push(Warning {
            code: issue.code,
            message: issue.message.clone(),
            source: Some(issue.source.clone()),
        });
    }
    let partial = collected.partial || !coverage.sources_unavailable.is_empty();
    if !coverage.sources_unavailable.is_empty()
        && !warnings.iter().any(|w| w.code == ErrorCode::PartialResult)
    {
        warnings.push(Warning {
            code: ErrorCode::PartialResult,
            message: "some sources could not be searched; results are incomplete".into(),
            source: None,
        });
    }
    if session.discovery.service.is_some() {
        coverage.bounded_by.push("sources".into());
    }
    if coverage
        .sources_searched
        .iter()
        .any(|s| s == "linkedin" || s == "xing")
    {
        coverage.bounded_by.push("permissions".into());
    }

    let mut excluded = collected.excluded;
    let mut matches: Vec<(Relevance, JobRecord, Vec<String>)> = Vec::new();
    let mut unresolved: Vec<(Relevance, JobRecord, Vec<String>)> = Vec::new();
    let mut unresolved_filters: Vec<String> = Vec::new();
    for record in collected.records {
        if let Some(s) = &similar {
            if s.exclude.contains(&record.id)
                || record
                    .quality
                    .possible_duplicates
                    .iter()
                    .any(|d| s.exclude.contains(d))
            {
                excluded.duplicates += 1;
                continue;
            }
        }
        // The query, or a close variant of its role that fits this job.
        let topic = std::iter::once(&req.query)
            .chain(req.also.iter())
            .find(|q| filter::on_topic(&record, q))
            .unwrap_or(&req.query)
            .clone();
        let relevance = filter::relevance(&record, &topic, &req.filters, now);
        match filter::check(&record, &topic, &req.filters, now) {
            Verdict::Match { notes } => matches.push((relevance, record, notes)),
            Verdict::Unresolved { filters, reasons } => {
                excluded.unresolved += 1;
                for f in filters {
                    if !unresolved_filters.contains(&f) {
                        unresolved_filters.push(f);
                    }
                }
                unresolved.push((relevance, record, reasons));
            }
            Verdict::Excluded { reason } => {
                if reason.starts_with("closed") {
                    excluded.closed_or_unavailable += 1;
                } else {
                    excluded.filtered_out += 1;
                }
            }
        }
    }
    let order = |items: Vec<(Relevance, JobRecord, Vec<String>)>| -> Vec<JobSummary> {
        let mut notes_by_id: HashMap<String, Vec<String>> = HashMap::new();
        let mut pairs: Vec<(Relevance, JobRecord)> = Vec::new();
        for (r, record, notes) in items {
            notes_by_id.insert(record.id.clone(), notes);
            pairs.push((r, record));
        }
        filter::sort(&mut pairs, req.sort, &req.filters);
        pairs
            .into_iter()
            .map(|(r, record)| {
                let notes = notes_by_id.remove(&record.id).unwrap_or_default();
                summary(&record, r, notes)
            })
            .collect()
    };
    let jobs = order(matches);
    let unresolved_jobs = if req.include_unresolved {
        order(unresolved)
    } else {
        Vec::new()
    };
    let envelope = SearchResult {
        schema_version: SCHEMA_VERSION,
        query: req.query.clone(),
        applied_filters: req.filters.clone(),
        unresolved_filters,
        jobs: Vec::new(),
        unresolved_candidates: Vec::new(),
        total_returned: 0,
        excluded,
        coverage,
        warnings,
        partial,
        searched_at: extract::iso(now),
        snapshot_expires_at: extract::iso(now + SNAPSHOT_TTL_MS),
        from_cache: false,
        next_cursor: None,
        similar_to: similar.map(|s| s.basis),
    };
    let snapshot_id = format!("rs_{}", &store::sha(&format!("{key}{now}"))[..20]);
    let snapshot = Snapshot {
        jobs,
        unresolved: unresolved_jobs,
        envelope: serde_json::to_value(&envelope).unwrap_or_default(),
    };
    session
        .state
        .db
        .call(|c| {
            store::save_snapshot(c, &snapshot_id, &key, &snapshot, now, now + SNAPSHOT_TTL_MS)
        })
        .map_err(db_error)?;
    page(session, &snapshot_id, 0, req.limit, false, now)
}

fn disabled() -> ToolError {
    ToolError::new(
        ErrorCode::SourceNotAuthorized,
        "ReMa MCP is turned off in Settings → MCP; it cannot run. Do not call it again.",
    )
}

/// One page of a stored search, within the payload limit.
fn page(
    session: &Session,
    snapshot_id: &str,
    offset: usize,
    limit: usize,
    from_cache: bool,
    now: i64,
) -> Result<SearchResult, ToolError> {
    let stored = session
        .state
        .db
        .call(|c| store::snapshot(c, snapshot_id))
        .map_err(db_error)?;
    let Some((snapshot, key, _, expires)) = stored else {
        return Err(ToolError::new(
            ErrorCode::CursorExpired,
            "the search has expired; search again",
        ));
    };
    if expires <= now && from_cache {
        return Err(ToolError::new(
            ErrorCode::CursorExpired,
            "the search has expired; search again",
        ));
    }
    let mut result: SearchResult = serde_json::from_value(snapshot.envelope)
        .map_err(|_| ToolError::new(ErrorCode::ParsingFailed, "the stored search is unreadable"))?;
    let total = snapshot.jobs.len();
    let mut take = limit.min(total.saturating_sub(offset));
    loop {
        result.jobs = snapshot
            .jobs
            .iter()
            .skip(offset)
            .take(take)
            .cloned()
            .collect();
        result.unresolved_candidates = if offset == 0 {
            snapshot.unresolved.iter().take(limit).cloned().collect()
        } else {
            Vec::new()
        };
        result.total_returned = result.jobs.len() as u32;
        let next = offset + take;
        result.next_cursor = (next < total).then(|| encode_cursor(snapshot_id, next, &key));
        result.from_cache = from_cache;
        let size = serde_json::to_vec(&result).map(|v| v.len()).unwrap_or(0);
        if size <= MAX_SEARCH_PAYLOAD || take <= 1 {
            break;
        }
        take = take * 3 / 4;
        if !result
            .warnings
            .iter()
            .any(|w| w.code == ErrorCode::PayloadLimitReached)
        {
            result.warnings.push(Warning {
                code: ErrorCode::PayloadLimitReached,
                message: "fewer jobs in this page to stay within the size limit; use next_cursor"
                    .into(),
                source: None,
            });
        }
    }
    if !result.unresolved_candidates.is_empty() {
        result.unresolved_candidates.truncate(limit);
    }
    Ok(result)
}

pub async fn search_similar(
    session: &Session,
    input: &SimilarJobsInput,
    cancel: &CancellationToken,
) -> Result<SearchResult, ToolError> {
    let seed = resolve(session, &input.seed, false, cancel).await?.0;
    let query = seed
        .normalized_title
        .clone()
        .unwrap_or_else(|| seed.title.clone());
    let terms: Vec<String> = seed.description.skills.iter().take(5).cloned().collect();
    let search = SearchJobsInput {
        query: query.clone(),
        skills: terms.clone(),
        locations: input.locations.clone(),
        work_modes: input.work_modes.clone(),
        seniority: input.seniority.clone(),
        posted_within_days: input.posted_within_days,
        salary_min: input.salary_min,
        salary_currency: input.salary_currency.clone(),
        salary_period: input.salary_period,
        sources: input.sources.clone(),
        filter_mode: input.filter_mode,
        limit: input.limit,
        ..SearchJobsInput::default()
    };
    let req = validate(&search)?;
    let mut exclude: HashSet<String> = seed.quality.possible_duplicates.iter().cloned().collect();
    exclude.insert(seed.id.clone());
    run(
        session,
        req,
        Some(SimilarTo {
            basis: SimilarBasis {
                seed_id: seed.id.clone(),
                title: seed.title.clone(),
                terms,
            },
            exclude,
        }),
        cancel,
        true,
    )
    .await
}

// ── get_job / get_jobs ────────────────────────────────────────────────

fn one_ref(id: &Option<String>, url: &Option<String>) -> Result<JobRef, ToolError> {
    let id = id.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let url = url.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match (id, url) {
        (Some(id), None) => {
            if !id.starts_with("rj_") || id.len() > 40 {
                return Err(ToolError::invalid("a ReMa job id looks like \"rj_…\""));
            }
            Ok(JobRef {
                id: Some(id.to_string()),
                url: None,
            })
        }
        (None, Some(url)) => Ok(JobRef {
            id: None,
            url: Some(url.to_string()),
        }),
        _ => Err(ToolError::invalid("give exactly one of id or url")),
    }
}

/// A job's record: the cache when fresh, else read from its source. The
/// flags tell whether it came from the cache and whether it is stale.
async fn resolve(
    session: &Session,
    reference: &JobRef,
    refresh: bool,
    cancel: &CancellationToken,
) -> Result<(JobRecord, bool, bool, Vec<Warning>), ToolError> {
    let reference = one_ref(&reference.id, &reference.url)?;
    let now = now_ms();
    let (cached, target) = match (&reference.id, &reference.url) {
        (Some(id), _) => {
            let cached = session
                .state
                .db
                .call(|c| store::get(c, id))
                .map_err(db_error)?
                .ok_or_else(|| {
                    ToolError::new(
                        ErrorCode::JobNotFound,
                        "unknown ReMa job id; ids come from search_jobs results",
                    )
                })?;
            let url = cached
                .links
                .canonical_url
                .clone()
                .or_else(|| cached.links.discovered.first().map(|d| d.url.clone()));
            let target = url.and_then(|u| sources::classify_with(&u, session.local()).ok());
            (Some(cached), target)
        }
        (None, Some(url)) => {
            let target =
                sources::classify_with(url, session.local()).map_err(ToolError::invalid)?;
            (cached_for(session, &target), Some(target))
        }
        _ => unreachable!("one_ref returns one reference"),
    };
    let readable = target
        .as_ref()
        .is_some_and(|t| t.target != Target::DiscoveryOnly);
    if let Some(record) = &cached {
        let snippet_only = record.description.state == DescriptionState::SnippetOnly;
        if !refresh && (recently_checked(record, now) || (!readable && snippet_only)) {
            return Ok((record.clone(), true, false, Vec::new()));
        }
    }
    let Some(target) = target.filter(|_| readable) else {
        return match cached {
            Some(record) => Ok((record, true, false, Vec::new())),
            None => Err(ToolError::new(
                ErrorCode::BlockedBySourcePolicy,
                "this link is discovery-only (LinkedIn, XING or a job board): ReMa does not fetch \
                 it. Use search_jobs to find the employer's own posting.",
            )),
        };
    };
    if !super::is_enabled(&session.state) {
        return Err(disabled());
    }
    let deadline = Instant::now() + SERVICE_DEADLINE;
    let ctx = session.ctx(cancel, deadline, refresh);
    let title_hint = cached.as_ref().map(|c| c.title.clone()).unwrap_or_default();
    match adapters::read(&ctx, &target, &title_hint).await {
        Ok(mut record) => {
            session.state.rema_mcp.record(target.source.id, now, Ok(()));
            if let Some(old) = &cached {
                for link in &old.links.discovered {
                    if !record.links.discovered.contains(link) {
                        record.links.discovered.push(link.clone());
                    }
                }
            }
            let mut record = session
                .state
                .db
                .call(|c| store::upsert(c, record, now))
                .map_err(db_error)?;
            let mut warnings = Vec::new();
            if record.quality.availability == Availability::Closed {
                warnings.push(Warning {
                    code: ErrorCode::JobExpired,
                    message: record
                        .quality
                        .availability_basis
                        .clone()
                        .unwrap_or_else(|| "the posting is closed".into()),
                    source: Some(target.source.id.into()),
                });
            }
            record.dates.last_checked_at = Some(extract::iso(now));
            Ok((record, false, false, warnings))
        }
        Err(e) if e.code == ErrorCode::Cancelled => Err(e),
        Err(e) if e.code == ErrorCode::JobNotFound => match cached {
            Some(mut record) => {
                // A real check: the source no longer serves it.
                record.quality.availability = Availability::Unavailable;
                record.quality.availability_basis = Some(e.message.clone());
                record.dates.last_checked_at = Some(extract::iso(now));
                let record = session
                    .state
                    .db
                    .call(|c| store::upsert(c, record, now))
                    .map_err(db_error)?;
                Ok((
                    record,
                    false,
                    false,
                    vec![Warning {
                        code: ErrorCode::JobNotFound,
                        message: e.message,
                        source: Some(target.source.id.into()),
                    }],
                ))
            }
            None => Err(e),
        },
        Err(e) => {
            session
                .state
                .rema_mcp
                .record(target.source.id, now, Err(e.message.clone()));
            match cached {
                // Older evidence, labeled as such (original timestamps kept).
                Some(record) => Ok((
                    record,
                    true,
                    true,
                    vec![Warning {
                        code: ErrorCode::StaleCache,
                        message: format!(
                            "could not check the source again ({}); showing the last check",
                            e.message
                        ),
                        source: Some(target.source.id.into()),
                    }],
                )),
                None => Err(e),
            }
        }
    }
}

fn description_cursor(id: &str, offset: usize) -> String {
    URL_SAFE_NO_PAD.encode(format!("{id}:{offset}"))
}

fn detail(
    record: JobRecord,
    offset: usize,
    part: usize,
    from_cache: bool,
    stale: bool,
    warnings: Vec<Warning>,
) -> JobDetail {
    let mut job = record;
    let text = job.description.text.clone().unwrap_or_default();
    let total = text.chars().count();
    let offset = offset.min(total);
    let chunk: String = text.chars().skip(offset).take(part).collect();
    let length = chunk.chars().count();
    let next = offset + length;
    job.description.text = (!chunk.is_empty()).then_some(chunk);
    // Section texts repeat the description; the lists carry their items.
    for section in &mut job.description.sections {
        section.text.clear();
    }
    JobDetail {
        schema_version: SCHEMA_VERSION,
        description_part: DescriptionPart {
            offset: offset as u32,
            length: length as u32,
            total_length: total as u32,
            next_cursor: (next < total).then(|| description_cursor(&job.id, next)),
        },
        job,
        from_cache,
        stale,
        warnings,
    }
}

pub async fn get_job(
    session: &Session,
    input: &GetJobInput,
    cancel: &CancellationToken,
) -> Result<JobDetail, ToolError> {
    let reference = one_ref(&input.id, &input.url)?;
    let offset = match &input.description_cursor {
        None => None,
        Some(cursor) => {
            let bad = || ToolError::invalid("the description cursor is not valid");
            let raw = URL_SAFE_NO_PAD.decode(cursor.trim()).map_err(|_| bad())?;
            let text = String::from_utf8(raw).map_err(|_| bad())?;
            let (id, offset) = text.rsplit_once(':').ok_or_else(bad)?;
            Some((id.to_string(), offset.parse::<usize>().map_err(|_| bad())?))
        }
    };
    let refresh = input.refresh.unwrap_or(false) && offset.is_none();
    let (record, from_cache, stale, warnings) =
        resolve(session, &reference, refresh, cancel).await?;
    let start = match offset {
        Some((id, offset)) if id == record.id => offset,
        Some(_) => {
            return Err(ToolError::invalid(
                "the description cursor belongs to another job",
            ))
        }
        None => 0,
    };
    Ok(detail(
        record,
        start,
        DESCRIPTION_PART,
        from_cache,
        stale,
        warnings,
    ))
}

pub async fn get_jobs(
    session: &Session,
    input: &GetJobsInput,
    cancel: &CancellationToken,
) -> Result<BatchResult, ToolError> {
    if input.jobs.is_empty() || input.jobs.len() > MAX_BATCH {
        return Err(ToolError::invalid(format!("give 1–{MAX_BATCH} jobs")));
    }
    let refresh = input.refresh.unwrap_or(false);
    let results: Vec<BatchItem> =
        stream::iter(input.jobs.clone().into_iter().map(|reference| async move {
            match resolve(session, &reference, refresh, cancel).await {
                Ok((record, from_cache, stale, warnings)) => BatchItem {
                    reference,
                    ok: true,
                    detail: Some(detail(
                        record,
                        0,
                        BATCH_DESCRIPTION,
                        from_cache,
                        stale,
                        warnings,
                    )),
                    error: None,
                },
                Err(e) => BatchItem {
                    reference,
                    ok: false,
                    detail: None,
                    error: Some(e.body()),
                },
            }
        }))
        .buffered(3)
        .collect()
        .await;
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    let succeeded = results.iter().filter(|r| r.ok).count() as u32;
    Ok(BatchResult {
        schema_version: SCHEMA_VERSION,
        failed: results.len() as u32 - succeeded,
        succeeded,
        results,
    })
}

// ── source_status ─────────────────────────────────────────────────────

pub fn source_status(
    session: &Session,
    input: &SourceStatusInput,
) -> Result<SourceStatusResult, ToolError> {
    let wanted = clean_sources(&input.sources)?;
    let discovery = &session.discovery;
    let backend = BackendStatus {
        kind: match (&discovery.service, &discovery.provider) {
            (Some(_), _) => "rema_sources_and_search_service",
            (None, Some(_)) => "rema_sources_and_provider_web_search",
            (None, None) => "rema_sources",
        }
        .into(),
        name: discovery.name(),
        usable: true,
        detail: match (&discovery.service, &discovery.provider) {
            (Some(service), _) => format!(
                "ReMa's own job sources (employer boards and public job boards, no key) and {}",
                service.name()
            ),
            (None, Some((endpoint, _))) => format!(
                "ReMa's own job sources (employer boards and public job boards, no key) and {}",
                retrieval::native::engine_name(endpoint)
            ),
            (None, None) => {
                "ReMa's own job sources: employer boards and public job boards (no key needed)"
                    .into()
            }
        },
        deadline_seconds: discovery.deadline().as_secs() as u32,
    };
    let searchable = discovery.searches_the_web();
    let sources = sources::REGISTRY
        .iter()
        .filter(|s| wanted.is_empty() || wanted.iter().any(|w| w == s.id))
        .map(|s| {
            let health = session.state.rema_mcp.health(s.id);
            let (usable, limitation) = match s.mode {
                AcquisitionMode::SearchDiscoveryOnly if !searchable => (
                    false,
                    "its links are found only by a web search (the chat model's own, when it has \
                     one); known links still work"
                        .to_string(),
                ),
                AcquisitionMode::SearchDiscoveryOnly => (true, s.restrictions.to_string()),
                AcquisitionMode::Blocked => (false, s.restrictions.to_string()),
                _ => (true, s.restrictions.to_string()),
            };
            SourceState {
                id: s.id.into(),
                name: s.name.into(),
                acquisition: s.mode,
                discovery: s.discovery.into(),
                content: s.content.into(),
                implemented: s.implemented,
                usable,
                needs_credentials: s.needs_credentials,
                last_success_at: health.last_success.map(extract::iso),
                last_error: health.last_error.as_ref().map(|(_, m)| m.clone()),
                last_error_at: health.last_error.as_ref().map(|(t, _)| extract::iso(*t)),
                limitations: limitation,
            }
        })
        .collect();
    Ok(SourceStatusResult {
        schema_version: SCHEMA_VERSION,
        checked_at: extract::iso(now_ms()),
        search_backend: backend,
        sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_titles_from_search_results() {
        assert_eq!(
            split_listing_title(
                "Nordlicht AI hiring Senior AI Engineer in Vienna, Austria | LinkedIn",
                "LinkedIn"
            ),
            (
                "Senior AI Engineer".into(),
                Some("Nordlicht AI".into()),
                Some("Vienna, Austria".into())
            )
        );
        assert_eq!(
            split_listing_title("Senior AI Engineer - Nordlicht AI - LinkedIn", "LinkedIn"),
            (
                "Senior AI Engineer".into(),
                Some("Nordlicht AI".into()),
                None
            )
        );
        assert_eq!(
            split_listing_title("KI Engineer (m/w/d) | XING Jobs", "XING").0,
            "KI Engineer (m/w/d)"
        );
    }

    #[test]
    fn validates_inputs() {
        let base = SearchJobsInput {
            query: "AI Engineer".into(),
            ..SearchJobsInput::default()
        };
        assert!(validate(&base).is_ok());
        let bad = |f: &dyn Fn(&mut SearchJobsInput)| {
            let mut input = base.clone();
            f(&mut input);
            validate(&input).unwrap_err().code
        };
        assert_eq!(bad(&|i| i.query = " ".into()), ErrorCode::InvalidInput);
        assert_eq!(
            bad(&|i| i.salary_min = Some(96_000.0)),
            ErrorCode::InvalidInput,
            "no currency"
        );
        assert_eq!(bad(&|i| i.limit = Some(51)), ErrorCode::InvalidInput);
        assert_eq!(
            bad(&|i| i.sources = vec!["monster".into()]),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            bad(&|i| i.languages = vec!["german".into()]),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            bad(&|i| i.locations = vec![LocationFilter {
                city: None,
                region: None,
                country: None
            }]),
            ErrorCode::InvalidInput
        );
        let mut ok = base.clone();
        ok.locations = vec![LocationFilter {
            city: Some("Wien".into()),
            region: None,
            country: Some("AT".into()),
        }];
        ok.sources = vec!["karriere.at".into(), "LinkedIn".into()];
        let req = validate(&ok).unwrap();
        assert_eq!(req.filters.locations[0].city.as_deref(), Some("Vienna"));
        assert_eq!(req.filters.locations[0].country.as_deref(), Some("Austria"));
        assert_eq!(req.filters.sources, ["karriere_at", "linkedin"]);
    }

    #[test]
    fn cursors_round_trip_and_reject_tampering() {
        let key = store::sha("q");
        let cursor = encode_cursor("rs_abc", 20, &key);
        assert_eq!(
            decode_cursor(&cursor).unwrap(),
            ("rs_abc".into(), 20, key[..16].to_string())
        );
        assert!(decode_cursor("not-base64!").is_err());
        assert!(decode_cursor(&URL_SAFE_NO_PAD.encode("../../etc/passwd:0:x")).is_err());
    }
}
