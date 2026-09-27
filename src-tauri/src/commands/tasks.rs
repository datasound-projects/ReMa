use tauri::State;

use crate::{
    error::AppResult,
    models::task::{ScheduledTask, TaskInput, TaskRun, TaskRunPage},
    services::{runs, tasks},
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
    tasks::create(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_task(
    state: State<'_, AppState>,
    id: i64,
    input: TaskInput,
) -> AppResult<ScheduledTask> {
    tasks::update(&state, id, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_task_enabled(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> AppResult<ScheduledTask> {
    tasks::set_enabled_checked(&state, id, enabled).await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_task(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    tasks::delete(&state, id)
}

/// Starts a manual run; returns its id.
#[tauri::command]
#[specta::specta]
pub async fn run_task_now(state: State<'_, AppState>, id: i64) -> AppResult<i64> {
    tasks::run_now(&state, id)
}

/// A task's runs, newest first; `before` is the last run id already shown.
#[tauri::command]
#[specta::specta]
pub async fn list_task_runs(
    state: State<'_, AppState>,
    task_id: i64,
    before: Option<i64>,
) -> AppResult<TaskRunPage> {
    runs::list(&state, task_id, before)
}

#[tauri::command]
#[specta::specta]
pub async fn get_task_run(state: State<'_, AppState>, run_id: i64) -> AppResult<TaskRun> {
    runs::get(&state, run_id)
}

/// Stops a run that is still going.
#[tauri::command]
#[specta::specta]
pub async fn cancel_task_run(state: State<'_, AppState>, run_id: i64) -> AppResult<()> {
    runs::cancel(&state, run_id)
}
