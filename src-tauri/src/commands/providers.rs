use tauri::State;

use crate::{
    error::AppResult,
    models::provider::{
        CustomProviderInput, ModelCatalog, ModelRef, ProviderKind, ProviderSettings, ProviderView,
    },
    services::providers,
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
