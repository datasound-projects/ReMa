//! Source adapters: each reads one vacancy from a permitted source and
//! returns a [`JobRecord`] with evidence. The cheapest reliable path per
//! source: documented ATS APIs and feeds first, then a permitted page's
//! `JobPosting` JSON-LD, then its main text. Remote scripts never run.

pub mod ashby;
pub mod greenhouse;
pub mod lever;
pub mod page;
pub mod personio;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio_util::sync::CancellationToken;

use super::{
    contract::{
        AcquisitionMode, Availability, Dates, Description, DescriptionState, Employer, ErrorCode,
        Evidence, JobRecord, Links, ListsStatus, Quality, SourceId, ToolError, SCHEMA_VERSION,
    },
    extract,
    fetch::{Accept, FetchError, Fetcher, Validators, FEED_LIMIT},
    sources::{Classified, Target},
};

const FEED_TTL: Duration = Duration::from_secs(600);

/// Where the documented APIs live (overridable for tests and local E2E).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Apis {
    pub greenhouse: String,
    pub lever: String,
    pub lever_eu: String,
    pub ashby: String,
    /// `None`: the employer's own `{company}.jobs.personio.de`.
    pub personio: Option<String>,
}

impl Apis {
    pub fn official() -> Self {
        Self {
            greenhouse: "https://boards-api.greenhouse.io".into(),
            lever: "https://api.lever.co".into(),
            lever_eu: "https://api.eu.lever.co".into(),
            ashby: "https://api.ashbyhq.com".into(),
            personio: None,
        }
    }

    /// Every API on one local test server (paths keep them apart).
    pub fn local(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            greenhouse: format!("{base}/greenhouse"),
            lever: format!("{base}/lever"),
            lever_eu: format!("{base}/lever-eu"),
            ashby: format!("{base}/ashby"),
            personio: Some(format!("{base}/personio")),
        }
    }

    pub fn personio_feed(&self, company: &str, domain: &str) -> String {
        match &self.personio {
            Some(base) => format!("{base}/{company}/xml"),
            None => format!("https://{company}.{domain}/xml"),
        }
    }
}

type FeedEntries = HashMap<String, (Instant, Validators, Arc<String>)>;

/// Recently read ATS feeds (Ashby boards, Personio XML), kept in memory
/// for 10 minutes and revalidated with ETag/Last-Modified. Concurrent reads
/// of one feed share a single request.
#[derive(Clone, Default)]
pub struct FeedCache {
    entries: Arc<Mutex<FeedEntries>>,
    locks: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl FeedCache {
    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }
}

/// Everything an adapter needs for one request.
pub struct Ctx<'a> {
    pub fetcher: &'a Fetcher,
    pub apis: &'a Apis,
    pub feeds: &'a FeedCache,
    pub deadline: Instant,
    pub cancel: &'a CancellationToken,
    pub now: i64,
    /// Read again even when a fresh copy is cached.
    pub refresh: bool,
}

impl Ctx<'_> {
    pub async fn json(&self, url: &str) -> Result<serde_json::Value, ToolError> {
        let response = self
            .fetcher
            .get(
                url,
                Accept::Json,
                FEED_LIMIT,
                None,
                self.deadline,
                self.cancel,
            )
            .await
            .map_err(fetch_error)?;
        serde_json::from_str(response.body.as_deref().unwrap_or_default()).map_err(|_| {
            ToolError::new(ErrorCode::ParsingFailed, "the source sent unreadable JSON")
        })
    }

    /// A feed, from the cache while fresh (or revalidated).
    pub async fn feed(&self, url: &str, accept: Accept) -> Result<Arc<String>, ToolError> {
        let lock = self
            .feeds
            .locks
            .lock()
            .unwrap()
            .entry(url.to_string())
            .or_default()
            .clone();
        let _one = lock.lock().await;
        let cached = self.feeds.entries.lock().unwrap().get(url).cloned();
        if let Some((at, _, body)) = &cached {
            if !self.refresh && at.elapsed() < FEED_TTL {
                return Ok(body.clone());
            }
        }
        let validators = cached.as_ref().map(|(_, v, _)| v.clone());
        let response = self
            .fetcher
            .get(
                url,
                accept,
                FEED_LIMIT,
                validators.as_ref(),
                self.deadline,
                self.cancel,
            )
            .await
            .map_err(fetch_error)?;
        let body = match (response.body, cached) {
            (Some(body), _) => Arc::new(body),
            // 304 Not Modified: the cached copy is still current.
            (None, Some((_, _, body))) => body,
            (None, None) => {
                return Err(ToolError::new(
                    ErrorCode::ParsingFailed,
                    "the source sent no content",
                ))
            }
        };
        self.feeds.entries.lock().unwrap().insert(
            url.to_string(),
            (Instant::now(), response.validators, body.clone()),
        );
        Ok(body)
    }
}

