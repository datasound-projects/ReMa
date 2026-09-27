//! Source and route health (§34–§36): one failing source never fails a
//! request, and a source that keeps failing is rested for a while instead
//! of being asked again and again.
//!
//! - Healthy: the last attempt worked (or none was made).
//! - Degraded: recent failures, still tried.
//! - Temporarily unavailable: after [`TRIP_AFTER`] failures in a row (or a
//!   rate limit) the source is skipped until its cool-down ends; the pause
//!   doubles with every further failure (with jitter), up to
//!   [`MAX_COOLDOWN`]. The first attempt after it ends decides again.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::Serialize;
use specta::Type;

use crate::time::now_ms;

/// Failures in a row before a source is rested.
pub const TRIP_AFTER: u32 = 3;
const BASE_COOLDOWN: Duration = Duration::from_secs(30);
pub const MAX_COOLDOWN: Duration = Duration::from_secs(15 * 60);
/// A rate limit rests the source at least this long.
const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Healthy,
    Degraded,
    TemporarilyUnavailable,
}

#[derive(Debug, Default)]
struct Entry {
    failures: u32,
    rested_until: Option<Instant>,
    last_error: Option<String>,
    last_success_at: Option<i64>,
    last_failure_at: Option<i64>,
}

/// Health of one source, as shown in Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceHealth {
    pub id: String,
    pub state: HealthState,
    pub failures: u32,
    pub last_error: Option<String>,
    pub last_success_at: Option<i64>,
    pub last_failure_at: Option<i64>,
}

#[derive(Debug, Default)]
pub struct HealthBook {
    entries: Mutex<HashMap<String, Entry>>,
}

fn jitter(max_ms: u64) -> Duration {
    let mut byte = [0u8; 2];
    let _ = getrandom::fill(&mut byte);
    Duration::from_millis(u64::from(u16::from_le_bytes(byte)) % max_ms.max(1))
}

impl HealthBook {
    /// Whether the source may be asked now.
    pub fn allow(&self, id: &str) -> bool {
        self.entries
            .lock()
            .unwrap()
            .get(id)
            .and_then(|e| e.rested_until)
            .is_none_or(|until| Instant::now() >= until)
    }

    pub fn success(&self, id: &str) {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.entry(id.to_string()).or_default();
        entry.failures = 0;
        entry.rested_until = None;
        entry.last_success_at = Some(now_ms());
    }

    /// Records a failure; `rate_limited` rests the source at once.
    pub fn failure(&self, id: &str, error: &str, rate_limited: bool) {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.entry(id.to_string()).or_default();
        entry.failures += 1;
        entry.last_error = Some(error.chars().take(200).collect());
        entry.last_failure_at = Some(now_ms());
        let pause = if entry.failures >= TRIP_AFTER {
            let doublings = (entry.failures - TRIP_AFTER).min(10);
            Some((BASE_COOLDOWN * 2u32.pow(doublings)).min(MAX_COOLDOWN))
        } else {
            None
        };
        let pause = match (pause, rate_limited) {
            (Some(p), true) => Some(p.max(RATE_LIMIT_COOLDOWN)),
            (None, true) => Some(RATE_LIMIT_COOLDOWN),
            (p, false) => p,
        };
        entry.rested_until = pause.map(|p| Instant::now() + p + jitter(p.as_millis() as u64 / 10));
    }

    pub fn state(&self, id: &str) -> HealthState {
        let entries = self.entries.lock().unwrap();
        match entries.get(id) {
            None => HealthState::Healthy,
            Some(e) if e.rested_until.is_some_and(|until| Instant::now() < until) => {
                HealthState::TemporarilyUnavailable
            }
            Some(e) if e.failures > 0 => HealthState::Degraded,
            Some(_) => HealthState::Healthy,
        }
    }

    pub fn snapshot(&self, id: &str) -> SourceHealth {
        let state = self.state(id);
        let entries = self.entries.lock().unwrap();
        let entry = entries.get(id);
        SourceHealth {
            id: id.to_string(),
            state,
            failures: entry.map_or(0, |e| e.failures),
            last_error: entry.and_then(|e| e.last_error.clone()),
            last_success_at: entry.and_then(|e| e.last_success_at),
            last_failure_at: entry.and_then(|e| e.last_failure_at),
        }
    }

    /// Ends every rest (after the user asks for a fresh check).
    pub fn reset(&self) {
        for entry in self.entries.lock().unwrap().values_mut() {
            entry.rested_until = None;
            entry.failures = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_failures_rest_a_source_and_a_success_restores_it() {
        let book = HealthBook::default();
        assert!(book.allow("arbeitnow"));
        assert_eq!(book.state("arbeitnow"), HealthState::Healthy);
        book.failure("arbeitnow", "timeout", false);
        assert!(
            book.allow("arbeitnow"),
            "one failure does not rest a source"
        );
        assert_eq!(book.state("arbeitnow"), HealthState::Degraded);
        book.failure("arbeitnow", "timeout", false);
        book.failure("arbeitnow", "timeout", false);
        assert!(!book.allow("arbeitnow"));
        assert_eq!(book.state("arbeitnow"), HealthState::TemporarilyUnavailable);
        assert!(book.allow("themuse"), "other sources are unaffected");
        book.success("arbeitnow");
        assert!(book.allow("arbeitnow"));
        assert_eq!(book.snapshot("arbeitnow").failures, 0);
    }

    #[test]
    fn a_rate_limit_rests_the_source_at_once() {
        let book = HealthBook::default();
        book.failure("remotive", "rate limited", true);
        assert!(!book.allow("remotive"));
        book.reset();
        assert!(book.allow("remotive"));
    }
}
