use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    browser::{self, autofill, HistoryStep},
    error::{AppError, AppResult},
    models::browser::{AutofillResult, BrowserBounds, BrowserStatus},
    services::profile,
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn get_browser_status(state: State<'_, AppState>) -> AppResult<BrowserStatus> {
    Ok(state.browser.status())
}

/// Opens a web page in the browser workspace at `bounds`.
#[tauri::command]
#[specta::specta]
pub async fn open_in_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    bounds: BrowserBounds,
) -> AppResult<BrowserStatus> {
    browser::open(&app, &state, &url, bounds)
}

#[tauri::command]
#[specta::specta]
pub async fn set_browser_bounds(app: AppHandle, bounds: BrowserBounds) -> AppResult<()> {
    browser::set_bounds(&app, bounds)
}

#[tauri::command]
#[specta::specta]
pub async fn set_browser_visible(app: AppHandle, visible: bool) -> AppResult<()> {
    browser::set_visible(&app, visible)
}

#[tauri::command]
#[specta::specta]
pub async fn browser_back(app: AppHandle) -> AppResult<()> {
    browser::go(&app, HistoryStep::Back)
}

#[tauri::command]
#[specta::specta]
pub async fn browser_forward(app: AppHandle) -> AppResult<()> {
    browser::go(&app, HistoryStep::Forward)
}

#[tauri::command]
#[specta::specta]
pub async fn browser_reload(app: AppHandle) -> AppResult<()> {
    browser::reload(&app)
}

#[tauri::command]
#[specta::specta]
pub async fn close_browser(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    browser::close(&app, &state)
}

/// Fills the current page's form from the profile. Never submits.
#[tauri::command]
#[specta::specta]
pub async fn run_autofill(app: AppHandle, state: State<'_, AppState>) -> AppResult<AutofillResult> {
    autofill::run(&app, &state).await
}

/// Puts a profile document into an upload field found by Auto Fill.
#[tauri::command]
#[specta::specta]
pub async fn attach_profile_document(
    app: AppHandle,
    state: State<'_, AppState>,
    field: u32,
    document_id: i64,
) -> AppResult<()> {
    autofill::attach(&app, &state, field, document_id).await
}

/// Shows a profile document in the system file manager, so the user can
/// choose it in a website's own upload dialog.
#[tauri::command]
#[specta::specta]
pub async fn reveal_profile_document(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<()> {
    let (_, path) = profile::document_path(&state, id)?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| AppError::internal(format!("could not show the file: {e}")))
}
