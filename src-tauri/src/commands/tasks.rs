use tauri::State;

use crate::{
    error::AppResult,
    models::task::{ScheduledTask, TaskExecution, TaskInput},
    services::tasks,
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn list_tasks(state: State<'_, AppState>) -> AppResult<Vec<ScheduledTask>> {
    tasks::list(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn create_task(state: State<'_, AppState>, input: TaskInput) -> AppResult<ScheduledTask> {
    tasks::create(&state, input)
}

#[tauri::command]
#[specta::specta]
pub async fn update_task(
    state: State<'_, AppState>,
    id: i64,
    input: TaskInput,
) -> AppResult<ScheduledTask> {
    tasks::update(&state, id, input)
}

#[tauri::command]
#[specta::specta]
pub async fn set_task_enabled(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> AppResult<ScheduledTask> {
    tasks::set_enabled(&state, id, enabled)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_task(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    tasks::delete(&state, id)
}

#[tauri::command]
#[specta::specta]
pub async fn run_task_now(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    tasks::run_now(&state, id)
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_executions(
    state: State<'_, AppState>,
    task_id: i64,
) -> AppResult<Vec<TaskExecution>> {
    tasks::executions(&state, task_id)
}
