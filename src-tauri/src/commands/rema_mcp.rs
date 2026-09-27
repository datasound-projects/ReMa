use tauri::State;

use crate::{
    error::AppResult,
    rema_mcp::host::{self, RemaMcpStatus},
    state::AppState,
};

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
