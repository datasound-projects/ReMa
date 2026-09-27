use tauri::State;

use crate::{
    error::AppResult,
    models::jobs::{ApplicationDetail, ApplicationStatus, ApplicationsOverview, NotificationItem},
    services::{applications, notifications},
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn get_applications(state: State<'_, AppState>) -> AppResult<ApplicationsOverview> {
    applications::overview(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn get_application(state: State<'_, AppState>, id: i64) -> AppResult<ApplicationDetail> {
    applications::detail(&state, id)
}

/// A status change by the user (recorded on the timeline).
#[tauri::command]
#[specta::specta]
pub async fn set_application_status(
    state: State<'_, AppState>,
    id: i64,
    status: ApplicationStatus,
    note: Option<String>,
) -> AppResult<ApplicationDetail> {
    applications::set_status(&state, id, status, note)
}

/// Adds a confirmed interview to the connected calendar. A conflict is an
/// error unless `allow_conflict` confirms it.
#[tauri::command]
#[specta::specta]
pub async fn add_interview_to_calendar(
    state: State<'_, AppState>,
    interview_id: i64,
    allow_conflict: bool,
) -> AppResult<ApplicationDetail> {
    applications::add_interview_to_calendar(&state, interview_id, allow_conflict).await
}

#[tauri::command]
#[specta::specta]
pub async fn decline_interview_calendar(
    state: State<'_, AppState>,
    interview_id: i64,
) -> AppResult<ApplicationDetail> {
    applications::decline_interview_calendar(&state, interview_id)
}

#[tauri::command]
#[specta::specta]
pub async fn list_notifications(state: State<'_, AppState>) -> AppResult<Vec<NotificationItem>> {
    notifications::list(&state)
}

/// Marks the given notifications (or all, when `ids` is omitted) as read.
#[tauri::command]
#[specta::specta]
pub async fn mark_notifications_read(
    state: State<'_, AppState>,
    ids: Option<Vec<i64>>,
) -> AppResult<()> {
    notifications::mark_read(&state, ids)
}

#[tauri::command]
#[specta::specta]
pub async fn clear_notifications(state: State<'_, AppState>) -> AppResult<()> {
    notifications::clear(&state)
}
