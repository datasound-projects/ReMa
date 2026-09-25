use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    error::{AppError, AppResult},
    integrations::google,
    models::google::{GoogleService, GoogleStatus},
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn get_google_status(state: State<'_, AppState>) -> AppResult<GoogleStatus> {
    google::status(&state).await
}

/// Stores the user's own Google Cloud "Desktop app" OAuth client.
#[tauri::command]
#[specta::specta]
pub async fn save_google_client(
    state: State<'_, AppState>,
    client_id: String,
    client_secret: String,
) -> AppResult<GoogleStatus> {
    google::save_client(&state, &client_id, &client_secret).await?;
    google::status(&state).await
}

/// Opens Google's consent page in the browser and waits for the redirect.
#[tauri::command]
#[specta::specta]
pub async fn connect_google(app: AppHandle, state: State<'_, AppState>) -> AppResult<GoogleStatus> {
    google::connect(&state, |url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| AppError::internal(format!("could not open the browser: {e}")))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_google_connect(state: State<'_, AppState>) -> AppResult<()> {
    google::cancel_connect(&state);
    Ok(())
}

/// Revokes ReMa's access at Google and removes the stored tokens.
#[tauri::command]
#[specta::specta]
pub async fn disconnect_google(state: State<'_, AppState>) -> AppResult<GoogleStatus> {
    google::disconnect(&state).await?;
    google::status(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_google_service_enabled(
    state: State<'_, AppState>,
    service: GoogleService,
    enabled: bool,
) -> AppResult<GoogleStatus> {
    google::set_service_enabled(&state, service, enabled)?;
    google::status(&state).await
}
