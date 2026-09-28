//! What Settings shows about career search (§41, §49–§51): that it is
//! automatic, which routes the selected model has, how ReMa's own sources
//! are doing, and a health check on demand. Nothing here can turn career
//! search off.

use std::time::{Duration, Instant};

use serde::Serialize;
use specta::Type;
use tokio_util::sync::CancellationToken;

use super::{
    capabilities::{self, RuntimeCapabilities},
    health::{HealthState, SourceHealth},
    RouteReport,
};
use crate::{
    error::AppResult,
    llm::Endpoint,
    models::provider::ProviderKind,
    rema_mcp::adapters::boards,
    retrieval::{backend, native},
    services::providers,
    state::AppState,
};

/// ReMa's own sources as Settings lists them: (health id, name).
const OWN_SOURCES: &[(&str, &str)] = &[
    (boards::ARBEITNOW, "Arbeitnow"),
    (boards::THEMUSE, "The Muse"),
    (boards::REMOTIVE, "Remotive"),
    (boards::HN, "Hacker News hiring thread"),
    ("wikidata", "Wikidata"),
];

/// One way ReMa can search.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RouteState {
    pub name: String,
    pub detail: String,
    pub available: bool,
}

/// One result of a health check.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// Settings → Career Search.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CareerSearchStatus {
    /// Always true: career search needs no setup.
    pub automatic: bool,
    /// The default chat model.
    pub model: Option<String>,
    /// Its own web search ("Anthropic web search"), if it has one.
    pub model_search: Option<String>,
    /// What the selected model's runtime can do for search (§6): found
    /// from the runtime, never from the provider's name alone.
    pub capabilities: Option<RuntimeCapabilities>,
    pub routes: Vec<RouteState>,
    /// An optional search service set up under Advanced.
    pub extra_service: Option<String>,
    pub sources: Vec<SourceHealth>,
    /// Recent searches (diagnostics; no page text, no credentials).
    pub recent: Vec<RouteReport>,
    /// Filled by a health check.
    pub checked: Vec<CheckResult>,
    /// How chats answer questions that need the web.
    pub answer_mode: super::mode::AnswerMode,
}

/// The default chat model, its name and endpoint.
async fn default_model(state: &AppState) -> Option<(String, Endpoint, String)> {
    let catalog = providers::catalog(state).ok()?;
    let model = catalog.default_model?;
    let endpoint = providers::resolve_endpoint(state, &model.provider_id)
        .await
        .ok()?;
    let name = catalog
        .models
        .iter()
        .find(|m| m.model == model)
        .map(|m| m.display_name.clone())
        .unwrap_or_else(|| model.model_id.clone());
    Some((name, endpoint, model.model_id))
}

fn model_route(endpoint: Option<&Endpoint>, caps: Option<&RuntimeCapabilities>) -> RouteState {
    match endpoint {
        // The provider has one, but the account or organization may not
        // use it now: ReMa's own search answers instead.
        Some(e) if native::supported(e) && caps.is_some_and(|c| !c.native_search_available) => {
            RouteState {
                name: native::engine_name(e).to_string(),
                detail: format!(
                    "Not used right now: {}. ReMa searches for this model with its own sources.",
                    caps.and_then(|c| c.note.clone())
                        .unwrap_or_else(|| "not available".into())
                ),
                available: false,
            }
        }
        Some(e) if native::supported(e) => RouteState {
            name: native::engine_name(e).to_string(),
            detail: if e.kind == ProviderKind::OpenaiCompatible {
                "The local server's own web search, turned on automatically for career searches."
                    .into()
            } else if caps.is_some_and(|c| !c.native_search_live) {
                format!(
                    "The selected model's own web search ({}).",
                    caps.and_then(|c| c.note.clone()).unwrap_or_default()
                )
            } else {
                "The selected model's own web search, live, localized to the place you ask \
                 about."
                    .into()
            },
            available: true,
        },
        Some(_) => RouteState {
            name: "Model web search".into(),
            detail: "This model has no web search of its own; ReMa searches for it and gives it \
                     the results."
                .into(),
            available: false,
        },
        None => RouteState {
            name: "Model web search".into(),
            detail: "Used when the selected model has one.".into(),
            available: false,
        },
    }
}

/// What Settings shows (no network requests).
pub async fn status(state: &AppState) -> AppResult<CareerSearchStatus> {
    let default = default_model(state).await;
    let caps = match &default {
        Some((_, endpoint, model_id)) => {
            Some(capabilities::detect(state, endpoint, model_id).await)
        }
        None => None,
    };
    let extra_service = backend::configured(state)
        .await
        .ok()
        .flatten()
        .map(|s| s.name().to_string());
    let health = &state.career.health;
    let own_healthy = OWN_SOURCES
        .iter()
        .any(|(id, _)| health.state(id) != HealthState::TemporarilyUnavailable);
    let mut routes = vec![
        RouteState {
            name: "ReMa job sources".into(),
            detail: "Employers' own job boards (Greenhouse, Lever, Ashby, Personio, \
                     SmartRecruiters, Workable, Recruitee) and public job boards (Arbeitnow, The \
                     Muse, Remotive, Hacker News). No key needed."
                .into(),
            available: own_healthy,
        },
        RouteState {
            name: "Company and people research".into(),
            detail: "Wikidata, Wikipedia and companies' own websites (careers, team and about \
                     pages). No key needed."
                .into(),
            available: health.state("wikidata") != HealthState::TemporarilyUnavailable,
        },
        model_route(default.as_ref().map(|(_, e, _)| e), caps.as_ref()),
    ];
    if let Some(name) = &extra_service {
        routes.push(RouteState {
            name: name.clone(),
            detail: "Optional service from Advanced: adds its results.".into(),
            available: true,
        });
    }
    Ok(CareerSearchStatus {
        automatic: true,
        model: default.as_ref().map(|(name, _, _)| name.clone()),
        model_search: default
            .as_ref()
            .filter(|(_, e, _)| native::supported(e))
            .map(|(_, e, _)| native::engine_name(e).to_string()),
        capabilities: caps,
        routes,
        extra_service,
        sources: OWN_SOURCES
            .iter()
            .map(|(id, name)| {
                let mut snapshot = health.snapshot(id);
                snapshot.id = (*name).to_string();
                snapshot
            })
            .collect(),
        recent: state.career.reports().into_iter().take(5).collect(),
        checked: Vec::new(),
        answer_mode: super::mode::get(state),
    })
}

