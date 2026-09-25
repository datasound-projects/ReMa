use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    error::{AppError, AppResult},
    models::system::AppStatus,
    services::{schedule, system},
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn get_app_status(state: State<'_, AppState>) -> AppResult<AppStatus> {
    Ok(system::app_status(&state.info))
}

/// IANA name of the computer's timezone, the default for new tasks.
#[tauri::command]
#[specta::specta]
pub async fn get_system_timezone() -> AppResult<String> {
    Ok(schedule::system_timezone_name())
}

/// Opens a link from chat content in the default browser.
#[tauri::command]
#[specta::specta]
pub async fn open_external_url(app: AppHandle, url: String) -> AppResult<()> {
    if !system::is_safe_external_url(&url) {
        return Err(AppError::validation(
            "Only web and email links can be opened.",
        ));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| AppError::internal(format!("could not open the link: {e}")))
}
