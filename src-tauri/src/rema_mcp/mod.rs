//! ReMa MCP: ReMa's built-in, first-party job-search MCP server.
//!
//! Five read-only tools (`search_jobs`, `get_job`, `get_jobs`,
//! `search_similar_jobs`, `source_status`) served over MCP by an in-process
//! server ([`server`]) to ReMa's own MCP client ([`host`]). The job engine
//! ([`engine`]) discovers vacancies through the user's search backend,
//! reads them from permitted sources ([`sources`], [`adapters`]) through a
//! safe fetcher ([`fetch`]), normalizes, deduplicates, filters and ranks
//! them, and keeps a local cache ([`store`]).
//!
//! ReMa MCP is built in and enabled by default. While enabled its tools are
//! offered in every chat; it does nothing until a model calls a tool for
//! the user's request. Disabling it (Settings → MCP → Built-in, after two
//! confirmations) removes the tools, rejects new or queued calls and
//! cancels running work, without deleting saved jobs or history.
//!
//! See `docs/rema-mcp/implementation.md` for decisions and source rules.

pub mod adapters;
pub mod contract;
pub mod engine;
pub mod extract;
pub mod fetch;
pub mod filter;
pub mod host;
pub mod server;
pub mod sources;
pub mod stdio;
pub mod store;
#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

use tokio_util::sync::CancellationToken;

use crate::{db::providers as settings, error::AppResult, state::AppState};
use adapters::{Apis, FeedCache};
use fetch::Fetcher;

/// The settings key for the enabled switch ("true" / "false").
pub const ENABLED_KEY: &str = "rema_mcp.enabled";
/// The name shown in chats and Settings.
pub const NAME: &str = "ReMa MCP";

/// Last outcome per source, for `source_status`.
#[derive(Debug, Clone, Default)]
pub struct Health {
    pub last_success: Option<i64>,
    pub last_error: Option<(i64, String)>,
}

struct Inner {
    fetcher: OnceLock<Fetcher>,
    feeds: FeedCache,
    apis: Apis,
    local_fixtures: bool,
    next: AtomicU64,
    active: Mutex<HashMap<u64, CancellationToken>>,
    health: Mutex<HashMap<String, Health>>,
    /// Mirrors the saved setting for fast checks at every call.
    enabled: AtomicBool,
    enabled_loaded: AtomicBool,
}

/// ReMa MCP's runtime: network client, feed cache, running work. Cheap to
/// clone.
#[derive(Clone)]
pub struct RemaMcp {
    inner: Arc<Inner>,
}

impl Default for RemaMcp {
    fn default() -> Self {
        Self::new()
    }
}

impl RemaMcp {
    /// The documented APIs; debug builds accept a local test site
    /// (`REMA_DEV_ALLOW_LOCAL_PAGES`, `REMA_DEV_ATS_BASE`). Release builds
    /// do not contain these switches.
    pub fn new() -> Self {
        #[cfg(debug_assertions)]
        {
            let local = std::env::var_os("REMA_DEV_ALLOW_LOCAL_PAGES").is_some();
            let apis = match std::env::var("REMA_DEV_ATS_BASE") {
                Ok(base) => Apis::local(&base),
                Err(_) => Apis::official(),
            };
            Self::with(apis, local)
        }
        #[cfg(not(debug_assertions))]
        Self::with(Apis::official(), false)
    }

    pub fn with(apis: Apis, local_fixtures: bool) -> Self {
        Self {
            inner: Arc::new(Inner {
                fetcher: OnceLock::new(),
                feeds: FeedCache::default(),
                apis,
                local_fixtures,
                next: AtomicU64::new(1),
                active: Mutex::default(),
                health: Mutex::default(),
                enabled: AtomicBool::new(true),
                enabled_loaded: AtomicBool::new(false),
            }),
        }
    }

    pub fn fetcher(&self, version: &str) -> &Fetcher {
        self.inner
            .fetcher
            .get_or_init(|| Fetcher::new(version, self.inner.local_fixtures))
    }

    pub fn feeds(&self) -> &FeedCache {
        &self.inner.feeds
    }

