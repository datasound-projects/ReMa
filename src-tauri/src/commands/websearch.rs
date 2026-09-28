use tauri::State;

use crate::{
    career_search::{
        mode::{self, AnswerMode},
        status::{self, CareerSearchStatus},
    },
    error::AppResult,
    services::websearch::{self, WebSearchInput, WebSearchSettings, WebSearchTest},
    state::AppState,
};

/// Career search: automatic, its routes and sources (no setup needed).
#[tauri::command]
#[specta::specta]
pub async fn career_search_status(state: State<'_, AppState>) -> AppResult<CareerSearchStatus> {
    status::status(&state).await
}

/// How chats answer questions that need the web: the model's own search
/// (like ChatGPT and Claude) or ReMa's verified search.
#[tauri::command]
#[specta::specta]
pub async fn set_answer_mode(
    state: State<'_, AppState>,
    mode: AnswerMode,
) -> AppResult<CareerSearchStatus> {
    mode::set(&state, mode)?;
    status::status(&state).await
}

/// Checks that ReMa's own sources and company research answer now.
#[tauri::command]
#[specta::specta]
pub async fn check_career_search(state: State<'_, AppState>) -> AppResult<CareerSearchStatus> {
    status::check(&state).await
}

/// An optional search service (Advanced) that adds results.
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
