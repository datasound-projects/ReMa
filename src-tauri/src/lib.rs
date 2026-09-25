//! ReMa application core.
//!
//! Layering (each layer only talks to the one below it):
//!
//! ```text
//! ipc       — the command list and generated TypeScript bindings
//! commands  — thin Tauri IPC adapters: extract state/args, call a service
//! services  — deterministic business logic (chat, providers, tasks, scheduler)
//! llm       — provider adapters behind one `LanguageModel` interface
//! db        — SQLite persistence and migrations
//! secrets   — credentials in the OS credential store
//! models    — typed data shared across layers and serialized to the UI
//! state     — shared application state managed by Tauri
//! error     — the single error type returned across the IPC boundary
//! ```

pub mod commands;
pub mod db;
pub mod error;
pub mod events;
pub mod ipc;
pub mod llm;
pub mod models;
pub mod secrets;
pub mod services;
pub mod state;
pub mod time;

use std::sync::Arc;

use tauri::{App, Manager, RunEvent};

use crate::{
    db::Database,
    events::TauriEvents,
    llm::HttpLanguageModel,
    secrets::{KeyringStore, SecretVault},
    services::{chat::Generations, scheduler, scheduler::SchedulerHandle},
    state::{AppInfo, AppState},
};

/// Builds and runs the desktop application.
pub fn run() {
    let ipc = ipc::builder();

    // Keep the frontend bindings in sync while developing.
    #[cfg(debug_assertions)]
    if let Err(error) = ipc::export_bindings(&ipc) {
        eprintln!("warning: failed to export TypeScript bindings: {error}");
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(ipc.invoke_handler())
        .setup(move |app| {
            ipc.mount_events(app);
            let state = init_state(app)?;
            scheduler::start(state.clone());
            app.manage(state);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to start ReMa");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<AppState>() {
                state.scheduler.shutdown();
                state.generations.cancel_all();
            }
        }
    });
}

fn init_state(app: &App) -> Result<AppState, Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db = Database::open(&data_dir.join("rema.db"))?;

    // Work interrupted by the last shutdown is marked as such, never resumed.
    let now = time::now_ms();
    db.call(|conn| {
        db::conversations::mark_interrupted(conn)?;
        db::tasks::mark_interrupted_executions(conn, now)?;
        Ok(())
    })?;

    Ok(AppState {
        info: Arc::new(AppInfo::from_package(app.package_info())),
        db,
        vault: SecretVault::new(Arc::new(KeyringStore)),
        llm: Arc::new(HttpLanguageModel::new()),
        events: Arc::new(TauriEvents(app.handle().clone())),
        generations: Generations::default(),
        scheduler: SchedulerHandle::default(),
    })
}
