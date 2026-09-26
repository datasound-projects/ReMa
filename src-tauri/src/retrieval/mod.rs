//! Live job search: ReMa searches the web before it answers a request for
//! current job listings.
//!
//! The workflow is enforced here, in application code, not left to a
//! model's choice or a prompt:
//!
//! 1. [`intent::detect`] recognizes a job search (or a request to verify
//!    linked postings) from the user's message.
//! 2. A search runs: the selected model's own hosted web search
//!    ([`native`]; ChatGPT through Codex, OpenAI, Anthropic, Gemini), and
//!    when that is unavailable or fails, the search service configured in
//!    Settings ([`backend`]; Brave Search, Tavily or a SearXNG instance).
//!    A step only counts when the provider reports searches that actually
//!    ran.
//! 3. [`listings`] reads the postings' own pages (public addresses only,
//!    like a browser opening them once), keeps what the pages state and
//!    applies the request's hard filters: posting date, salary floor,
//!    location, role.
//! 4. Only then is an answer written: ReMa shows the listings it validated
//!    ([`render`]) and the model adds its assessment of those listings,
//!    with no web access of its own.
//!
//! Results are [`Outcome::Found`], [`Outcome::Empty`] (searches ran, nothing
//! matched) or [`Outcome::Failed`] (no search could run) — never an answer
//! with listings that were not retrieved.

pub mod backend;
pub mod intent;
pub mod listings;
pub mod native;
pub mod render;
pub mod tools;

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

pub use intent::{detect, JobQuery};

use crate::{
    llm::{Endpoint, WebObserver},
    state::AppState,
    time::now_ms,
};

/// A posting a search reported, before ReMa read its page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Candidate {
    pub url: String,
    pub title: Option<String>,
    pub company: Option<String>,
    pub location: Option<String>,
    /// As the search or the model stated it ("3 days ago", "2026-09-20").
    pub posted: Option<String>,
    pub salary: Option<String>,
    pub snippet: Option<String>,
    /// The search engine itself reported this address (a result, a page it
    /// opened or cited), not only the model's summary.
    pub reported: bool,
    /// The model listed it as a posting.
    pub listed: bool,
}

/// What one search step found.
#[derive(Debug, Clone, Default)]
pub struct Found {
    /// "ChatGPT web search", "Brave Search".
    pub engine: String,
    /// Searches that actually ran.
    pub searches: usize,
    pub candidates: Vec<Candidate>,
    /// The engine reports the addresses it found (Codex does not: only the
    /// pages its model opened), so a posting only the model named can be
    /// told apart.
    pub lists_sources: bool,
}

/// How much of a listing ReMa checked itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// ReMa read the page and it holds a structured job posting.
    Posting,
    /// ReMa read the page; it has no structured posting data.
    Page,
    /// ReMa could not read the page; the details come from the search and
    /// are not checked.
    SearchOnly,
}

/// A validated listing.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub title: String,
    pub company: Option<String>,
    pub location: Option<String>,
    /// The posting's own address (after redirects).
    pub url: String,
    pub summary: Option<String>,
    /// UTC midnight of the posting date.
    pub posted: Option<i64>,
    pub salary: Option<String>,
    pub work_mode: Option<String>,
    pub verification: Verification,
    /// What is missing or uncertain ("salary not stated").
    pub notes: Vec<String>,
}

/// Postings found but not shown, by reason.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Excluded {
    pub older: usize,
    pub expired: usize,
    pub gone: usize,
    pub below_salary: usize,
    pub elsewhere: usize,
    pub off_topic: usize,
    pub not_postings: usize,
}

impl Excluded {
    pub fn total(&self) -> usize {
        self.older
            + self.expired
            + self.gone
            + self.below_salary
            + self.elsewhere
            + self.off_topic
            + self.not_postings
    }
}

/// A completed retrieval.
#[derive(Debug, Clone)]
pub struct Retrieval {
    pub query: JobQuery,
    pub engine: String,
    pub searches: usize,
    pub pages_read: usize,
    pub listings: Vec<Listing>,
    pub excluded: Excluded,
    pub retrieved_at: i64,
    /// Search steps that failed before one worked ("ChatGPT web search: …").
    pub fallbacks: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    /// Searches ran and listings passed validation.
    Found(Retrieval),
    /// Searches ran, but no posting matched.
    Empty(Retrieval),
    /// No search could run; nothing may be presented as a current listing.
    Failed {
        reasons: Vec<String>,
    },
    Cancelled,
}

/// Where a retrieval reports what it is doing.
pub trait Progress: Send + Sync {
    /// A short status line ("Searching the web…").
    fn status(&self, text: &str);
    /// Receives the provider's searches as they happen.
    fn web(&self) -> Option<Arc<dyn WebObserver>> {
        None
    }
    /// A posting page was read (or could not be).
    fn page(&self, _url: &str, _result: Result<(), String>) {}
}

/// Reports nothing (scheduled tasks).
pub struct Silent;

impl Progress for Silent {
    fn status(&self, _text: &str) {}
}

/// Runs the whole retrieval for a job-search request.
pub async fn run(
    state: &AppState,
    endpoint: &Endpoint,
    model_id: &str,
    query: &JobQuery,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> Outcome {
    let mut reasons = Vec::new();
    progress.status("Searching the web…");
    let mut found = None;
    if native::supported(endpoint) {
        match native::search(state, endpoint, model_id, query, progress, cancel).await {
            Ok(result) => found = Some(result),
            Err(reason) => reasons.push(reason),
        }
    }
    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }
    if found.is_none() {
        match backend::configured(state).await {
            Ok(Some(service)) => {
                if !reasons.is_empty() {
                    progress.status(&format!("Searching with {}…", service.name()));
                }
                match service.search_jobs(query, progress, cancel).await {
                    Ok(result) => found = Some(result),
                    Err(reason) => reasons.push(format!("{}: {reason}", service.name())),
                }
            }
            Ok(None) if native::supported(endpoint) => reasons
                .push("No other search service is set up (Settings → Web search).".to_string()),
            Ok(None) => reasons.push(
                "This model has no web search of its own, and no search service is set up. Add \
                 one in Settings → Web search, or choose a model with web search."
                    .to_string(),
            ),
            Err(error) => reasons.push(error.to_string()),
        }
    }
    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }
    let Some(found) = found else {
        return Outcome::Failed { reasons };
    };
    let checked = listings::check(state, &found, query, progress, cancel).await;
    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }
    let retrieval = Retrieval {
        query: query.clone(),
        engine: found.engine,
        searches: found.searches,
        pages_read: checked.pages_read,
        listings: checked.listings,
        excluded: checked.excluded,
        retrieved_at: now_ms(),
        fallbacks: reasons,
    };
    if retrieval.listings.is_empty() {
        Outcome::Empty(retrieval)
    } else {
        Outcome::Found(retrieval)
    }
}
