use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    error::{AppError, AppResult},
    models::provider::{
        CustomProviderInput, ModelCatalog, ModelRef, ProviderKind, ProviderSettings, ProviderView,
    },
    services::{accounts, providers},
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn get_provider_settings(state: State<'_, AppState>) -> AppResult<ProviderSettings> {
    providers::settings(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn get_model_catalog(state: State<'_, AppState>) -> AppResult<ModelCatalog> {
    providers::catalog(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn connect_provider(
    state: State<'_, AppState>,
    kind: ProviderKind,
    api_key: String,
) -> AppResult<ProviderView> {
    providers::connect(&state, kind, &api_key).await
}

#[tauri::command]
#[specta::specta]
pub async fn disconnect_provider(state: State<'_, AppState>, provider_id: String) -> AppResult<()> {
    providers::disconnect(&state, &provider_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_custom_provider(
    state: State<'_, AppState>,
    input: CustomProviderInput,
) -> AppResult<ProviderView> {
    providers::save_custom(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn refresh_provider_models(
    state: State<'_, AppState>,
    provider_id: String,
) -> AppResult<ProviderView> {
    providers::refresh_models(&state, &provider_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_model_enabled(
    state: State<'_, AppState>,
    model: ModelRef,
    enabled: bool,
) -> AppResult<()> {
    providers::set_model_enabled(&state, &model, enabled)
}

#[tauri::command]
#[specta::specta]
pub async fn set_default_model(state: State<'_, AppState>, model: ModelRef) -> AppResult<()> {
    providers::set_default_model(&state, &model)
}

/// Starts the account sign-in (ChatGPT, Claude Console) in the system
/// browser. Returns at once; progress arrives as `ProvidersChanged`.
#[tauri::command]
#[specta::specta]
pub async fn start_provider_sign_in(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: ProviderKind,
    device_code: bool,
) -> AppResult<ProviderView> {
    accounts::start_sign_in(&state, kind, device_code, |url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| AppError::internal(e.to_string()))
    })
    .await
}

/// Cancels a running sign-in, or clears a finished one's notice.
#[tauri::command]
#[specta::specta]
pub async fn cancel_provider_sign_in(
    state: State<'_, AppState>,
    kind: ProviderKind,
) -> AppResult<ProviderView> {
    accounts::cancel_sign_in(&state, kind)
}

/// Asks an account connection's runtime whether the sign-in still works.
#[tauri::command]
#[specta::specta]
pub async fn check_provider_connection(
    state: State<'_, AppState>,
    provider_id: String,
) -> AppResult<ProviderView> {
    accounts::check(&state, &provider_id).await
}

/// Signs out with the runtime's official logout and disconnects.
#[tauri::command]
#[specta::specta]
pub async fn sign_out_provider(state: State<'_, AppState>, provider_id: String) -> AppResult<()> {
    accounts::sign_out(&state, &provider_id).await
}
