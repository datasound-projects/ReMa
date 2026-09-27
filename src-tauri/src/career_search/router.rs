//! The CareerSearchRouter (§4–§6, §19, §33, §37): one entry point for every
//! feature that needs current career information. Callers never know which
//! route answered.
//!
//! **Jobs** — the Jobs MCP first (ReMa's own sources, no key, plus a search
//! service when one is set up), and at the same time the model's own web
//! search restricted to the registry's career sites and localized to the
//! request's place. Both results are verified, merged and deduplicated
//! (the employer's own posting wins over a board's copy). One route
//! failing is a fallback, not a failure; only when every route fails is
//! the request answered with [`super::UNAVAILABLE`].
//!
//! **Research** (companies, people, market) — ReMa's own sources
//! (Wikidata, Wikipedia, company sites, current postings) and the model's
//! own web search, merged into one evidence list.

use std::{collections::HashMap, sync::Arc, time::Instant};

use tokio_util::sync::CancellationToken;

use super::{
    plan::{self, SearchPlan},
    research::{self, Research, ResearchOutcome},
    RouteReport, UNAVAILABLE,
};
use crate::{
    analytics::normalize,
    llm::Endpoint,
    rema_mcp::{
        contract::{
            AcquisitionMode, Availability, DescriptionState, FilterMode, JobSummary,
            LocationFilter, SearchJobsInput, SortMode, WorkMode,
        },
        engine::{self, Discovery, Session},
        extract, sources, store,
    },
    retrieval::{
        self, listings, native, Excluded, JobQuery, Listing, Outcome, Progress, Retrieval,
        Verification,
    },
    state::AppState,
    time::now_ms,
};

/// How ReMa's own job route is named to users (run history, sources).
pub const REMA_JOBS: &str = "ReMa Jobs";
/// Most listings shown.
const MAX_LISTINGS: usize = 20;

/// What ReMa's own job sources returned, as listings.
#[derive(Default)]
struct OwnJobs {
    listings: Vec<Listing>,
    excluded: Excluded,
    /// Source registry ids that answered.
    searched: Vec<String>,
    /// "arbeitnow: the source did not answer in time".
    failed: Vec<String>,
    /// The sources that failed, by name.
    unreached: Vec<String>,
    lookups: usize,
    /// When the sources were read: a search repeated within minutes reuses
    /// the earlier one (the Jobs MCP's snapshot), and says so by its time.
    searched_at: i64,
}

/// The model's own search, checked.
struct WebJobs {
    engine: String,
    searches: usize,
    pages_read: usize,
    listings: Vec<Listing>,
    excluded: Excluded,
}

/// Registry names of the sources that answered ("Greenhouse", "Arbeitnow").
fn source_names(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        let name = match sources::get(id) {
            Some(s)
                if matches!(
                    s.id,
                    "greenhouse"
                        | "lever"
                        | "ashby"
                        | "personio"
                        | "smartrecruiters"
                        | "workable"
                        | "recruitee"
                ) =>
            {
                "Company career sites".to_string()
            }
            Some(s) => s.name.to_string(),
            None => id.clone(),
        };
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// The official posting beats a board's copy of the same vacancy (§33).
fn authority(url: &str) -> u8 {
    match sources::classify(url).map(|c| c.source.id) {
        Ok(
            "greenhouse" | "lever" | "ashby" | "personio" | "smartrecruiters" | "workable"
            | "recruitee" | "web",
        ) => 0,
        _ => 1,
    }
}

fn work_mode_label(mode: WorkMode) -> &'static str {
    match mode {
        WorkMode::Remote => "Remote",
        WorkMode::Hybrid => "Hybrid",
        WorkMode::Onsite => "On-site",
    }
}

