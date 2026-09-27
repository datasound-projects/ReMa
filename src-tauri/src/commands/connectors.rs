use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::{
    connectors,
    error::{AppError, AppResult},
    models::connectors::{ConnectorId, ConnectorsOverview, ProviderId},
    services::background,
    state::AppState,
};

/// Every connector's state. Never contains tokens.
#[tauri::command]
#[specta::specta]
pub async fn get_connectors(state: State<'_, AppState>) -> AppResult<ConnectorsOverview> {
    connectors::overview(&state).await
}

/// Opens the provider's sign-in in the default browser and waits for the
/// redirect. Resolves when the account is connected (or the sign-in failed,
/// was cancelled or timed out).
#[tauri::command]
#[specta::specta]
pub async fn connect_connector(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ConnectorId,
) -> AppResult<ConnectorsOverview> {
    connectors::connect(&state, id, |url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| AppError::internal(format!("could not open the browser: {e}")))
    })
    .await?;
    connectors::overview(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_connector_sign_in(
    state: State<'_, AppState>,
    provider: ProviderId,
) -> AppResult<()> {
    state.connectors.cancel_sign_in(provider);
    state.events.connectors_changed();
    Ok(())
}

/// Stops the connector's sync; the last connector of an account also signs
/// out (revokes access where the provider allows it and deletes the tokens).
/// Application history is kept.
#[tauri::command]
#[specta::specta]
pub async fn disconnect_connector(
    state: State<'_, AppState>,
    id: ConnectorId,
) -> AppResult<ConnectorsOverview> {
    connectors::disconnect(&state, id).await?;
    connectors::overview(&state).await
}

/// "Run ReMa in background" (system tray) and "Start ReMa at login".
#[tauri::command]
#[specta::specta]
pub async fn set_background_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    run_in_background: bool,
    start_at_login: bool,
) -> AppResult<ConnectorsOverview> {
    if run_in_background && !background::tray_available() {
        return Err(AppError::validation(
            "Running in the background needs a system tray, which is not available here.",
        ));
    }
    let autostart = app.autolaunch();
    let result = if start_at_login {
        autostart.enable()
    } else {
        autostart.disable()
    };
    // Disabling an entry that does not exist is fine; enabling must work.
    if let (Err(error), true) = (result, start_at_login) {
        return Err(AppError::internal(format!(
            "ReMa could not be added to the programs that start at login: {error}"
        )));
    }
    background::save(&state, run_in_background, start_at_login)?;
    if let Some(tray) = app.tray_by_id(crate::TRAY_ID) {
        let _ = tray.set_visible(run_in_background);
    }
    connectors::overview(&state).await
}
