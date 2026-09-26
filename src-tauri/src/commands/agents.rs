use tauri::State;

use crate::{
    error::AppResult,
    models::agent::{Agent, AgentInput},
    services::agents,
    state::AppState,
};

/// Built-in agents first, then the user's own.
#[tauri::command]
#[specta::specta]
pub async fn list_agents(state: State<'_, AppState>) -> AppResult<Vec<Agent>> {
    agents::list(&state)
}

/// Creates (`id` = null) or updates a custom agent.
#[tauri::command]
#[specta::specta]
pub async fn save_agent(
    state: State<'_, AppState>,
    id: Option<i64>,
    input: AgentInput,
) -> AppResult<Agent> {
    agents::save(&state, id, input)
}

/// Copies any agent (built-in or custom) into a new custom agent.
#[tauri::command]
#[specta::specta]
pub async fn duplicate_agent(state: State<'_, AppState>, agent_id: String) -> AppResult<Agent> {
    agents::duplicate(&state, &agent_id)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_agent(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    agents::delete(&state, id)
}