/// A Jobs MCP result as a listing, with the request's date and salary rules.
fn listing_of(
    job: &JobSummary,
    description: Option<&str>,
    query: &JobQuery,
    now: i64,
) -> Result<Listing, listings::Drop> {
    let mut notes: Vec<String> = job
        .notes
        .iter()
        .map(|n| {
            n.trim_start_matches("Unverified: ")
                .trim_end_matches('.')
                .to_string()
        })
        .filter(|n| !n.starts_with("Needs verification"))
        .collect();
    if job.availability == Availability::Unknown
        && job.acquisition != AcquisitionMode::SearchDiscoveryOnly
    {
        notes.push("whether it is still open is not stated".into());
    }
    let posted = job
        .posted_at
        .as_deref()
        .and_then(extract::date)
        .map(|(_, ms)| ms);
    listings::check_date(posted, query.posted_within_days, now, &mut notes)?;
    let salary = job.salary.as_ref().map(|s| normalize::Salary {
        min: s.min,
        max: s.max,
        currency: s
            .currency
            .as_deref()
            .and_then(|c| normalize::currency(&c.to_lowercase())),
        period: s.period.map(|p| match p {
            crate::rema_mcp::contract::SalaryPeriod::Year => {
                crate::models::analytics::SalaryPeriod::Year
            }
            crate::rema_mcp::contract::SalaryPeriod::Month => {
                crate::models::analytics::SalaryPeriod::Month
            }
            crate::rema_mcp::contract::SalaryPeriod::Week => {
                crate::models::analytics::SalaryPeriod::Week
            }
            crate::rema_mcp::contract::SalaryPeriod::Day => {
                crate::models::analytics::SalaryPeriod::Day
            }
            crate::rema_mcp::contract::SalaryPeriod::Hour => {
                crate::models::analytics::SalaryPeriod::Hour
            }
        }),
    });
    let estimate = job.salary.as_ref().is_some_and(|s| s.estimate);
    let salary_status = listings::check_salary(
        salary.as_ref(),
        estimate,
        query.min_salary.as_ref(),
        &mut notes,
    )?;
    let verification = if job.needs_verification {
        Verification::SearchOnly
    } else if job.acquisition == AcquisitionMode::PermittedPublicPage
        && job.description_state != DescriptionState::Full
    {
        Verification::Page
    } else {
        Verification::Posting
    };
    let location = (!job.locations.is_empty()).then(|| job.locations.join(" / "));
    Ok(Listing {
        title: extract::clip(&job.title, 120),
        company: job.employer.clone(),
        location,
        url: job.url.clone(),
        summary: description.and_then(listings::summary),
        posted,
        // As ReMa read it ("€95k / year"); the source's words otherwise.
        salary: salary
            .as_ref()
            .filter(|s| s.min.is_some() || s.max.is_some())
            .map(normalize::format_salary)
            .or_else(|| job.salary.as_ref().map(|s| extract::clip(&s.text, 60))),
        salary_status,
        work_mode: job.work_mode.map(work_mode_label).map(str::to_string),
        verification,
        source: sources::get(&job.source)
            .map(|s| s.name.to_string())
            .unwrap_or_else(|| listings::source_label(&job.url)),
        notes,
    })
}

