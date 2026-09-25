//! ReMa application core.
//!
//! Layering (each layer only talks to the one below it):
//!
//! ```text
//! ipc       — the command list and generated TypeScript bindings
//! commands  — thin Tauri IPC adapters: extract state/args, call a service
//! services  — deterministic business logic
//! models    — typed data shared across layers and serialized to the UI
//! state     — shared application state managed by Tauri
//! error     — the single error type returned across the IPC boundary
//! ```

pub mod commands;
pub mod error;
pub mod ipc;
pub mod models;
pub mod services;
pub mod state;

use tauri::Manager;

use crate::state::AppState;

/// Builds and runs the desktop application.
pub fn run() {
    let ipc = ipc::builder();

    // Keep the frontend bindings in sync while developing.
    #[cfg(debug_assertions)]
    if let Err(error) = ipc::export_bindings(&ipc) {
        eprintln!("warning: failed to export TypeScript bindings: {error}");
    }

    tauri::Builder::default()
        .setup(|app| {
            let state = AppState::new(app.package_info());
            app.manage(state);
            Ok(())
        })
        .invoke_handler(ipc.invoke_handler())
        .run(tauri::generate_context!())
        .expect("failed to start ReMa");
}
