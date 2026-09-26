use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    error::{AppError, AppResult},
    models::mcp::{McpServer, McpServerInput, McpTestResult},
    services::mcp,
    state::AppState,
};

/// Configured servers. Secret values never leave Rust (`hasSecret` only).
#[tauri::command]
#[specta::specta]
pub async fn list_mcp_servers(state: State<'_, AppState>) -> AppResult<Vec<McpServer>> {
    mcp::list(&state)
}

/// Adds (`id` = null, always disabled) or edits a server. Secrets left
/// empty keep their stored value.
#[tauri::command]
#[specta::specta]
pub async fn save_mcp_server(
    state: State<'_, AppState>,
    id: Option<i64>,
    input: McpServerInput,
) -> AppResult<McpServer> {
    mcp::save(&state, id, input).await
}

/// Removes a server, its secrets and its selection in every chat.
#[tauri::command]
#[specta::specta]
pub async fn delete_mcp_server(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    mcp::delete(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_mcp_server_enabled(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> AppResult<McpServer> {
    mcp::set_enabled(&state, id, enabled)
}

/// Connects (or reconnects) an enabled server.
#[tauri::command]
#[specta::specta]
pub async fn connect_mcp_server(state: State<'_, AppState>, id: i64) -> AppResult<McpServer> {
    mcp::reconnect(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn disconnect_mcp_server(state: State<'_, AppState>, id: i64) -> AppResult<McpServer> {
    mcp::disconnect(&state, id)
}

/// Tries the form's configuration without saving it.
#[tauri::command]
#[specta::specta]
pub async fn test_mcp_server(
    state: State<'_, AppState>,
    id: Option<i64>,
    input: McpServerInput,
) -> AppResult<McpTestResult> {
    mcp::test(&state, id, input).await
}

/// OAuth sign-in in the system browser; resolves when it completes.
#[tauri::command]
#[specta::specta]
pub async fn sign_in_mcp_server(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<McpServer> {
    mcp::sign_in(&state, id, |url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| AppError::internal(format!("could not open the browser: {e}")))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_mcp_sign_in(state: State<'_, AppState>, id: i64) -> AppResult<McpServer> {
    mcp::cancel_sign_in(&state, id)
}

#[tauri::command]
#[specta::specta]
pub async fn sign_out_mcp_server(state: State<'_, AppState>, id: i64) -> AppResult<McpServer> {
    mcp::sign_out(&state, id).await
}