/// The Jobs MCP search for a request (ReMa's own sources, and a search
/// service when one is set up).
async fn own_jobs(
    state: &AppState,
    plan: &SearchPlan,
    query: &JobQuery,
    cancel: &CancellationToken,
) -> Result<OwnJobs, String> {
    let service = retrieval::backend::configured(state)
        .await
        .ok()
        .flatten()
        .map(Arc::new);
    let session = Session {
        state: state.clone(),
        discovery: Discovery {
            service,
            provider: None,
        },
    };
    let role = plan
        .roles
        .first()
        .cloned()
        .or_else(|| query.role.clone())
        .unwrap_or_else(|| extract::clip(&query.text, 80));
    let locations = match (&plan.place, query.remote) {
        (Some(place), false) => vec![LocationFilter {
            city: place.city.clone(),
            region: None,
            country: place.country.clone(),
        }],
        _ => Vec::new(),
    };
    let input = SearchJobsInput {
        query: extract::clip(&role, 200),
        companies: query.company.iter().cloned().collect(),
        locations,
        work_modes: if query.remote && plan.place.is_none() {
            vec![WorkMode::Remote]
        } else {
            Vec::new()
        },
        filter_mode: Some(FilterMode::Lenient),
        sort: Some(if plan.newest_first {
            SortMode::Newest
        } else {
            SortMode::Relevance
        }),
        limit: Some(50),
        ..SearchJobsInput::default()
    };
    let mut request = engine::validate(&input).map_err(|e| e.message)?;
    request.also = plan.roles.iter().skip(1).cloned().collect();
    let result = engine::search_for_rema(&session, request, cancel)
        .await
        .map_err(|e| e.message)?;
    let now = now_ms();
    let ids: Vec<String> = result.jobs.iter().map(|j| j.id.clone()).collect();
    let descriptions: HashMap<String, String> = state
        .db
        .call(move |c| {
            let mut out = HashMap::new();
            for id in ids {
                if let Some(record) = store::get(c, &id)? {
                    if let Some(text) = record.description.text {
                        out.insert(id, text);
                    }
                }
            }
            Ok(out)
        })
        .unwrap_or_default();
    let mut own = OwnJobs {
        searched: result.coverage.sources_searched.clone(),
        failed: result
            .coverage
            .sources_unavailable
            .iter()
            .map(|i| format!("{}: {}", i.source, i.message))
            .collect(),
        unreached: source_names(
            &result
                .coverage
                .sources_unavailable
                .iter()
                .map(|i| i.source.clone())
                .collect::<Vec<_>>(),
        ),
        lookups: result.coverage.backend_queries as usize,
        searched_at: if result.from_cache {
            result
                .searched_at
                .parse::<jiff::Timestamp>()
                .map(|t| t.as_millisecond())
                .ok()
                .filter(|ms| *ms <= now)
                .unwrap_or(now)
        } else {
            now
        },
        ..OwnJobs::default()
    };
    own.excluded.expired += result.excluded.closed_or_unavailable as usize;
    for job in &result.jobs {
        // Discovery-only links (LinkedIn, boards) have no checked facts.
        if job.acquisition == AcquisitionMode::SearchDiscoveryOnly && job.employer.is_none() {
            own.excluded.not_postings += 1;
            continue;
        }
        match listing_of(
            job,
            descriptions.get(&job.id).map(String::as_str),
            query,
            now,
        ) {
            Ok(listing) => own.listings.push(listing),
            Err(listings::Drop::Older) => own.excluded.older += 1,
            Err(listings::Drop::BelowSalary) => own.excluded.below_salary += 1,
            Err(_) => own.excluded.not_postings += 1,
        }
    }
    Ok(own)
}

