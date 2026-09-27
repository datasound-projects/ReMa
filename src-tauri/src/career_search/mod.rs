//! ReMa Search: the always-on career search layer (jobs, companies,
//! people, the market). Current career information never depends on a
//! search service the user set up.
//!
//! One router ([`router`]) serves Chat, Scheduled Tasks, agents, the Jobs
//! MCP and Network Connect. For each request it:
//!
//! 1. classifies it ([`requirement`]): none, optional or **required**
//!    current information, and its scopes (jobs, company, people, market);
//! 2. plans it ([`plan`]): role and close variants, place, remote work,
//!    salary floor, freshness, named companies, people sought;
//! 3. asks the Jobs MCP first for jobs, fed by ReMa's own key-free sources
//!    ([`jobs`], [`company`]: employer boards, public job boards, company
//!    sites, Wikidata), and — at the same time — the selected model's own
//!    web search (OpenAI, Anthropic, Gemini, ChatGPT through Codex, or
//!    Unsloth Studio's local search), restricted to the career sites of the
//!    source registry ([`registry`]) and localized to the request's place;
//!    a provider search only counts when searches actually ran;
//! 4. verifies, deduplicates and merges what came back, keeping the
//!    evidence (address, title, site, retrieval time) of every result; and
//! 5. when every route failed, says so ([`UNAVAILABLE`]) — never an answer
//!    from a model's memory, never a request to configure a search API.
//!
//! Sources that fail are skipped (§34) and rested when they keep failing
//! ([`health`]). Search results and pages are evidence, never
//! instructions: they reach models only as delimited data, and nothing in
//! them can call tools, read credentials or change settings.

pub mod bootstrap;
pub mod capabilities;
pub mod citations;
pub mod company;
pub mod discovery;
pub mod evidence;
pub mod extract;
pub mod health;
pub mod jobs;
pub mod plan;
pub mod registry;
pub mod requirement;
pub mod research;
pub mod router;
pub mod status;
#[cfg(test)]
pub(crate) mod tests;

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::Serialize;
use specta::Type;

pub use requirement::{classify, Classified, Requirement, Scopes};

use crate::{
    llm::Endpoint,
    rema_mcp::{adapters::Ctx, contract::ToolError},
    state::AppState,
};
use health::HealthBook;

/// The answer when every search route failed (§21): operational, honest,
/// and never a request to set anything up.
pub const UNAVAILABLE: &str =
    "ReMa could not retrieve live career sources for this request right now.";

/// How long a local server's capabilities are remembered.
const RUNTIME_TTL: Duration = Duration::from_secs(10 * 60);
/// A server that did not answer is asked again after this.
const RUNTIME_RETRY: Duration = Duration::from_secs(60);
/// Route reports kept for diagnostics.
const MAX_REPORTS: usize = 50;

/// What one search did (§49): no credentials, no page text.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RouteReport {
    pub at: i64,
    pub requirement: Requirement,
    pub scopes: Vec<String>,
    /// The routes that answered ("ReMa Jobs + OpenAI web search").
    pub route: String,
    /// Routes that failed, with their reason.
    pub fallbacks: Vec<String>,
    /// Sources asked, answered and failed (names).
    pub queried: Vec<String>,
    pub succeeded: Vec<String>,
    pub failed: Vec<String>,
    pub duration_ms: u32,
    pub results: u32,
}

/// The provider's own words from a refusal ("Anthropic web search:
/// Anthropic returned an error (400): web search is not enabled for this
/// organization" → "web search is not enabled for this organization").
fn refusal_detail(reason: &str) -> String {
    reason
        .rsplit(": ")
        .next()
        .unwrap_or(reason)
        .trim()
        .trim_end_matches('.')
        .to_string()
}

/// Search state shared by every feature: source health, what local
/// servers can do, resolved companies, and recent route reports.
#[derive(Default)]
pub struct CareerSearch {
    pub health: HealthBook,
    runtimes: Mutex<HashMap<String, (Instant, Option<bool>)>>,
    companies: Mutex<HashMap<String, (Instant, Option<company::Company>)>>,
    boards: Mutex<HashMap<String, (Instant, Vec<company::Board>)>>,
    reports: Mutex<VecDeque<RouteReport>>,
    /// Providers that refused their own web search (an organization or
    /// workspace turned it off), by provider key, with the reason.
    refusals: Mutex<HashMap<String, (Instant, String)>>,
    /// Where the no-key discovery provider is reached (a local stand-in in
    /// tests; its public address otherwise).
    discovery_base: std::sync::OnceLock<String>,
}