pub fn fetch_error(error: FetchError) -> ToolError {
    let code = match &error {
        FetchError::Blocked(_) | FetchError::Refused(_) => ErrorCode::BlockedBySourcePolicy,
        FetchError::Gone(_) => ErrorCode::JobNotFound,
        FetchError::RateLimited => ErrorCode::RateLimited,
        FetchError::Timeout => ErrorCode::Timeout,
        FetchError::TooLarge => ErrorCode::PayloadLimitReached,
        FetchError::Cancelled => ErrorCode::Cancelled,
        FetchError::Failed(_) => ErrorCode::SourceUnavailable,
    };
    ToolError::new(code, error.message())
}

/// A record with nothing known yet (the engine assigns the id and
/// first-seen time).
pub fn blank(acquisition: AcquisitionMode) -> JobRecord {
    JobRecord {
        schema_version: SCHEMA_VERSION,
        id: String::new(),
        title: String::new(),
        normalized_title: None,
        language: None,
        employer: Employer {
            name: None,
            website: None,
        },
        source_ids: Vec::new(),
        locations: Vec::new(),
        work_mode: None,
        remote_eligibility: Vec::new(),
        seniority: None,
        employment_type: None,
        working_time: None,
        compensation: None,
        dates: Dates {
            posted_at: None,
            posted_precision: None,
            posted_reference: None,
            updated_at: None,
            first_seen_at: String::new(),
            retrieved_at: None,
            last_checked_at: None,
            valid_through: None,
        },
        description: Description {
            text: None,
            sections: Vec::new(),
            requirements: Vec::new(),
            preferred: Vec::new(),
            skills: Vec::new(),
            benefits: Vec::new(),
            state: DescriptionState::Missing,
            lists: ListsStatus::NoDescription,
            truncated: false,
        },
        links: Links {
            discovered: Vec::new(),
            canonical_url: None,
            employer_url: None,
            apply_url: None,
        },
        evidence: Vec::new(),
        quality: Quality {
            availability: Availability::Unknown,
            availability_basis: None,
            acquisition,
            missing: Vec::new(),
            conflicts: Vec::new(),
            inferred: Vec::new(),
            possible_duplicates: Vec::new(),
        },
    }
}

/// Evidence for a field.
pub fn evidence(
    record: &mut JobRecord,
    field: &str,
    source: &str,
    url: &str,
    method: &str,
    path: Option<&str>,
    now: i64,
) {
    record.evidence.push(Evidence {
        field: field.into(),
        source: source.into(),
        url: url.into(),
        method: method.into(),
        path: path.map(str::to_string),
        retrieved_at: extract::iso(now),
    });
}

pub fn source_id(source: &str, id: &str) -> SourceId {
    SourceId {
        source: source.into(),
        id: id.into(),
    }
}

/// Fills title-derived fields and the salary a description states.
pub fn finish_common(record: &mut JobRecord, source: &str, url: &str, now: i64) {
    record.normalized_title = extract::normalized_title(&record.title);
    if record.seniority.is_none() {
        record.seniority = extract::seniority(&record.title);
    }
    if record.work_mode.is_none() {
        let places: Vec<&str> = record.locations.iter().map(|l| l.text.as_str()).collect();
        record.work_mode = extract::work_mode(&format!("{} {}", record.title, places.join(" ")));
    }
    if record.language.is_none() {
        if let Some(text) = &record.description.text {
            if let Some(lang) = extract::language(text) {
                record.language = Some(lang.into());
                record.quality.inferred.push("language".into());
            }
        }
    }
    if record.compensation.is_none() {
        if let Some(found) = record
            .description
            .text
            .as_deref()
            .and_then(extract::salary_from_description)
        {
            record.compensation = Some(found);
            evidence(
                record,
                "compensation",
                source,
                url,
                "description_text",
                None,
                now,
            );
        }
    }
    record.dates.retrieved_at = Some(extract::iso(now));
}

/// Reads one vacancy from its source.
pub async fn read(
    ctx: &Ctx<'_>,
    target: &Classified,
    title_hint: &str,
) -> Result<JobRecord, ToolError> {
    match &target.target {
        Target::Greenhouse { board, job } => greenhouse::read(ctx, board, job).await,
        Target::Lever { site, id, eu } => lever::read(ctx, site, id, *eu).await,
        Target::Ashby { board, id } => ashby::read(ctx, board, id).await,
        Target::Personio {
            company,
            id,
            domain,
        } => personio::read(ctx, company, id, domain).await,
        Target::Page => page::read(ctx, &target.url, title_hint).await,
        Target::DiscoveryOnly => Err(ToolError::new(
            ErrorCode::BlockedBySourcePolicy,
            format!(
                "{} links are discovery-only: ReMa does not fetch them",
                target.source.name
            ),
        )),
    }
}
