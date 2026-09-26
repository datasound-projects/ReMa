use std::path::PathBuf;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::{
    error::{AppError, AppResult},
    models::profile::{
        AddDocumentsResult, CredentialInput, DocumentBlock, DocumentKind, Profile,
        ProfileCredential, ProfileDocument, ProfileImport, ProfileView,
    },
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
        Some(DocumentKind::Certificate) => "Choose the credential file",
        _ => "Add a document to your profile",
    };
    let Some(path) = pick_file(&app, title, kind) else {
        return Ok(None);
    };
    profile::add_document(&state, &path, kind).await.map(Some)
}

fn file_dialog(
    app: &AppHandle,
    title: &str,
    kind: Option<DocumentKind>,
) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    app.dialog()
        .file()
        .set_title(title)
        .add_filter("Documents", &profile::picker_extensions(kind))
}

fn pick_file(app: &AppHandle, title: &str, kind: Option<DocumentKind>) -> Option<PathBuf> {
    file_dialog(app, title, kind)
        .blocking_pick_file()
        .and_then(|picked| picked.into_path().ok())
}

/// Lets the user pick one or more files and stores each. The user stays
/// where they are: nothing is extracted into the Custom Profile.
#[tauri::command]
#[specta::specta]
pub async fn add_profile_documents(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: Option<DocumentKind>,
) -> AppResult<AddDocumentsResult> {
    let title = match kind {
        Some(DocumentKind::Cv) => "Choose your CV files",
        _ => "Add documents to your profile",
    };
    let paths: Vec<PathBuf> = file_dialog(&app, title, kind)
        .blocking_pick_files()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|picked| picked.into_path().ok())
        .collect();
    Ok(profile::add_documents(&state, &paths, kind).await)
}

/// Replaces a document with a new file picked by the user. `None` if the
/// user cancelled.
#[tauri::command]
#[specta::specta]
pub async fn replace_profile_document(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<Option<ProfileDocument>> {
    let current = profile::get(&state)?
        .documents
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| AppError::not_found("Document not found"))?;
    let title = format!("Replace “{}”", current.name);
    let Some(path) = pick_file(&app, &title, Some(current.kind)) else {
        return Ok(None);
    };
    profile::replace_document(&state, id, &path).await.map(Some)
}

/// Marks a CV as the primary one.
#[tauri::command]
#[specta::specta]
pub async fn set_primary_document(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<ProfileDocument> {
    profile::set_primary_document(&state, id)
}

/// A Word or text document as plain blocks for the in-app viewer.
#[tauri::command]
#[specta::specta]
pub async fn profile_document_blocks(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<Vec<DocumentBlock>> {
    profile::document_blocks(&state, id).await
}

/// Creates (`id` = null) or updates a credential; `documentId` is its file
/// (added with `add_profile_document`).
#[tauri::command]
#[specta::specta]
pub async fn save_credential(
    state: State<'_, AppState>,
    id: Option<i64>,
    input: CredentialInput,
    document_id: Option<i64>,
) -> AppResult<ProfileCredential> {
    profile::save_credential(&state, id, input, document_id)
}

/// Deletes a credential and its file.
#[tauri::command]
#[specta::specta]
pub async fn delete_credential(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    profile::delete_credential(&state, id)
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
