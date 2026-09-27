//! Network Connect: provider capabilities and professional research.
//! Results never carry a token; LinkedIn connection details stay in this
//! session's memory.

use std::sync::Arc;

use tauri::State;

use crate::{
    error::AppResult,
    events::EventSink,
    llm::Endpoint,
    network::{
        capabilities::{self, ProviderCapabilities},
        model::{LastNetworkResult, NetworkResearchInput, NetworkResult},
        service::{self, Request},
    },
    retrieval::Progress,
    services::providers,
    state::AppState,
};

/// What LinkedIn and XING let ReMa do right now.
#[tauri::command]
#[specta::specta]
pub async fn network_capabilities(
    state: State<'_, AppState>,
) -> AppResult<Vec<ProviderCapabilities>> {
    capabilities::all(&state).await
}

/// Reports a request's status lines to the page.
struct PageProgress {
    events: Arc<dyn EventSink>,
    run_id: String,
}

impl Progress for PageProgress {
    fn status(&self, text: &str) {
        self.events.network_progress(&self.run_id, text);
    }
}

/// The default model's own web search joins the research when it has one;
/// without a model, ReMa's own sources answer.
async fn default_model(state: &AppState) -> Option<(Endpoint, String)> {
    let model = providers::current_default_model(state).ok().flatten()?;
    let endpoint = providers::resolve_endpoint(state, &model.provider_id)
        .await
        .ok()?;
    Some((endpoint, model.model_id))
}

/// Runs a research request from the Network Connect page.
#[tauri::command]
#[specta::specta]
pub async fn network_research(
    state: State<'_, AppState>,
    input: NetworkResearchInput,
) -> AppResult<NetworkResult> {
    let state: AppState = (*state).clone();
    let cancel = state.network.begin(&input.run_id);
    let progress = PageProgress {
        events: state.events.clone(),
        run_id: input.run_id.clone(),
    };
    let model = default_model(&state).await;
    let request = Request {
        query: input.query.clone(),
        job_url: input.job_url.clone(),
        company: input.company.clone(),
        // The page uses the Profile only when the request refers to the
        // user's own background (NC §27).
        profile_allowed: true,
        purpose: None,
    };
    let result = service::research(
        &state,
        &request,
        model.as_ref().map(|(e, m)| (e, m.as_str())),
        &progress,
        &cancel,
    )
    .await;
    state.network.end(&input.run_id);
    state.network.remember_result(result.clone());
    Ok(result)
}

/// Stops a running request.
#[tauri::command]
#[specta::specta]
pub fn network_cancel(state: State<'_, AppState>, run_id: String) -> bool {
    state.network.cancel(&run_id)
}

/// The page's last result this session (nothing is kept on disk).
#[tauri::command]
#[specta::specta]
pub fn network_last_result(state: State<'_, AppState>) -> LastNetworkResult {
    LastNetworkResult {
        result: state.network.last_result(),
    }
}
