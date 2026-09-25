//! Backend → frontend notifications.
//!
//! Services report changes through [`EventSink`]; the Tauri implementation
//! emits typed events (see `ipc.rs`), tests record them.

use std::sync::Mutex;

use tauri::AppHandle;
use tauri_specta::Event;

use crate::models::{
    analytics::AnalyticsChanged,
    browser::{BrowserChanged, BrowserStatus},
    chat::{ChatEvent, ConversationsChanged},
    google::GoogleChanged,
    profile::ProfileChanged,
    provider::ProvidersChanged,
    task::TasksChanged,
};

pub trait EventSink: Send + Sync {
    fn chat(&self, event: ChatEvent);
    fn conversations_changed(&self);
    fn providers_changed(&self);
    fn tasks_changed(&self);
    fn google_changed(&self);
    fn profile_changed(&self);
    fn browser_changed(&self, status: BrowserStatus);
    fn analytics_changed(&self);
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

    fn google_changed(&self) {
        let _ = GoogleChanged.emit(&self.0);
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
}

/// Collects events for assertions in tests.
#[derive(Default)]
pub struct RecordingEvents {
    pub chat: Mutex<Vec<ChatEvent>>,
    pub conversations: Mutex<usize>,
    pub providers: Mutex<usize>,
    pub tasks: Mutex<usize>,
    pub google: Mutex<usize>,
    pub profile: Mutex<usize>,
    pub browser: Mutex<Vec<BrowserStatus>>,
    pub analytics: Mutex<usize>,
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

    fn google_changed(&self) {
        *self.google.lock().unwrap() += 1;
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
}
