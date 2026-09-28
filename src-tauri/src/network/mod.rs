//! Network Connect: professional research across companies, jobs, people
//! and — only where a provider permits it — the user's own connections.
//!
//! The order of trust never inverts (NC "Final Product Rule"): provider
//! capability → permission → data policy → research → normalized evidence
//! → model. The model reasons over data ReMa is allowed to use; it never
//! decides what provider data ReMa may access.
//!
//! - [`capabilities`]: what each network actually grants (from the granted
//!   scopes, never from a Connect button).
//! - [`policy`]: what may be fetched, shown, sent to a model, stored or
//!   derived, per source, data class and purpose.
//! - [`planner`]: the request as criteria and the stages it needs.

pub mod capabilities;
pub mod companies;
pub mod contacts;
pub mod evidence;
pub mod model;
pub mod people;
pub mod planner;
pub mod policy;
pub mod relationships;
pub mod render;
pub mod resolve;
pub mod service;
#[cfg(test)]
pub(crate) mod tests;
pub mod tools;

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use tokio_util::sync::CancellationToken;

/// What Network Connect keeps while ReMa runs, never on disk: the user's
/// LinkedIn connections (at most [`policy::SESSION_TTL_SECS`], dropped on
/// disconnect or expiry), the page's last result, and running requests.
#[derive(Default)]
pub struct NetworkSession {
    connections: Mutex<Option<(Instant, Vec<relationships::Member>)>>,
    last: Mutex<Option<(Instant, model::NetworkResult)>>,
    runs: Mutex<HashMap<String, CancellationToken>>,
}

fn session_ttl() -> Duration {
    Duration::from_secs(u64::from(policy::SESSION_TTL_SECS))
}

/// Removes what a provider returned from a result, saying so where the
/// connection check was shown (never an empty "Your connections").
fn drop_connections(result: &mut model::NetworkResult, reason: &str) {
    result.connections.clear();
    for person in &mut result.people {
        person.relationship = None;
    }
    if matches!(
        result.connections_outcome,
        model::ConnectionsOutcome::Checked { .. }
    ) {
        result.connections_outcome = model::ConnectionsOutcome::Unavailable {
            reason: reason.to_string(),
        };
    }
}

impl NetworkSession {
    pub fn cached_connections(&self) -> Option<Vec<relationships::Member>> {
        let mut cached = self.connections.lock().unwrap();
        match &*cached {
            Some((at, members)) if at.elapsed() < session_ttl() => Some(members.clone()),
            Some(_) => {
                *cached = None;
                None
            }
            None => None,
        }
    }

    pub fn remember_connections(&self, members: Vec<relationships::Member>) {
        *self.connections.lock().unwrap() = Some((Instant::now(), members));
    }

    /// The provider was disconnected or its access expired: nothing it
    /// returned is kept.
    pub fn forget_provider_data(&self) {
        *self.connections.lock().unwrap() = None;
        if let Some((_, last)) = self.last.lock().unwrap().as_mut() {
            drop_connections(
                last,
                "LinkedIn was disconnected: the connection details it returned were removed.",
            );
        }
    }

    pub fn remember_result(&self, result: model::NetworkResult) {
        *self.last.lock().unwrap() = Some((Instant::now(), result));
    }

    /// The page's last result this session (connections expire with the
    /// session data they came from).
    pub fn last_result(&self) -> Option<model::NetworkResult> {
        let mut last = self.last.lock().unwrap();
        let (at, result) = last.as_mut()?;
        if at.elapsed() >= session_ttl() {
            drop_connections(
                result,
                "Connection details are kept for a limited time only; ask again to check them.",
            );
        }
        Some(result.clone())
    }

    pub fn begin(&self, run_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.runs
            .lock()
            .unwrap()
            .insert(run_id.to_string(), token.clone());
        token
    }

    pub fn end(&self, run_id: &str) {
        self.runs.lock().unwrap().remove(run_id);
    }

    pub fn cancel(&self, run_id: &str) -> bool {
        match self.runs.lock().unwrap().get(run_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}
