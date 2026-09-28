//! Network Connect: provider capabilities and professional research.
//! Results never carry a token; LinkedIn connection details stay in this
//! session's memory.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    error::{AppError, AppResult},
    events::EventSink,
    llm::Endpoint,
    network::{
        capabilities::{self, ProviderCapabilities},
        contacts::{self, ContactSource, ContactsImported, ContactsSummary},
        model::{LastNetworkResult, NetworkResearchInput, NetworkResult},
        service::{self, Request},
    },
    retrieval::Progress,
    services::providers,
    state::AppState,
};

/// Contacts the user imported (their LinkedIn export, vCards): counts only.
#[tauri::command]
#[specta::specta]
pub fn network_contacts(state: State<'_, AppState>) -> AppResult<ContactsSummary> {
    contacts::summary(&state)
}

/// Lets the user pick their LinkedIn data export (the ZIP, or
/// Connections.csv) or vCard files and imports the contacts in them.
/// `None` if the user cancelled.
#[tauri::command]
#[specta::specta]
pub async fn import_network_contacts(
    app: AppHandle,
    state: State<'_, AppState>,
    source: ContactSource,
) -> AppResult<Option<ContactsImported>> {
    let dialog = app.dialog().file();
    let paths: Vec<std::path::PathBuf> = match source {
        ContactSource::LinkedinExport => dialog
            .set_title("Choose your LinkedIn data export (ZIP or Connections.csv)")
            .add_filter("LinkedIn export", &["zip", "csv"])
            .blocking_pick_file()
            .into_iter()
            .collect(),
        ContactSource::Vcard => dialog
            .set_title("Choose contact cards (vCard)")
            .add_filter("vCard", &["vcf", "vcard"])
            .blocking_pick_files()
            .unwrap_or_default(),
    }
    .into_iter()
    .filter_map(|picked| picked.into_path().ok())
    .collect();
    if paths.is_empty() {
        return Ok(None);
    }
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || contacts::import(&state, source, &paths))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
        .map(Some)
}

/// Removes imported contacts (one source, or all).
#[tauri::command]
#[specta::specta]
pub fn clear_network_contacts(
    state: State<'_, AppState>,
    source: Option<ContactSource>,
) -> AppResult<ContactsSummary> {
    contacts::clear(&state, source)
}

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
