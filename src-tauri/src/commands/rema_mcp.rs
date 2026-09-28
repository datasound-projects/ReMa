use tauri::State;

use crate::{
    error::AppResult,
    mcp::import::McpImportApp,
    rema_mcp::{
        host::{self, RemaMcpStatus},
        stdio::{self, RemaMcpLaunch},
    },
    state::AppState,
};

/// How another app (Claude Desktop, Claude Code, Codex, Cursor) starts
/// ReMa MCP: this ReMa's program with the `mcp` argument.
#[tauri::command]
#[specta::specta]
pub async fn rema_mcp_launch(state: State<'_, AppState>) -> AppResult<Option<RemaMcpLaunch>> {
    Ok(stdio::launch(&state))
}

/// Adds ReMa MCP to another app's MCP settings file; the file's path.
#[tauri::command]
#[specta::specta]
pub async fn add_rema_mcp_to(state: State<'_, AppState>, app: McpImportApp) -> AppResult<String> {
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || stdio::add_to(&state, app))
        .await
        .map_err(|e| crate::error::AppError::internal(e.to_string()))?
        .map(|path| path.to_string_lossy().into_owned())
}

/// ReMa MCP's card in Settings → MCP → Built-in. `check` also lists its
/// tools over MCP (nothing is searched).
#[tauri::command]
#[specta::specta]
pub async fn rema_mcp_status(state: State<'_, AppState>, check: bool) -> AppResult<RemaMcpStatus> {
    host::status(&state, check).await
}

/// Turns ReMa MCP on or off (the interface asks twice before turning it
/// off). Off: its tools leave every chat and running calls stop.
#[tauri::command]
#[specta::specta]
pub async fn set_rema_mcp_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<RemaMcpStatus> {
    host::set_enabled(&state, enabled).await
}

/// Clears ReMa MCP's cached jobs and searches (nothing else).
#[tauri::command]
#[specta::specta]
pub async fn clear_rema_mcp_cache(state: State<'_, AppState>) -> AppResult<RemaMcpStatus> {
    host::clear_cache(&state).await
}
