use tauri::State;

use crate::{
    error::AppResult,
    services::websearch::{self, WebSearchInput, WebSearchSettings, WebSearchTest},
    state::AppState,
};

/// The search service ReMa uses for models without their own web search.
#[tauri::command]
#[specta::specta]
pub async fn web_search_settings(state: State<'_, AppState>) -> AppResult<WebSearchSettings> {
    websearch::settings(&state).await
}

/// Saves the search service; the key goes to the OS credential store.
#[tauri::command]
#[specta::specta]
pub async fn save_web_search_settings(
    state: State<'_, AppState>,
    input: WebSearchInput,
) -> AppResult<WebSearchSettings> {
    websearch::save(&state, input).await
}

/// Runs one real search with the saved service.
#[tauri::command]
#[specta::specta]
pub async fn test_web_search(state: State<'_, AppState>) -> AppResult<WebSearchTest> {
    websearch::test(&state).await
}