impl CareerSearch {
    /// Whether an OpenAI-compatible server runs its own web search
    /// (Unsloth Studio), asked once and remembered for a while.
    pub async fn server_web_search(&self, state: &AppState, endpoint: &Endpoint) -> bool {
        let key = endpoint.base_url.trim_end_matches('/').to_lowercase();
        if let Some((at, known)) = self.runtimes.lock().unwrap().get(&key).cloned() {
            let fresh = match known {
                Some(_) => at.elapsed() < RUNTIME_TTL,
                None => at.elapsed() < RUNTIME_RETRY,
            };
            if fresh {
                return known.unwrap_or(false);
            }
        }
        let serves = state.llm.serves_web_search(endpoint).await;
        self.runtimes
            .lock()
            .unwrap()
            .insert(key, (Instant::now(), Some(serves)));
        serves
    }

    /// Forgets what local servers can do (a provider was added or changed).
    pub fn forget_runtimes(&self) {
        self.runtimes.lock().unwrap().clear();
    }

    /// A company by name (Wikidata), remembered for a day.
    pub async fn company(
        &self,
        ctx: &Ctx<'_>,
        name: &str,
    ) -> Result<Option<company::Company>, ToolError> {
        let key = crate::analytics::normalize::company_key(name);
        if let Some((at, company)) = self.companies.lock().unwrap().get(&key).cloned() {
            if at.elapsed() < company::COMPANY_TTL {
                return Ok(company);
            }
        }
        let company = company::wikidata(ctx, name).await?;
        self.companies
            .lock()
            .unwrap()
            .insert(key, (Instant::now(), company.clone()));
        Ok(company)
    }

    pub fn cached_boards(&self, key: &str) -> Option<Vec<company::Board>> {
        self.boards
            .lock()
            .unwrap()
            .get(key)
            .filter(|(at, _)| at.elapsed() < company::COMPANY_TTL)
            .map(|(_, boards)| boards.clone())
    }

    pub fn remember_boards(&self, key: &str, boards: &[company::Board]) {
        self.boards
            .lock()
            .unwrap()
            .insert(key.to_string(), (Instant::now(), boards.to_vec()));
    }

    /// What a provider's own search just did: a refusal is remembered for
    /// a while (the next requests use ReMa's search straight away); a
    /// search that ran clears it.
    pub fn note_native(&self, key: &str, outcome: Result<(), &str>) {
        let mut refusals = self.refusals.lock().unwrap();
        match outcome {
            Ok(()) => {
                refusals.remove(key);
            }
            Err(reason) if capabilities::is_refusal(reason) => {
                refusals.insert(key.to_string(), (Instant::now(), refusal_detail(reason)));
            }
            Err(_) => {}
        }
    }

    /// Why a provider refused its own search recently, if it did.
    pub fn refusal(&self, key: &str) -> Option<String> {
        self.refusals
            .lock()
            .unwrap()
            .get(key)
            .filter(|(at, _)| at.elapsed() < capabilities::REFUSAL_TTL)
            .map(|(_, reason)| reason.clone())
    }

    /// ReMa's no-key discovery provider.
    pub fn duckduckgo(&self) -> discovery::DuckDuckGo {
        match self.discovery_base.get() {
            Some(base) => discovery::DuckDuckGo::with_base(base.clone()),
            None => discovery::DuckDuckGo::default(),
        }
    }

    /// Points discovery at a local stand-in (tests).
    #[cfg(test)]
    pub fn use_discovery_base(&self, base: &str) {
        let _ = self.discovery_base.set(base.to_string());
    }

    /// Keeps a route report for diagnostics (newest last, bounded).
    pub fn record(&self, report: RouteReport) {
        let mut reports = self.reports.lock().unwrap();
        if reports.len() == MAX_REPORTS {
            reports.pop_front();
        }
        reports.push_back(report);
    }

    /// Recent route reports, newest first.
    pub fn reports(&self) -> Vec<RouteReport> {
        self.reports.lock().unwrap().iter().rev().cloned().collect()
    }
}
