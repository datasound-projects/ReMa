use tauri::State;

use crate::{
    analytics::{self, ingest},
    db::analytics as repo,
    error::AppResult,
    models::analytics::{
        AnalyticsOverview, AnalyticsPreferences, AnalyticsQuery, JobSearchRun, LearningCriteria,
        LearningView, LinkedRun, RequirementTableQuery, RequirementsView, SkillGapOptions,
        SkillGapView,
    },
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn get_analytics_preferences(
    state: State<'_, AppState>,
) -> AppResult<AnalyticsPreferences> {
    analytics::preferences(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn save_analytics_preferences(
    state: State<'_, AppState>,
    preferences: AnalyticsPreferences,
) -> AppResult<()> {
    analytics::save_preferences(&state, preferences)
}

#[tauri::command]
#[specta::specta]
pub async fn list_job_search_runs(state: State<'_, AppState>) -> AppResult<Vec<JobSearchRun>> {
    analytics::runs(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_job_search_run(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    analytics::delete_run(&state, id)
}

#[tauri::command]
#[specta::specta]
pub async fn get_analytics_overview(
    state: State<'_, AppState>,
    query: AnalyticsQuery,
) -> AppResult<AnalyticsOverview> {
    analytics::overview(&state, &query)
}

#[tauri::command]
#[specta::specta]
pub async fn get_skill_gap(
    state: State<'_, AppState>,
    query: AnalyticsQuery,
    options: SkillGapOptions,
) -> AppResult<SkillGapView> {
    analytics::skill_gap(&state, &query, &options)
}

#[tauri::command]
#[specta::specta]
pub async fn get_requirements_analysis(
    state: State<'_, AppState>,
    query: AnalyticsQuery,
    table: RequirementTableQuery,
) -> AppResult<RequirementsView> {
    analytics::requirements(&state, &query, &table)
}

#[tauri::command]
#[specta::specta]
pub async fn get_learning(
    state: State<'_, AppState>,
    query: AnalyticsQuery,
    criteria: LearningCriteria,
    options: SkillGapOptions,
) -> AppResult<LearningView> {
    analytics::learning(&state, &query, &criteria, &options)
}

/// Starts learning-resource research in the background.
#[tauri::command]
#[specta::specta]
pub async fn research_learning(
    state: State<'_, AppState>,
    query: AnalyticsQuery,
    criteria: LearningCriteria,
    options: SkillGapOptions,
) -> AppResult<LearningView> {
    analytics::research(&state, &query, &criteria, &options)
}

/// Makes a chat answer's job listings available to Analytics (reading a
/// list written as text with the model if needed). Returns the search run.
#[tauri::command]
#[specta::specta]
pub async fn analyze_answer(state: State<'_, AppState>, message_id: i64) -> AppResult<i64> {
    ingest::analyze_answer(&state, message_id).await
}

/// Same for a scheduled task's result.
#[tauri::command]
#[specta::specta]
pub async fn analyze_task_result(state: State<'_, AppState>, execution_id: i64) -> AppResult<i64> {
    ingest::analyze_task_result(&state, execution_id).await
}

/// Search runs created from a conversation's answers.
#[tauri::command]
#[specta::specta]
pub async fn list_conversation_job_runs(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> AppResult<Vec<LinkedRun>> {
    let rows = state
        .db
        .call(|c| repo::runs_for_conversation(c, conversation_id))?;
    Ok(rows
        .into_iter()
        .map(|(source_id, run_id, jobs)| LinkedRun {
            source_id,
            run_id,
            jobs,
        })
        .collect())
}

/// Search runs created from a task's runs.
#[tauri::command]
#[specta::specta]
pub async fn list_task_job_runs(
    state: State<'_, AppState>,
    task_id: i64,
) -> AppResult<Vec<LinkedRun>> {
    let rows = state.db.call(|c| repo::runs_for_task(c, task_id))?;
    Ok(rows
        .into_iter()
        .map(|(source_id, run_id, jobs)| LinkedRun {
            source_id,
            run_id,
            jobs,
        })
        .collect())
}
