//! ReMa application core.
//!
//! Layering (each layer only talks to the one below it):
//!
//! ```text
//! ipc       — the command list and generated TypeScript bindings
//! accounts  — provider account sign-in through official local runtimes
//! analytics — the deterministic job analytics engine (ingestion → dashboard)
//! browser   — the isolated built-in browser workspace and Auto Fill
//! commands  — thin Tauri IPC adapters: extract state/args, call a service
//! services  — deterministic business logic (chat, providers, tasks, scheduler)
//! llm       — provider adapters behind one `LanguageModel` interface
//! db        — SQLite persistence and migrations
//! integrations — Google Workspace (OAuth, Gmail, Calendar)
//! jobs      — job-application intelligence (Gmail → state → Calendar)
//! secrets   — credentials in the OS credential store
//! models    — typed data shared across layers and serialized to the UI
//! state     — shared application state managed by Tauri
//! error     — the single error type returned across the IPC boundary
//! ```

pub mod accounts;
pub mod analytics;
pub mod browser;
pub mod commands;
pub mod db;
pub mod error;
pub mod events;
pub mod integrations;
pub mod ipc;
pub mod ipc_commands;
pub mod jobs;
pub mod llm;
pub mod mcp;
pub mod models;
pub mod oauth_loopback;
pub mod protocol;
pub mod secrets;
pub mod services;
pub mod state;
#[cfg(test)]
mod test_support;
pub mod time;

use std::sync::Arc;

use tauri::{App, Manager, RunEvent};

use crate::{
    accounts::{claude_console::ClaudeConsole, codex::CodexRuntime, Accounts},
    db::Database,
    events::TauriEvents,
    integrations::google::{GoogleContext, GoogleEndpoints},
    llm::ProviderLanguageModel,
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
        // Used from Rust only; no dialog permission is granted to any webview.
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(ipc.invoke_handler())
        // Document files for the in-app viewer (main webview only).
        .register_asynchronous_uri_scheme_protocol(protocol::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let webview = ctx.webview_label().to_string();
            let path = request.uri().path().to_string();
            let origin = request
                .headers()
                .get(tauri::http::header::ORIGIN)
                .and_then(|o| o.to_str().ok())
                .map(str::to_string);
            // Reading a file can take a moment; keep it off the main thread.
            tauri::async_runtime::spawn_blocking(move || {
                let state = app.try_state::<AppState>();
                responder.respond(protocol::respond(
                    state.as_deref(),
                    &webview,
                    &path,
                    origin.as_deref(),
                ));
            });
        })
        .setup(move |app| {
            ipc.mount_events(app);
            let state = init_state(app)?;
            // Open in the saved theme (the page itself reads it too).
            if let Ok(appearance) = services::system::appearance(&state.db) {
                commands::system::apply_appearance(app.handle(), appearance);
            }
            scheduler::start(state.clone());
            analytics::enrich::start(state.clone());
            app.manage(state);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to start ReMa");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<AppState>() {
                state.scheduler.shutdown();
                state.analytics.stop();
                state.generations.cancel_all();
                state.accounts.shutdown();
                state.mcp.shutdown();
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
        db::analytics::mark_interrupted_research(conn)?;
        Ok(())
    })?;

    // The providers' official runtimes, each with a ReMa-private home: the
    // user's own Codex and Anthropic CLI settings are neither read nor changed.
    let info = AppInfo::from_package(app.package_info());
    let runtimes = data_dir.join("runtimes");
    let codex = Arc::new(CodexRuntime::new(
        runtimes.join("codex"),
        runtimes.join("codex-workspace"),
        info.version.clone(),
    ));
    let claude_console = Arc::new(ClaudeConsole::new(runtimes.join("anthropic")));

    Ok(AppState {
        info: Arc::new(info),
        data_dir: Arc::new(data_dir),
        db,
        vault: SecretVault::new(Arc::new(KeyringStore)),
        accounts: Accounts::new(codex.clone(), claude_console),
        llm: Arc::new(ProviderLanguageModel::new(Some(codex))),
        events: Arc::new(TauriEvents(app.handle().clone())),
        generations: Generations::default(),
        scheduler: SchedulerHandle::default(),
        google: GoogleContext::new(GoogleEndpoints::from_env()),
        browser: Default::default(),
        analytics: Default::default(),
        mcp: Default::default(),
        approvals: Default::default(),
    })
}
