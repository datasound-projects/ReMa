use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    error::AppResult,
    models::portfolio::{PageSize, PortfolioDocument, PortfolioInput, PortfolioStart},
    services::portfolio,
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn list_portfolios(state: State<'_, AppState>) -> AppResult<Vec<PortfolioDocument>> {
    portfolio::list(&state)
}

/// A new document, blank or with a one-time copy of the Custom Profile.
#[tauri::command]
#[specta::specta]
pub async fn create_portfolio(
    state: State<'_, AppState>,
    name: String,
    template_id: String,
    page_size: PageSize,
    start: PortfolioStart,
) -> AppResult<PortfolioDocument> {
    portfolio::create(&state, &name, &template_id, page_size, start)
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