/// A health check (§50): ReMa's job sources and company research answer,
/// and whether the selected model has a web search of its own. Rested
/// sources are asked again. A failure here never turns search off.
pub async fn check(state: &AppState) -> AppResult<CareerSearchStatus> {
    state.career.health.reset();
    let cancel = CancellationToken::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    let ctx = state
        .rema_mcp
        .ctx(&state.info.version, &cancel, deadline, true);
    let mut checked = Vec::new();
    let jobs = boards::arbeitnow(&ctx, 1).await;
    match &jobs {
        Ok(_) => state.career.health.success(boards::ARBEITNOW),
        Err(e) => state
            .career
            .health
            .failure(boards::ARBEITNOW, &e.message, false),
    }
    checked.push(CheckResult {
        name: "ReMa job sources".into(),
        ok: jobs.is_ok(),
        detail: match &jobs {
            Ok(list) => format!("Answered ({} current jobs on the first page).", list.len()),
            Err(e) => format!("Not reachable right now: {}.", e.message),
        },
    });
    let company = state.career.company(&ctx, "Siemens").await;
    match &company {
        Ok(_) => state.career.health.success("wikidata"),
        Err(e) => state.career.health.failure("wikidata", &e.message, false),
    }
    checked.push(CheckResult {
        name: "Company research".into(),
        ok: company.is_ok(),
        detail: match &company {
            Ok(Some(_)) => "Answered.".into(),
            Ok(None) => "Answered (no match for the test company).".into(),
            Err(e) => format!("Not reachable right now: {}.", e.message),
        },
    });
    let default = default_model(state).await;
    checked.push(match &default {
        // Proven by running it: the provider's own search events, the pages
        // they returned and one of them opened, not the model's say-so.
        Some((name, endpoint, model)) if native::supported(endpoint) => {
            let run = super::proof::prove(
                state.llm.as_ref(),
                state.rema_mcp.fetcher(&state.info.version),
                endpoint,
                model,
                cancel.clone(),
            );
            let outcome = tokio::time::timeout(Duration::from_secs(120), run).await;
            let (ok, detail) = match outcome {
                Ok(Ok(proof)) => match proof.verdict() {
                    Ok(evidence) => (
                        true,
                        format!("{name}, {}: {evidence}", native::engine_name(endpoint)),
                    ),
                    Err(problem) => (false, format!("{name}: {problem}")),
                },
                Ok(Err(error)) => (false, format!("{name} could not search: {error}")),
                Err(_) => (
                    false,
                    format!("{name} did not finish a search within 2 minutes."),
                ),
            };
            CheckResult {
                name: "Model web search".into(),
                ok,
                detail,
            }
        }
        Some((name, _, _)) => CheckResult {
            name: "Model web search".into(),
            ok: true,
            detail: format!(
                "{name} has no web search of its own; ReMa's career search tools are used instead."
            ),
        },
        None => CheckResult {
            name: "Model web search".into(),
            ok: true,
            detail: "No default model is set; ReMa's own sources still work.".into(),
        },
    });
    let mut status = status(state).await?;
    status.checked = checked;
    Ok(status)
}

/// At start-up (§51): learn what the connected local servers can do, so a
/// search never waits for it and an incompatibility shows before the first
/// request.
pub async fn detect_capabilities(state: &AppState) {
    let Ok(rows) = state.db.call(|c| crate::db::providers::list(c)) else {
        return;
    };
    for row in rows
        .iter()
        .filter(|r| r.kind == ProviderKind::OpenaiCompatible)
    {
        // Resolving the endpoint asks the server once and remembers it.
        let _ = providers::resolve_endpoint(state, &row.id).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing};

    #[tokio::test]
    async fn is_automatic_and_works_with_no_service_set_up() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&["ok"])));
        let status = status(&state).await.unwrap();
        assert!(status.automatic);
        assert_eq!(status.extra_service, None);
        assert!(
            status.routes[0].available,
            "ReMa's own sources need no setup"
        );
        assert!(!serde_json::to_string(&status)
            .unwrap()
            .to_lowercase()
            .contains("api key"));
        // A check never fails as a whole, even when nothing is reachable.
        let checked = check(&state).await.unwrap();
        assert_eq!(checked.checked.len(), 3);
        assert!(checked.checked.iter().any(|c| c.name == "ReMa job sources"));
    }
}
