//! ReMa Business (Appendix B): find clients for a reviewed offer, find
//! advertised contract work, plan go-to-market experiments and keep one
//! commercial pipeline — separate from job Applications.
//!
//! Business is the commercial application of Network Connect's shared
//! research layer (companies, jobs, people, evidence, the provider data
//! policy); it adds no search engine, credential store or scheduler.
//!
//! - [`offers`] and [`ingest`]: a product or service description or URL
//!   becomes a user-reviewed, versioned offer with provenance.
//! - [`clients`] and [`fit`]: prospects with evidence and a reproducible
//!   fit measure (never a probability of buying).
//! - [`contracts`]: advertised engagements with their terms as stated.
//! - [`pipeline`]: saved opportunities, stages and user-reported activity.
//! - [`gtm`] and [`experiments`]: hypotheses, positioning, channels, local
//!   drafts, experiments and deterministic metrics.
//!
//! Nothing here sends a message, submits a proposal, pays for anything or
//! contacts anyone.

pub mod clients;
pub mod contracts;
pub mod experiments;
pub mod fit;
pub mod gtm;
pub mod ingest;
pub mod locations;
pub mod model;
pub mod offers;
pub mod pipeline;
pub mod render;
pub mod service;
pub mod store;
#[cfg(test)]
mod tests;
pub mod text;
pub mod tools;

use std::{collections::HashMap, sync::Mutex};

use tokio_util::sync::CancellationToken;

/// Running Business requests (in memory; the runs themselves are stored).
#[derive(Default)]
pub struct BusinessSession {
    runs: Mutex<HashMap<String, CancellationToken>>,
}

impl BusinessSession {
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

/// A new identifier made by ReMa (never by a model): `prefix_` and 16 hex
/// characters.
pub fn new_id(prefix: &str) -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("system randomness");
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{prefix}_{hex}")
}
