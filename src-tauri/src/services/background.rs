//! Explicit background execution options. Both are off by default:
//!
//! - **Run ReMa in background**: closing the window keeps ReMa running in
//!   the system tray, so mail sync continues. Quit from the tray menu.
//! - **Start ReMa at login**: the operating system starts ReMa (in the
//!   background) when the user logs in.
//!
//! No system service is installed; once ReMa quits, nothing runs.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    db::providers as settings_repo, error::AppResult, models::connectors::BackgroundSettings,
    state::AppState,
};

const KEY_RUN: &str = "app.run_in_background";
const KEY_LOGIN: &str = "app.start_at_login";

/// Set at startup once the system tray icon exists.
static TRAY_AVAILABLE: AtomicBool = AtomicBool::new(false);

pub fn set_tray_available(available: bool) {
    TRAY_AVAILABLE.store(available, Ordering::Relaxed);
}

pub fn tray_available() -> bool {
    TRAY_AVAILABLE.load(Ordering::Relaxed)
}

fn flag(state: &AppState, key: &str) -> AppResult<bool> {
    Ok(state
        .db
        .call(|c| settings_repo::get_setting(c, key))?
        .is_some_and(|v| v == "1"))
}

pub fn settings(state: &AppState) -> AppResult<BackgroundSettings> {
    Ok(BackgroundSettings {
        run_in_background: flag(state, KEY_RUN)?,
        start_at_login: flag(state, KEY_LOGIN)?,
        tray_available: tray_available(),
    })
}

/// Whether closing the window should keep ReMa running.
pub fn keeps_running(state: &AppState) -> bool {
    tray_available() && flag(state, KEY_RUN).unwrap_or(false)
}

pub fn save(state: &AppState, run_in_background: bool, start_at_login: bool) -> AppResult<()> {
    state.db.call(|c| {
        settings_repo::set_setting(c, KEY_RUN, if run_in_background { "1" } else { "0" })?;
        settings_repo::set_setting(c, KEY_LOGIN, if start_at_login { "1" } else { "0" })
    })?;
    state.events.connectors_changed();
    Ok(())
}
