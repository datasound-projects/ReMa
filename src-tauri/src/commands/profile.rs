use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::{
    error::{AppError, AppResult},
    models::profile::{DocumentKind, Profile, ProfileDocument, ProfileImport, ProfileView},
    services::{profile, profile_import},
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn get_profile(state: State<'_, AppState>) -> AppResult<ProfileView> {
    profile::get(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn save_profile(state: State<'_, AppState>, profile: Profile) -> AppResult<ProfileView> {
    profile::save(&state, profile)
}

/// Lets the user pick a file with the system file dialog, then stores it.
/// The dialog runs in Rust, so the frontend never handles file paths.
/// Returns `None` if the user cancelled.
#[tauri::command]
#[specta::specta]
pub async fn add_profile_document(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: Option<DocumentKind>,
) -> AppResult<Option<ProfileDocument>> {
    let title = match kind {
        Some(DocumentKind::Cv) => "Choose your CV",
        _ => "Add a document to your profile",
    };
    let picked = app
        .dialog()
        .file()
        .set_title(title)
        .add_filter(
            "Documents",
            &["pdf", "docx", "txt", "md", "markdown", "png", "jpg", "jpeg"],
        )
        .blocking_pick_file();
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|_| AppError::validation("That file cannot be used."))?;
    profile::add_document(&state, &path, kind).await.map(Some)
}

/// Reads profile details from a stored document, for the user to review.
#[tauri::command]
#[specta::specta]
pub async fn import_profile_document(
    state: State<'_, AppState>,
    document_id: i64,
) -> AppResult<ProfileImport> {
    profile_import::import(&state, document_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_profile_document(
    state: State<'_, AppState>,
    id: i64,
    name: String,
    kind: DocumentKind,
) -> AppResult<ProfileDocument> {
    profile::update_document(&state, id, &name, kind)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_profile_document(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    profile::delete_document(&state, id)
}

/// Opens the stored file in the system's default app.
#[tauri::command]
#[specta::specta]
pub async fn open_profile_document(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<()> {
    let (_, path) = profile::document_path(&state, id)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::internal(format!("could not open the document: {e}")))
}
