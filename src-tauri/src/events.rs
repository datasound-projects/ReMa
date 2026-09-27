//! Backend → frontend notifications.
//!
//! Services report changes through [`EventSink`]; the Tauri implementation
//! emits typed events (see `ipc.rs`), tests record them.

use std::sync::Mutex;

use tauri::AppHandle;
use tauri_specta::Event;

use crate::models::{
    agent::AgentsChanged,
    analytics::AnalyticsChanged,
    browser::{BrowserChanged, BrowserStatus},
    chat::{ChatEvent, ConversationsChanged},
    connectors::ConnectorsChanged,
    jobs::{ApplicationsChanged, NotificationsChanged},
    mcp::McpChanged,
    portfolio::PortfolioChanged,
    profile::ProfileChanged,
    provider::ProvidersChanged,
    task::{TaskRunChanged, TasksChanged},
};
use crate::network::model::NetworkProgress;

pub trait EventSink: Send + Sync {
    fn chat(&self, event: ChatEvent);
    fn conversations_changed(&self);
    fn providers_changed(&self);
    fn tasks_changed(&self);
    /// A run was created or changed (status, progress, outputs).
    fn task_run_changed(&self, task_id: i64, run_id: i64);
    fn connectors_changed(&self);
    fn applications_changed(&self);
    fn notifications_changed(&self);
    /// Shows an operating-system notification.
    fn notify(&self, title: &str, body: &str);
    fn profile_changed(&self);
    fn browser_changed(&self, status: BrowserStatus);
    fn analytics_changed(&self);
    fn portfolio_changed(&self);
    fn agents_changed(&self);
    fn mcp_changed(&self);
    /// A Network Connect request made progress (a status line).
    fn network_progress(&self, run_id: &str, text: &str);
}

pub struct TauriEvents(pub AppHandle);

impl EventSink for TauriEvents {
    // Emitting only fails if the app is shutting down; nothing to do then.
    fn chat(&self, event: ChatEvent) {
        let _ = event.emit(&self.0);
    }

    fn conversations_changed(&self) {
        let _ = ConversationsChanged.emit(&self.0);
    }

    fn providers_changed(&self) {
        let _ = ProvidersChanged.emit(&self.0);
    }

    fn tasks_changed(&self) {
        let _ = TasksChanged.emit(&self.0);
    }

    fn task_run_changed(&self, task_id: i64, run_id: i64) {
        let _ = TaskRunChanged { task_id, run_id }.emit(&self.0);
    }

    fn connectors_changed(&self) {
        let _ = ConnectorsChanged.emit(&self.0);
    }

    fn applications_changed(&self) {
        let _ = ApplicationsChanged.emit(&self.0);
    }

    fn notifications_changed(&self) {
        let _ = NotificationsChanged.emit(&self.0);
    }

    fn notify(&self, title: &str, body: &str) {
        use tauri_plugin_notification::NotificationExt;
        let _ = self
            .0
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show();
    }

    fn profile_changed(&self) {
        let _ = ProfileChanged.emit(&self.0);
    }

    fn browser_changed(&self, status: BrowserStatus) {
        let _ = BrowserChanged(status).emit(&self.0);
    }

    fn analytics_changed(&self) {
        let _ = AnalyticsChanged.emit(&self.0);
    }

    fn portfolio_changed(&self) {
        let _ = PortfolioChanged.emit(&self.0);
    }

    fn agents_changed(&self) {
        let _ = AgentsChanged.emit(&self.0);
    }

    fn mcp_changed(&self) {
        let _ = McpChanged.emit(&self.0);
    }

    fn network_progress(&self, run_id: &str, text: &str) {
        let _ = NetworkProgress {
            run_id: run_id.to_string(),
            text: text.to_string(),
        }
        .emit(&self.0);
    }
}

/// Collects events for assertions in tests.
#[derive(Default)]
pub struct RecordingEvents {
    pub chat: Mutex<Vec<ChatEvent>>,
    pub conversations: Mutex<usize>,
    pub providers: Mutex<usize>,
    pub tasks: Mutex<usize>,
    /// (task id, run id) of every run change.
    pub runs: Mutex<Vec<(i64, i64)>>,
    pub connectors: Mutex<usize>,
    pub applications: Mutex<usize>,
    pub notifications: Mutex<usize>,
    /// OS notifications as (title, body).
    pub shown: Mutex<Vec<(String, String)>>,
    pub profile: Mutex<usize>,
    pub browser: Mutex<Vec<BrowserStatus>>,
    pub analytics: Mutex<usize>,
    pub portfolio: Mutex<usize>,
    pub agents: Mutex<usize>,
    pub mcp: Mutex<usize>,
    /// Network Connect status lines as (run id, text).
    pub network: Mutex<Vec<(String, String)>>,
}

impl EventSink for RecordingEvents {
    fn chat(&self, event: ChatEvent) {
        self.chat.lock().unwrap().push(event);
    }

    fn conversations_changed(&self) {
        *self.conversations.lock().unwrap() += 1;
    }

    fn providers_changed(&self) {
        *self.providers.lock().unwrap() += 1;
    }

    fn tasks_changed(&self) {
        *self.tasks.lock().unwrap() += 1;
    }

    fn task_run_changed(&self, task_id: i64, run_id: i64) {
        self.runs.lock().unwrap().push((task_id, run_id));
    }

    fn connectors_changed(&self) {
        *self.connectors.lock().unwrap() += 1;
    }

    fn applications_changed(&self) {
        *self.applications.lock().unwrap() += 1;
    }

    fn notifications_changed(&self) {
        *self.notifications.lock().unwrap() += 1;
    }

    fn notify(&self, title: &str, body: &str) {
        self.shown
            .lock()
            .unwrap()
            .push((title.to_string(), body.to_string()));
    }

    fn profile_changed(&self) {
        *self.profile.lock().unwrap() += 1;
    }

    fn browser_changed(&self, status: BrowserStatus) {
        self.browser.lock().unwrap().push(status);
    }

    fn analytics_changed(&self) {
        *self.analytics.lock().unwrap() += 1;
    }

    fn portfolio_changed(&self) {
        *self.portfolio.lock().unwrap() += 1;
    }

    fn agents_changed(&self) {
        *self.agents.lock().unwrap() += 1;
    }

    fn mcp_changed(&self) {
        *self.mcp.lock().unwrap() += 1;
    }

    fn network_progress(&self, run_id: &str, text: &str) {
        self.network
            .lock()
            .unwrap()
            .push((run_id.to_string(), text.to_string()));
    }
}
