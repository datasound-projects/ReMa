use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    error::{AppError, AppResult},
    models::system::{AppStatus, Appearance},
    services::{schedule, system},
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn get_app_status(state: State<'_, AppState>) -> AppResult<AppStatus> {
    Ok(system::app_status(&state.info))
}

/// IANA name of the computer's timezone, the default for new tasks.
#[tauri::command]
#[specta::specta]
pub async fn get_system_timezone() -> AppResult<String> {
    Ok(schedule::system_timezone_name())
}

/// Opens a link from chat content in the default browser.
#[tauri::command]
#[specta::specta]
pub async fn open_external_url(app: AppHandle, url: String) -> AppResult<()> {
    if !system::is_safe_external_url(&url) {
        return Err(AppError::validation(
            "Only web and email links can be opened.",
        ));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| AppError::internal(format!("could not open the link: {e}")))
}

/// Remembers the color theme and applies it to the window: native menus and
/// pickers follow it, and the next launch opens in it without a flash.
#[tauri::command]
#[specta::specta]
pub async fn set_appearance(
    app: AppHandle,
    state: State<'_, AppState>,
    appearance: Appearance,
) -> AppResult<()> {
    system::save_appearance(&state.db, appearance)?;
    apply_appearance(&app, appearance);
    Ok(())
}

/// Sets the main window's theme and canvas color (best effort: not every
/// platform supports both).
pub fn apply_appearance(app: &AppHandle, appearance: Appearance) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let theme = match appearance {
        Appearance::Light => tauri::Theme::Light,
        Appearance::Dark => tauri::Theme::Dark,
    };
    let _ = window.set_theme(Some(theme));
    let (r, g, b) = appearance.window_rgb();
    let _ = window.set_background_color(Some(tauri::window::Color(r, g, b, 255)));
}
