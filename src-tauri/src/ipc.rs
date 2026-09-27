//! IPC surface: the list of commands exposed to the frontend and the
//! generator for their TypeScript bindings (`src/generated/bindings.ts`).
//!
//! Rust is the source of truth. After changing a command or a model, the
//! bindings are regenerated automatically by `pnpm tauri dev`, or explicitly
//! with `pnpm bindings`. `cargo test` fails if the committed file is stale.

use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

use specta_typescript::Typescript;
use tauri_specta::{collect_commands, collect_events, Builder, ErrorHandlingMode};

use crate::{
    business::model::{BusinessChanged, BusinessProgress},
    commands,
    models::{
        agent::AgentsChanged,
        analytics::AnalyticsChanged,
        browser::BrowserChanged,
        chat::{ChatEvent, ConversationsChanged},
        connectors::ConnectorsChanged,
        jobs::{ApplicationsChanged, NotificationsChanged},
        mcp::McpChanged,
        portfolio::PortfolioChanged,
        profile::ProfileChanged,
        provider::ProvidersChanged,
        task::{TaskRunChanged, TasksChanged},
    },
    network::model::NetworkProgress,
};

/// Path of the generated bindings, relative to this crate.
pub const BINDINGS_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../src/generated/bindings.ts");

const HEADER: &str = "// Source of truth: the Rust commands and models in src-tauri.\n// Regenerate with `pnpm bindings` (or run `pnpm tauri dev`).";

/// Every command and event the frontend may use. Register new ones here.
pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            // System
            commands::system::get_app_status,
            commands::system::get_system_timezone,
            commands::system::open_external_url,
            commands::system::set_appearance,
            // Providers & models
            commands::providers::get_provider_settings,
            commands::providers::get_model_catalog,
            commands::providers::connect_provider,
            commands::providers::disconnect_provider,
            commands::providers::save_custom_provider,
            commands::providers::refresh_provider_models,
            commands::providers::set_model_enabled,
            commands::providers::set_default_model,
            commands::providers::start_provider_sign_in,
            commands::providers::cancel_provider_sign_in,
            commands::providers::check_provider_connection,
            commands::providers::sign_out_provider,
            // Connectors (Gmail, Google Calendar, Outlook Mail, Outlook Calendar)
            commands::connectors::get_connectors,
            commands::connectors::connect_connector,
            commands::connectors::cancel_connector_sign_in,
            commands::connectors::disconnect_connector,
            commands::connectors::set_background_settings,
            // Network Connect
            commands::network::network_capabilities,
            commands::network::network_research,
            commands::network::network_cancel,
            commands::network::network_last_result,
            // Business
            commands::business::business_overview,
            commands::business::business_save_profile,
            commands::business::business_create_offer,
            commands::business::business_save_offer_draft,
            commands::business::business_review_offer,
            commands::business::business_archive_offer,
            commands::business::business_delete_offer,
            commands::business::business_offer_version,
            commands::business::business_describe_offer,
            commands::business::business_find_clients,
            commands::business::business_find_contracts,
            commands::business::business_cancel,
            commands::business::business_last_results,
            commands::business::business_runs,
            commands::business::business_pipeline,
            commands::business::business_save_prospect,
            commands::business::business_save_contract,
            commands::business::business_create_opportunity,
            commands::business::business_edit_opportunity,
            commands::business::business_change_stage,
            commands::business::business_record_activity,
            commands::business::business_delete_activity,
            commands::business::business_do_not_contact,
            commands::business::business_suppress_contact,
            commands::business::business_lift_suppression,
            commands::business::business_delete_contact,
            commands::business::business_delete_opportunity,
            commands::business::business_refresh_listing,
            commands::business::business_reassess,
            commands::business::business_redactions,
            commands::business::business_create_plan,
            commands::business::business_save_plan,
            commands::business::business_delete_plan,
            commands::business::business_research_plan,
            commands::business::business_target_accounts,
            commands::business::business_positioning,
            commands::business::business_create_draft,
            commands::business::business_update_draft,
            commands::business::business_delete_draft,
            commands::business::business_create_experiment,
            commands::business::business_update_experiment,
            commands::business::business_set_experiment_status,
            commands::business::business_amend_experiment,
            commands::business::business_delete_experiment,
            commands::business::business_experiment_metrics,
            // Applications tracker & notifications
            commands::applications::get_applications,
            commands::applications::get_calendar,
            commands::applications::get_application,
            commands::applications::set_application_status,
            commands::applications::add_interview_to_calendar,
            commands::applications::decline_interview_calendar,
            commands::applications::list_notifications,
            commands::applications::mark_notifications_read,
            commands::applications::clear_notifications,
            // Profile
            commands::profile::get_profile,
            commands::profile::save_profile,
            commands::profile::add_profile_document,
            commands::profile::import_profile_document,
            commands::profile::update_profile_document,
            commands::profile::delete_profile_document,
            commands::profile::open_profile_document,
            commands::profile::add_profile_documents,
            commands::profile::replace_profile_document,
            commands::profile::set_primary_document,
            commands::profile::profile_document_blocks,
            commands::profile::save_credential,
            commands::profile::delete_credential,
            // Portfolio Studio
            commands::portfolio::list_portfolios,
            commands::portfolio::create_portfolio,
            commands::portfolio::save_portfolio,
            commands::portfolio::duplicate_portfolio,
            commands::portfolio::delete_portfolio,
            commands::portfolio::export_portfolio_pdf,
            // Agents
            commands::agents::list_agents,
            commands::agents::save_agent,
            commands::agents::duplicate_agent,
            commands::agents::delete_agent,
            // Web search service
            commands::websearch::career_search_status,
            commands::websearch::check_career_search,
            commands::websearch::web_search_settings,
            commands::websearch::save_web_search_settings,
            commands::websearch::test_web_search,
            commands::rema_mcp::rema_mcp_status,
            commands::rema_mcp::set_rema_mcp_enabled,
            commands::rema_mcp::clear_rema_mcp_cache,
            // MCP servers
            commands::mcp::list_mcp_servers,
            commands::mcp::save_mcp_server,
            commands::mcp::delete_mcp_server,
            commands::mcp::set_mcp_server_enabled,
            commands::mcp::connect_mcp_server,
            commands::mcp::disconnect_mcp_server,
            commands::mcp::test_mcp_server,
            commands::mcp::sign_in_mcp_server,
            commands::mcp::cancel_mcp_sign_in,
            commands::mcp::sign_out_mcp_server,
            // Browser workspace & Auto Fill
            commands::browser::get_browser_status,
            commands::browser::open_in_browser,
            commands::browser::set_browser_bounds,
            commands::browser::set_browser_visible,
            commands::browser::browser_back,
            commands::browser::browser_forward,
            commands::browser::browser_reload,
            commands::browser::close_browser,
            commands::browser::run_autofill,
            commands::browser::attach_profile_document,
            commands::browser::reveal_profile_document,
            // Chat
            commands::chat::list_conversations,
            commands::chat::get_conversation,
            commands::chat::send_message,
            commands::chat::retry_message,
            commands::chat::stop_generation,
            commands::chat::delete_conversation,
            commands::chat::set_conversation_selections,
            commands::chat::respond_tool_approval,
            // Scheduled tasks
            commands::tasks::list_tasks,
            commands::tasks::create_task,
            commands::tasks::update_task,
            commands::tasks::set_task_enabled,
            commands::tasks::delete_task,
            commands::tasks::run_task_now,
            commands::tasks::list_task_runs,
            commands::tasks::get_task_run,
            commands::tasks::cancel_task_run,
            // Job analytics
            commands::analytics::get_analytics_preferences,
            commands::analytics::save_analytics_preferences,
            commands::analytics::list_job_search_runs,
            commands::analytics::delete_job_search_run,
            commands::analytics::get_analytics_overview,
            commands::analytics::get_skill_gap,
            commands::analytics::get_requirements_analysis,
            commands::analytics::get_learning,
            commands::analytics::research_learning,
            commands::analytics::analyze_answer,
            commands::analytics::analyze_task_result,
            commands::analytics::list_conversation_job_runs,
            commands::analytics::list_task_job_runs,
        ])
        .events(collect_events![
            AgentsChanged,
            AnalyticsChanged,
            ApplicationsChanged,
            BrowserChanged,
            BusinessChanged,
            BusinessProgress,
            ChatEvent,
            ConnectorsChanged,
            ConversationsChanged,
            McpChanged,
            NetworkProgress,
            NotificationsChanged,
            PortfolioChanged,
            ProfileChanged,
            ProvidersChanged,
            TaskRunChanged,
            TasksChanged,
        ])
        // Ids and epoch-millisecond timestamps are far below 2^53.
        .dangerously_cast_bigints_to_number()
        // Failed commands reject the promise; `src/services/ipc.ts` turns the
        // rejection into an `ApiError`.
        .error_handling(ErrorHandlingMode::Throw)
}