/// The model's own web search for a request, then ReMa's page checks.
#[allow(clippy::too_many_arguments)]
async fn web_jobs(
    state: &AppState,
    endpoint: &Endpoint,
    model_id: &str,
    query: &JobQuery,
    plan: &SearchPlan,
    hints: &plan::Hints,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> Result<WebJobs, String> {
    let found = native::search(state, endpoint, model_id, query, hints, progress, cancel).await?;
    let related: Vec<String> = plan.roles.iter().skip(1).cloned().collect();
    let checked = listings::check(state, &found, query, &related, progress, cancel).await;
    Ok(WebJobs {
        engine: found.engine,
        searches: found.searches,
        pages_read: checked.pages_read,
        listings: checked.listings,
        excluded: checked.excluded,
    })
}

/// "company|role|city" for listings that clearly describe one vacancy.
fn vacancy_key(l: &Listing) -> Option<String> {
    let company = normalize::company_key(l.company.as_deref()?);
    let title = normalize::clean_title(&l.title).to_lowercase();
    let city = l
        .location
        .as_deref()
        .and_then(|loc| normalize::place(loc).city())
        .unwrap_or_default();
    (!company.is_empty() && !title.is_empty()).then(|| format!("{company}|{title}|{city}"))
}

fn fill(into: &mut Listing, from: &Listing) {
    let take = |a: &mut Option<String>, b: &Option<String>| {
        if a.is_none() {
            a.clone_from(b);
        }
    };
    take(&mut into.company, &from.company);
    take(&mut into.location, &from.location);
    take(&mut into.summary, &from.summary);
    take(&mut into.work_mode, &from.work_mode);
    if into.salary.is_none() && from.salary.is_some() {
        into.salary.clone_from(&from.salary);
        into.salary_status = from.salary_status;
        // Salary caveats go with the salary they describe.
        into.notes.retain(|n| !n.starts_with("salary"));
        for caveat in from.notes.iter().filter(|n| n.starts_with("salary")) {
            into.notes.push(caveat.clone());
        }
    }
    if into.posted.is_none() {
        into.posted = from.posted;
    }
    let note = if (from.verification as u8) < (into.verification as u8) {
        // The duplicate was checked: its check, and its caveats, stand for
        // this listing; its own page may not have opened.
        into.verification = from.verification;
        into.notes
            .retain(|n| !n.starts_with("ReMa could not open the page"));
        for caveat in &from.notes {
            if !caveat.starts_with("salary") && !into.notes.contains(caveat) {
                into.notes.push(caveat.clone());
            }
        }
        format!("checked on {}", from.source)
    } else {
        format!("also listed on {}", from.source)
    };
    if from.source != into.source && !into.notes.contains(&note) {
        into.notes.push(note);
    }
}

/// One list: duplicates merged (same address, or same employer, role and
/// city), the employer's own posting kept (§33), employer-less postings
/// left out (§56), newest first.
pub fn merge(mut all: Vec<Listing>, excluded: &mut Excluded) -> Vec<Listing> {
    all.sort_by_key(|l| (authority(&l.url), l.verification as u8));
    let mut out: Vec<Listing> = Vec::new();
    for listing in all {
        let url_key = normalize::canonical_url(&listing.url);
        let vacancy = vacancy_key(&listing);
        let same = out.iter().position(|kept| {
            (url_key.is_some() && normalize::canonical_url(&kept.url) == url_key)
                || (vacancy.is_some() && vacancy_key(kept) == vacancy)
        });
        match same {
            Some(index) => fill(&mut out[index], &listing),
            None => out.push(listing),
        }
    }
    let before = out.len();
    out.retain(|l| l.company.as_deref().is_some_and(|c| !c.trim().is_empty()));
    excluded.incomplete += before - out.len();
    out.sort_by(|a, b| {
        b.posted
            .unwrap_or(i64::MIN)
            .cmp(&a.posted.unwrap_or(i64::MIN))
            .then((a.verification as u8).cmp(&(b.verification as u8)))
    });
    out.truncate(MAX_LISTINGS);
    out
}

/// A job search through every route (§19, §52–§55).
pub async fn search_jobs(
    state: &AppState,
    endpoint: &Endpoint,
    model_id: &str,
    query: &JobQuery,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> Outcome {
    search_jobs_with(state, Some((endpoint, model_id)), query, progress, cancel).await
}

/// A job search; without a model, ReMa's own job sources alone (Network
/// Connect when no model is set up).
pub async fn search_jobs_with(
    state: &AppState,
    model: Option<(&Endpoint, &str)>,
    query: &JobQuery,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> Outcome {
    let started = Instant::now();
    let plan = plan::for_job_query(query);
    progress.status("Searching jobs…");
    let company_domains = company_domains(state, &plan, cancel).await;
    let hints = plan::hints(&plan, &company_domains);
    let own = own_jobs(state, &plan, query, cancel);
    let web = async {
        match model {
            Some((endpoint, model_id)) if native::supported(endpoint) => Some((
                native::engine_name(endpoint).to_string(),
                web_jobs(
                    state, endpoint, model_id, query, &plan, &hints, progress, cancel,
                )
                .await,
            )),
            _ => None,
        }
    };
    let (own, web) = tokio::join!(own, web);
    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }
    let mut report = RouteReport {
        at: now_ms(),
        requirement: plan.requirement,
        scopes: plan.scopes.names().iter().map(|s| s.to_string()).collect(),
        route: String::new(),
        fallbacks: Vec::new(),
        queried: Vec::new(),
        succeeded: Vec::new(),
        failed: Vec::new(),
        duration_ms: 0,
        results: 0,
    };
    let mut engines: Vec<String> = Vec::new();
    let mut sources_used: Vec<String> = Vec::new();
    let mut fallbacks: Vec<String> = Vec::new();
    let mut listings: Vec<Listing> = Vec::new();
    let mut excluded = Excluded::default();
    let mut searches = 0;
    let mut pages_read = 0;
    let mut reasons: Vec<String> = Vec::new();
    // The time of the oldest results shown.
    let mut retrieved_at = now_ms();
    let mut unreached: Vec<String> = Vec::new();
    report.queried.push(REMA_JOBS.into());
    match own {
        Ok(own) => {
            engines.push(REMA_JOBS.into());
            report.succeeded.push(REMA_JOBS.into());
            sources_used.push(REMA_JOBS.into());
            for name in source_names(&own.searched) {
                if !sources_used.contains(&name) {
                    sources_used.push(name);
                }
            }
            report.failed.extend(own.failed.iter().cloned());
            unreached.extend(own.unreached);
            retrieved_at = retrieved_at.min(own.searched_at);
            searches += own.lookups;
            excluded.add(&own.excluded);
            listings.extend(own.listings);
        }
        Err(reason) => {
            // The engine names its sources already ("ReMa job sources: …").
            let reason = if reason.starts_with(engine::OWN_SOURCES) {
                reason
            } else {
                format!("{REMA_JOBS}: {reason}")
            };
            report.failed.push(reason.clone());
            reasons.push(reason);
        }
    }
    if let Some((name, web)) = web {
        report.queried.push(name.clone());
        match web {
            Ok(web) => {
                report.succeeded.push(name.clone());
                engines.push(web.engine.clone());
                sources_used.push(web.engine.clone());
                searches += web.searches;
                pages_read += web.pages_read;
                excluded.add(&web.excluded);
                listings.extend(web.listings);
            }
            Err(reason) => {
                report.failed.push(reason.clone());
                reasons.push(reason);
            }
        }
    }
    report.duration_ms = research::millis_since(started) as u32;
    if engines.is_empty() {
        report.route = "none".into();
        report.fallbacks = reasons.clone();
        state.career.record(report);
        let mut all = vec![UNAVAILABLE.to_string()];
        all.extend(reasons);
        return Outcome::Failed { reasons: all };
    }
    // Routes that failed while another answered: said once, not an error.
    fallbacks.extend(reasons);
    let listings = merge(listings, &mut excluded);
    report.route = engines.join(" + ");
    report.fallbacks = fallbacks.clone();
    report.results = listings.len() as u32;
    state.career.record(report);
    let retrieval = Retrieval {
        query: query.clone(),
        engine: engines.join(" + "),
        searches,
        pages_read,
        listings,
        excluded,
        retrieved_at,
        fallbacks,
        sources: sources_used,
        unreached,
    };
    if retrieval.listings.is_empty() {
        Outcome::Empty(retrieval)
    } else {
        Outcome::Found(retrieval)
    }
}

/// The official domains of the companies a request names (Wikidata,
/// remembered), so the provider's search includes their own sites (§24).
async fn company_domains(
    state: &AppState,
    plan: &SearchPlan,
    cancel: &CancellationToken,
) -> Vec<String> {
    if plan.companies.is_empty() {
        return Vec::new();
    }
    let deadline = Instant::now() + std::time::Duration::from_secs(8);
    let ctx = state
        .rema_mcp
        .ctx(&state.info.version, cancel, deadline, false);
    let mut out = Vec::new();
    for name in plan.companies.iter().take(3) {
        if let Ok(Some(company)) = state.career.company(&ctx, name).await {
            if let Some(domain) = company.domain {
                if !out.contains(&domain) {
                    out.push(domain);
                }
            }
        }
    }
    out
}

/// Research through every route: ReMa's own sources and the model's own
/// web search, merged into one evidence list (§29–§32).
pub async fn research(
    state: &AppState,
    model: Option<(&Endpoint, &str)>,
    plan: &SearchPlan,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> ResearchOutcome {
    let started = Instant::now();
    progress.status(if plan.scopes.people {
        "Searching professional sources…"
    } else if plan.scopes.market {
        "Searching current postings and market sources…"
    } else {
        "Searching company sources…"
    });
    let company_domains = company_domains(state, plan, cancel).await;
    let hints = plan::hints(plan, &company_domains);
    let deadline = Instant::now() + std::time::Duration::from_secs(30);
    let own = async {
        let ctx = state
            .rema_mcp
            .ctx(&state.info.version, cancel, deadline, false);
        research::own(state, &ctx, plan, &state.career.health).await
    };
    let web = async {
        match model {
            Some((endpoint, model_id)) if native::supported(endpoint) => Some((
                native::engine_name(endpoint).to_string(),
                native::research(
                    state,
                    endpoint,
                    model_id,
                    plan,
                    &hints,
                    &company_domains,
                    progress,
                    cancel,
                )
                .await,
            )),
            _ => None,
        }
    };
    let (own, web) = tokio::join!(own, web);
    if cancel.is_cancelled() {
        return ResearchOutcome::Cancelled;
    }
    let mut findings = own.findings;
    let mut engines: Vec<String> = Vec::new();
    let mut sources_used = own.sources.clone();
    let mut reasons: Vec<String> = own.failed.clone();
    let mut searches = own.lookups;
    let own_answered = !own.sources.is_empty();
    if own_answered {
        engines.push("ReMa sources".into());
    }
    let mut queried = vec!["ReMa sources".to_string()];
    let mut succeeded = if own_answered {
        vec!["ReMa sources".to_string()]
    } else {
        Vec::new()
    };
    if let Some((name, result)) = web {
        queried.push(name.clone());
        match result {
            Ok((found, count)) => {
                engines.push(name.clone());
                sources_used.push(name.clone());
                succeeded.push(name);
                searches += count;
                findings.extend(found);
            }
            Err(reason) => reasons.push(reason),
        }
    }
    let findings = research::merge(findings);
    state.career.record(RouteReport {
        at: now_ms(),
        requirement: plan.requirement,
        scopes: plan.scopes.names().iter().map(|s| s.to_string()).collect(),
        route: if engines.is_empty() {
            "none".into()
        } else {
            engines.join(" + ")
        },
        fallbacks: reasons.clone(),
        queried,
        succeeded,
        failed: reasons.clone(),
        duration_ms: research::millis_since(started) as u32,
        results: findings.len() as u32,
    });
    if engines.is_empty() {
        let mut all = vec![UNAVAILABLE.to_string()];
        all.extend(reasons);
        return ResearchOutcome::Failed { reasons: all };
    }
    let research = Research {
        plan: plan.clone(),
        findings,
        notes: own.notes,
        engine: engines.join(" + "),
        searches,
        retrieved_at: now_ms(),
        fallbacks: reasons,
        sources: sources_used,
    };
    if research.findings.is_empty() {
        ResearchOutcome::Empty(research)
    } else {
        ResearchOutcome::Found(research)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::SalaryStatus;

    fn listing(
        url: &str,
        company: Option<&str>,
        source: &str,
        verification: Verification,
    ) -> Listing {
        Listing {
            title: "Senior AI Engineer".into(),
            company: company.map(str::to_string),
            location: Some("Vienna, Austria".into()),
            url: url.into(),
            summary: None,
            posted: Some(1_790_121_600_000),
            salary: None,
            salary_status: SalaryStatus::NotListed,
            work_mode: None,
            verification,
            source: source.into(),
            notes: vec![],
        }
    }

    #[test]
    fn the_employers_posting_wins_and_duplicates_merge() {
        let mut board = listing(
            "https://www.arbeitnow.com/jobs/companies/nordlicht/senior-ai-engineer-1",
            Some("Nordlicht AI"),
            "Arbeitnow",
            Verification::Posting,
        );
        board.salary = Some("€90,000–€110,000".into());
        board.salary_status = SalaryStatus::Verified;
        let official = listing(
            "https://job-boards.greenhouse.io/nordlicht/jobs/4411001",
            Some("Nordlicht AI GmbH"),
            "Greenhouse",
            Verification::Posting,
        );
        let same_url_again = listing(
            "https://job-boards.greenhouse.io/nordlicht/jobs/4411001?utm_source=x",
            Some("Nordlicht AI"),
            "Greenhouse",
            Verification::SearchOnly,
        );
        let anonymous = listing(
            "https://example.com/jobs/ai-1",
            None,
            "example.com",
            Verification::Page,
        );
        let mut excluded = Excluded::default();
        let merged = merge(
            vec![board, same_url_again, anonymous, official],
            &mut excluded,
        );
        assert_eq!(merged.len(), 1, "{merged:#?}");
        let kept = &merged[0];
        assert!(
            kept.url.contains("greenhouse.io"),
            "the employer's own posting is kept"
        );
        assert_eq!(
            kept.salary.as_deref(),
            Some("€90,000–€110,000"),
            "filled from the board"
        );
        assert_eq!(kept.salary_status, SalaryStatus::Verified);
        assert!(kept.notes.iter().any(|n| n == "also listed on Arbeitnow"));
        assert_eq!(
            excluded.incomplete, 1,
            "a posting without an employer is not shown"
        );
    }

    #[test]
    fn a_checked_duplicate_vouches_for_a_page_that_did_not_open() {
        // The employer's page was found by search but did not open; the
        // same vacancy was read from a hiring thread.
        let mut official = listing(
            "https://job-boards.greenhouse.io/wienrobotics/jobs/5550001",
            Some("Wien Robotics"),
            "Greenhouse",
            Verification::SearchOnly,
        );
        official.notes = vec!["ReMa could not open the page: the site did not answer".into()];
        let mut thread = listing(
            "https://news.ycombinator.com/item?id=45100123",
            Some("Wien Robotics"),
            "Hacker News",
            Verification::Posting,
        );
        official.salary = Some("€95k–120k / year".into());
        official.salary_status = SalaryStatus::Verified;
        thread.salary = Some("€95k–120k".into());
        thread.notes = vec![
            "whether it is still open is not stated".into(),
            "salary period not stated".into(),
        ];
        let merged = merge(vec![thread, official], &mut Excluded::default());
        assert_eq!(merged.len(), 1, "{merged:#?}");
        let kept = &merged[0];
        assert!(kept.url.contains("greenhouse.io"));
        assert_eq!(kept.verification, Verification::Posting);
        assert_eq!(
            kept.notes,
            [
                "whether it is still open is not stated",
                "checked on Hacker News"
            ]
        );
    }

    #[test]
    fn summaries_keep_the_salary_rules() {
        let query = retrieval::detect(
            "Find me most recent AI jobs in Vienna Austria with salary starting from 85k a year.",
        )
        .unwrap();
        let summary = |salary: Option<(f64, f64)>| JobSummary {
            id: "rj_1".into(),
            title: "AI Engineer".into(),
            employer: Some("Donau Data".into()),
            locations: vec!["Vienna, Austria".into()],
            work_mode: Some(WorkMode::Hybrid),
            remote_eligibility: vec![],
            seniority: None,
            employment_type: None,
            working_time: None,
            salary: salary.map(|(min, max)| crate::rema_mcp::contract::SalarySummary {
                text: format!("EUR {min}–{max} per year"),
                min: Some(min),
                max: Some(max),
                currency: Some("EUR".into()),
                period: Some(crate::rema_mcp::contract::SalaryPeriod::Year),
                bound: None,
                estimate: false,
            }),
            posted_at: Some("2026-09-23".into()),
            last_checked_at: None,
            availability: Availability::Active,
            description_state: DescriptionState::Full,
            url: "https://jobs.lever.co/donau/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0".into(),
            source: "lever".into(),
            acquisition: AcquisitionMode::DocumentedPublicFeed,
            discovered_from: vec![],
            needs_verification: false,
            notes: vec![],
            relevance: crate::rema_mcp::contract::Relevance {
                score: 50,
                factors: vec![],
            },
        };
        let now = 1_790_380_800_000;
        let fits = listing_of(
            &summary(Some((85_000.0, 100_000.0))),
            Some("Build models."),
            &query,
            now,
        )
        .ok()
        .unwrap();
        assert_eq!(fits.salary_status, SalaryStatus::Verified);
        assert_eq!(fits.verification, Verification::Posting);
        assert_eq!(fits.source, "Lever");
        let unknown = listing_of(&summary(None), None, &query, now).ok().unwrap();
        assert_eq!(unknown.salary_status, SalaryStatus::NotListed);
        assert!(
            unknown.notes.iter().any(|n| n == "salary not stated"),
            "shown, and marked"
        );
        assert!(matches!(
            listing_of(&summary(Some((60_000.0, 70_000.0))), None, &query, now),
            Err(listings::Drop::BelowSalary)
        ));
    }
}