    pub fn apis(&self) -> &Apis {
        &self.inner.apis
    }

    pub fn local_fixtures(&self) -> bool {
        self.inner.local_fixtures
    }

    /// What adapters need for one request (the fetcher, APIs and caches).
    pub fn ctx<'a>(
        &'a self,
        version: &str,
        cancel: &'a CancellationToken,
        deadline: std::time::Instant,
        refresh: bool,
    ) -> adapters::Ctx<'a> {
        adapters::Ctx {
            fetcher: self.fetcher(version),
            apis: self.apis(),
            feeds: self.feeds(),
            deadline,
            cancel,
            now: crate::time::now_ms(),
            refresh,
        }
    }

    /// Tracks a piece of running work so disabling can cancel it. The
    /// returned token is a child of `parent`.
    pub fn begin(&self, parent: &CancellationToken) -> Work {
        let id = self.inner.next.fetch_add(1, Ordering::Relaxed);
        let token = parent.child_token();
        self.inner.active.lock().unwrap().insert(id, token.clone());
        Work {
            runtime: self.clone(),
            id,
            cancel: token,
        }
    }

    fn end(&self, id: u64) {
        self.inner.active.lock().unwrap().remove(&id);
    }

    /// Cancels all running ReMa MCP work.
    pub fn cancel_all(&self) -> usize {
        let active: Vec<CancellationToken> = self
            .inner
            .active
            .lock()
            .unwrap()
            .drain()
            .map(|(_, t)| t)
            .collect();
        for token in &active {
            token.cancel();
        }
        active.len()
    }

    pub fn running(&self) -> usize {
        self.inner.active.lock().unwrap().len()
    }

    pub fn record(&self, source: &str, now: i64, result: Result<(), String>) {
        let mut health = self.inner.health.lock().unwrap();
        let entry = health.entry(source.to_string()).or_default();
        match result {
            Ok(()) => entry.last_success = Some(now),
            Err(message) => entry.last_error = Some((now, message)),
        }
    }

    pub fn health(&self, source: &str) -> Health {
        self.inner
            .health
            .lock()
            .unwrap()
            .get(source)
            .cloned()
            .unwrap_or_default()
    }
}

/// Running work, unregistered when dropped.
pub struct Work {
    runtime: RemaMcp,
    id: u64,
    pub cancel: CancellationToken,
}

impl Drop for Work {
    fn drop(&mut self) {
        self.runtime.end(self.id);
    }
}

/// Enabled on first launch: writes the default once, never overwriting the
/// user's saved choice.
pub fn ensure_default(state: &AppState) -> AppResult<bool> {
    let enabled = state
        .db
        .call(|c| match settings::get_setting(c, ENABLED_KEY)? {
            Some(value) => Ok(value == "true"),
            None => {
                settings::set_setting(c, ENABLED_KEY, "true")?;
                Ok(true)
            }
        })?;
    state
        .rema_mcp
        .inner
        .enabled
        .store(enabled, Ordering::SeqCst);
    state
        .rema_mcp
        .inner
        .enabled_loaded
        .store(true, Ordering::SeqCst);
    Ok(enabled)
}

/// Whether ReMa MCP is enabled (checked before every tool call).
pub fn is_enabled(state: &AppState) -> bool {
    let inner = &state.rema_mcp.inner;
    if !inner.enabled_loaded.load(Ordering::SeqCst) {
        return ensure_default(state).unwrap_or(false);
    }
    inner.enabled.load(Ordering::SeqCst)
}

/// Saves the switch. Disabling cancels running ReMa MCP work at once;
/// saved jobs, chats and history are kept.
pub fn set_enabled(state: &AppState, enabled: bool) -> AppResult<()> {
    state
        .db
        .call(|c| settings::set_setting(c, ENABLED_KEY, if enabled { "true" } else { "false" }))?;
    state
        .rema_mcp
        .inner
        .enabled
        .store(enabled, Ordering::SeqCst);
    state
        .rema_mcp
        .inner
        .enabled_loaded
        .store(true, Ordering::SeqCst);
    if !enabled {
        state.rema_mcp.cancel_all();
    }
    Ok(())
}
