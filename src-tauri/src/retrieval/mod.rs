//! Live job search: ReMa searches the web before it answers a request for
//! current job listings.
//!
//! The workflow is enforced here, in application code, not left to a
//! model's choice or a prompt:
//!
//! 1. [`intent::detect`] recognizes a job search (or a request to verify
//!    linked postings) from the user's message.
//! 2. The career search router (`career_search::router`) searches: ReMa's
//!    own job sources through the Jobs MCP (no key needed), and at the same
//!    time the selected model's own web search ([`native`]; ChatGPT through
//!    Codex, OpenAI, Anthropic, Gemini, Unsloth Studio). A web step only
//!    counts when the provider reports searches that actually ran. A search
//!    service set up in Settings ([`backend`]) is optional and only adds
//!    sources.
//! 3. [`listings`] reads the postings' own pages (public addresses only,
//!    like a browser opening them once), keeps what the pages state and
//!    applies the request's hard filters: posting date, salary floor,
//!    location, role.
//! 4. Only then is an answer written: ReMa shows the listings it validated
//!    ([`render`]) and the model adds its assessment of those listings,
//!    with no web access of its own.
//!
//! Results are [`Outcome::Found`], [`Outcome::Empty`] (searches ran, nothing
//! matched) or [`Outcome::Failed`] (every route failed) — never an answer
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

/// Whether a listing's salary is known (§27). Unknown never counts as
/// meeting a requested minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SalaryStatus {
    /// The posting states it.
    Verified,
    /// The source labels it an estimate (never used for a minimum).
    Estimated,
    NotListed,
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
    pub salary_status: SalaryStatus,
    pub work_mode: Option<String>,
    pub verification: Verification,
    /// Where it was found ("Greenhouse", "Arbeitnow", "karriere.at").
    pub source: String,
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
    /// Postings that do not name the employer (§56).
    pub incomplete: usize,
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
            + self.incomplete
    }

    pub fn add(&mut self, other: &Excluded) {
        self.older += other.older;
        self.expired += other.expired;
        self.gone += other.gone;
        self.below_salary += other.below_salary;
        self.elsewhere += other.elsewhere;
        self.off_topic += other.off_topic;
        self.not_postings += other.not_postings;
        self.incomplete += other.incomplete;
    }
}

/// A completed retrieval.
#[derive(Debug, Clone)]
pub struct Retrieval {
    pub query: JobQuery,
    /// The routes that answered ("ReMa Jobs + OpenAI web search").
    pub engine: String,
    pub searches: usize,
    pub pages_read: usize,
    pub listings: Vec<Listing>,
    pub excluded: Excluded,
    pub retrieved_at: i64,
    /// Search steps that failed while others worked ("ChatGPT web search: …").
    pub fallbacks: Vec<String>,
    /// Sources consulted, by name ("ReMa Jobs", "Company career sites",
    /// "OpenAI web search").
    pub sources: Vec<String>,
    /// Sources that could not be searched this time, by name: their
    /// postings may be missing.
    pub unreached: Vec<String>,
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

/// Runs the whole retrieval for a job-search request, through the career
/// search router: ReMa's job sources (the Jobs MCP) and the model's own web
/// search together, never a search service the user has to set up.
pub async fn run(
    state: &AppState,
    endpoint: &Endpoint,
    model_id: &str,
    query: &JobQuery,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> Outcome {
    crate::career_search::router::search_jobs(state, endpoint, model_id, query, progress, cancel)
        .await
}
