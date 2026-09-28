use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    error::AppResult,
    models::portfolio::{
        AiProposal, AiRequest, PortfolioDocument, PortfolioImport, PortfolioInput, PortfolioKind,
    },
    services::{
        portfolio::{self, PortfolioCreate},
        portfolio_ai, portfolio_import, profile,
    },
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn list_portfolios(state: State<'_, AppState>) -> AppResult<Vec<PortfolioDocument>> {
    portfolio::list(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn get_portfolio(state: State<'_, AppState>, id: i64) -> AppResult<PortfolioDocument> {
    portfolio::get(&state, id)
}

/// A new document: blank, sample, a one-time copy of the Custom Profile, or
/// reviewed content from an import.
#[tauri::command]
#[specta::specta]
pub async fn create_portfolio(
    state: State<'_, AppState>,
    request: PortfolioCreate,
) -> AppResult<PortfolioDocument> {
    portfolio::create(&state, request)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_portfolio(
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> AppResult<PortfolioDocument> {
    portfolio::rename(&state, id, &name)
}

/// Lets the user pick a CV or cover letter file, stores it as a Profile
/// document (the original is never changed) and reads it into editable
/// parts for review. `None` if the user cancelled.
#[tauri::command]
#[specta::specta]
pub async fn import_portfolio_file(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: PortfolioKind,
) -> AppResult<Option<PortfolioImport>> {
    let title = match kind {
        PortfolioKind::Cv => "Choose the CV to import",
        PortfolioKind::CoverLetter => "Choose the cover letter to import",
    };
    let Some(path) = app
        .dialog()
        .file()
        .set_title(title)
        .add_filter("Documents", &profile::picker_extensions(None))
        .blocking_pick_file()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(None);
    };
    portfolio_import::import_file(&state, &path, kind)
        .await
        .map(Some)
}

/// Reads a document already in the Profile (an older CV) for review.
#[tauri::command]
#[specta::specta]
pub async fn import_portfolio_document(
    state: State<'_, AppState>,
    document_id: i64,
    kind: PortfolioKind,
) -> AppResult<PortfolioImport> {
    portfolio_import::import(&state, document_id, kind).await
}

/// One AI assistant request; the answer is a proposal for review.
#[tauri::command]
#[specta::specta]
pub async fn portfolio_ai_assist(
    state: State<'_, AppState>,
    request: AiRequest,
) -> AppResult<AiProposal> {
    portfolio_ai::assist(&state, request).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_portfolio(
    state: State<'_, AppState>,
    id: i64,
    input: PortfolioInput,
) -> AppResult<PortfolioDocument> {
    portfolio::save(&state, id, input)
}

#[tauri::command]
#[specta::specta]
pub async fn duplicate_portfolio(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<PortfolioDocument> {
    portfolio::duplicate(&state, id)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_portfolio(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    portfolio::delete(&state, id)
}

/// Saves the PDF rendered by the interface where the user chooses (system
/// save dialog in Rust; the interface never handles paths). Returns the
/// file name, or `None` if the user cancelled.
#[tauri::command]
#[specta::specta]
pub async fn export_portfolio_pdf(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    pdf_base64: String,
) -> AppResult<Option<String>> {
    let document = portfolio::get(&state, id)?;
    let bytes = portfolio::decode_pdf(&pdf_base64)?;
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Export as PDF")
        .set_file_name(portfolio::export_file_name(&document.name))
        .add_filter("PDF", &["pdf"])
        .blocking_save_file()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(None);
    };
    let path = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        path
    } else {
        path.with_extension("pdf")
    };
    let written = path.clone();
    tokio::task::spawn_blocking(move || portfolio::write_pdf(&written, &bytes))
        .await
        .map_err(|e| crate::error::AppError::internal(e.to_string()))??;
    Ok(path.file_name().map(|n| n.to_string_lossy().into_owned()))
}
