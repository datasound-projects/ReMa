//! ReMa application core.
//!
//! Layering (each layer only talks to the one below it):
//!
//! ```text
//! commands  — thin Tauri IPC adapters: extract state/args, call a service
//! services  — deterministic business logic
//! models    — typed data shared across layers and serialized to the UI
//! state     — shared application state managed by Tauri
//! error     — the single error type returned across the IPC boundary
//! ```

pub mod commands;
pub mod error;
pub mod models;
pub mod services;
pub mod state;

use tauri::Manager;

use crate::state::AppState;

/// Builds and runs the desktop application.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = AppState::new(app.package_info());
            app.manage(state);
            Ok(())
        })
        // Every command exposed to the frontend must be registered here.
        .invoke_handler(tauri::generate_handler![commands::system::get_app_status])
        .run(tauri::generate_context!())
        .expect("failed to start ReMa");
}
