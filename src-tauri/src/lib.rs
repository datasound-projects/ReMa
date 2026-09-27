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
//! connectors — Gmail, Google Calendar, Outlook Mail, Outlook Calendar
//!              (OAuth, tokens, provider APIs, incremental sync)
//! jobs      — job-application intelligence (mail → tracker → calendar)
//! secrets   — credentials in the OS credential store
//! models    — typed data shared across layers and serialized to the UI
//! state     — shared application state managed by Tauri
//! error     — the single error type returned across the IPC boundary
//! ```

pub mod accounts;
pub mod analytics;
pub mod browser;
pub mod business;
pub mod career_search;
pub mod commands;
pub mod connectors;
pub mod db;
pub mod error;
pub mod events;
pub mod ipc;
pub mod ipc_commands;
pub mod jobs;
pub mod llm;
pub mod mcp;
pub mod models;
pub mod network;
pub mod oauth_loopback;
pub mod protocol;
pub mod rema_mcp;
pub mod retrieval;
pub mod secrets;
pub mod services;
pub mod state;
#[cfg(test)]
mod test_support;
pub mod time;

use std::sync::Arc;

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    App, AppHandle, Manager, RunEvent, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;

use crate::{
    accounts::{claude_console::ClaudeConsole, codex::CodexRuntime, Accounts},
    connectors::{google::GoogleEndpoints, microsoft::MicrosoftEndpoints, Apps, ConnectorsContext},
    db::Database,
    events::TauriEvents,
    llm::ProviderLanguageModel,
    secrets::{KeyringStore, SecretVault},
    services::{
        background, chat::Generations, mail_monitor, scheduler, scheduler::SchedulerHandle,
    },
    state::{AppInfo, AppState},
};

/// The system tray icon (shown with "Run ReMa in background").
pub const TRAY_ID: &str = "rema";
/// Passed by the operating system when ReMa starts at login.
const BACKGROUND_ARG: &str = "--background";

/// Builds and runs the desktop application.
pub fn run() {
    let ipc = ipc::builder();

    // Keep the frontend bindings in sync while developing.
    #[cfg(debug_assertions)]
    if let Err(error) = ipc::export_bindings(&ipc) {
        eprintln!("warning: failed to export TypeScript bindings: {error}");
    }

    let app = tauri::Builder::default()
        // First: a second launch hands over to this instance and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app)
        }))
        .plugin(tauri_plugin_opener::init())
        // Used from Rust only; no dialog permission is granted to any webview.
        .plugin(tauri_plugin_dialog::init())
        // Used from Rust only (no frontend permission).
        .plugin(tauri_plugin_notification::init())
        // Registers ReMa to start at login only when the user turns it on.
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![BACKGROUND_ARG]),
        ))
        // With "Run ReMa in background", closing the window hides it; ReMa
        // keeps running in the tray until the user quits it there.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let keep = window
                    .try_state::<AppState>()
                    .is_some_and(|state| background::keeps_running(&state));
                if keep && window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
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
            // ReMa MCP is enabled on first launch; a saved choice is kept.
            if let Err(error) = rema_mcp::ensure_default(&state) {
                eprintln!("ReMa MCP setting could not be read: {error}");
            }
            let tray = match create_tray(app) {
                Ok(()) => true,
                Err(error) => {
                    eprintln!("the system tray is not available: {error}");
                    false
                }
            };
            background::set_tray_available(tray);
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_visible(background::keeps_running(&state));
            }
            // The window starts hidden (no flash when started at login) and
            // is shown unless ReMa was started in the background.
            let hidden =
                std::env::args().any(|a| a == BACKGROUND_ARG) && background::keeps_running(&state);
            if !hidden {
                show_main_window(app.handle());
            }
            scheduler::start(state.clone());
            analytics::enrich::start(state.clone());
            mail_monitor::start(state.clone());
            // Career search needs no setup: its parts are checked, an old
            // search-service setting is moved out of the way, and what the
            // connected models can do is learned before the first search.
            let startup = state.clone();
            tauri::async_runtime::spawn(async move {
                career_search::bootstrap::run(&startup).await;
            });
            app.manage(state);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to start ReMa");

    app.run(|handle, event| match event {
        // macOS: clicking the Dock icon reopens the hidden window.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main_window(handle),
        RunEvent::Exit => {
            if let Some(state) = handle.try_state::<AppState>() {
                state.connectors.shutdown();
                state.scheduler.shutdown();
                state.analytics.stop();
                state.generations.cancel_all();
                state.accounts.shutdown();
                state.mcp.shutdown();
            }
        }
        _ => {}
    });
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// The tray menu: Open ReMa, Run Job Mail & Interview Sync (only when the
/// user turned that task on), Quit ReMa. Hidden unless "Run ReMa in
/// background" is on.
fn create_tray(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open ReMa", true, None::<&str>)?;
    let sync = MenuItem::with_id(
        app,
        "sync",
        "Run Job Mail & Interview Sync",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit ReMa", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &sync, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("ReMa")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "sync" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = services::tasks::run_job_mail_sync_now(state.inner());
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn init_state(app: &App) -> Result<AppState, Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db = Database::open(&data_dir.join("rema.db"))?;

    // Work interrupted by the last shutdown is marked as such, never resumed.
    let now = time::now_ms();
    db.call(|conn| {
        db::conversations::mark_interrupted(conn)?;
        db::runs::mark_interrupted(conn, now)?;
        db::analytics::mark_interrupted_research(conn)?;
        business::store::reconcile_interrupted(conn, now)?;
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
        connectors: ConnectorsContext::new(
            GoogleEndpoints::from_env(),
            MicrosoftEndpoints::from_env(),
            Apps::from_build(),
        ),
        browser: Default::default(),
        analytics: Default::default(),
        mcp: Default::default(),
        approvals: Default::default(),
        rema_mcp: rema_mcp::RemaMcp::new(),
        career: Default::default(),
        network: Default::default(),
        business: Default::default(),
    })
}
