use tauri::State;

use crate::{error::AppResult, models::system::AppStatus, services, state::AppState};

#[tauri::command]
pub async fn get_app_status(state: State<'_, AppState>) -> AppResult<AppStatus> {
    Ok(services::system::app_status(&state))
}