/// Renders the TypeScript bindings for the given builder.
pub fn render_bindings(builder: &Builder<tauri::Wry>) -> Result<String, String> {
    // Unique per call: tests may render concurrently.
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rema-bindings-{}-{id}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("bindings.ts");
    builder
        .export(Typescript::default().header(HEADER), &file)
        .map_err(|e| e.to_string())?;
    let rendered = fs::read_to_string(&file).map_err(|e| e.to_string());
    let _ = fs::remove_dir_all(&dir);
    rendered
}

/// Writes the bindings to [`BINDINGS_PATH`], only if their content changed
/// (so the Vite dev server doesn't reload needlessly).
pub fn export_bindings(builder: &Builder<tauri::Wry>) -> Result<(), String> {
    let rendered = render_bindings(builder)?;
    let path = Path::new(BINDINGS_PATH);
    if fs::read_to_string(path).ok().as_deref() == Some(rendered.as_str()) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, rendered).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_bindings_are_up_to_date() {
        let rendered = render_bindings(&builder()).expect("render bindings");
        let committed = fs::read_to_string(BINDINGS_PATH).unwrap_or_default();

        assert!(
            committed == rendered,
            "src/generated/bindings.ts is out of date. Run `pnpm bindings` and commit the result."
        );
    }

    /// Every registered command is listed in `APP_COMMANDS`, which build.rs
    /// turns into the permission set granted to the main webview only. A
    /// command missing there would be rejected at runtime.
    #[test]
    fn every_command_is_permissioned() {
        let rendered = render_bindings(&builder()).expect("render bindings");
        let mut registered: Vec<&str> = rendered
            .split("__TAURI_INVOKE<")
            .skip(1)
            .filter_map(|s| s.split("(\"").nth(1)?.split('"').next())
            .collect();
        let mut listed = crate::ipc_commands::APP_COMMANDS.to_vec();
        registered.sort_unstable();
        listed.sort_unstable();
        assert_eq!(registered, listed);
    }

    /// Run via `pnpm bindings`.
    #[test]
    #[ignore = "writes src/generated/bindings.ts; run explicitly with `pnpm bindings`"]
    fn export() {
        export_bindings(&builder()).expect("export bindings");
    }
}
