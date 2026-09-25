//! Backend → frontend notifications.
//!
//! Services report changes through [`EventSink`]; the Tauri implementation
//! emits typed events (see `ipc.rs`), tests record them.

use std::sync::Mutex;

use tauri::AppHandle;
use tauri_specta::Event;

use crate::models::{
    chat::{ChatEvent, ConversationsChanged},
    provider::ProvidersChanged,
    task::TasksChanged,
};

pub trait EventSink: Send + Sync {
    fn chat(&self, event: ChatEvent);
    fn conversations_changed(&self);
    fn providers_changed(&self);
    fn tasks_changed(&self);
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
}

/// Collects events for assertions in tests.
#[derive(Default)]
pub struct RecordingEvents {
    pub chat: Mutex<Vec<ChatEvent>>,
    pub conversations: Mutex<usize>,
    pub providers: Mutex<usize>,
    pub tasks: Mutex<usize>,
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
}
