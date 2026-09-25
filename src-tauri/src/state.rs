use std::sync::Arc;

use tauri::PackageInfo;

use crate::{
    db::Database,
    events::EventSink,
    llm::LanguageModel,
    secrets::SecretVault,
    services::{chat::Generations, scheduler::SchedulerHandle},
};

#[derive(Debug, Clone)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

impl AppInfo {
    pub fn from_package(package: &PackageInfo) -> Self {
        Self {
            name: package.name.clone(),
            version: package.version.to_string(),
        }
    }
}

/// Shared application state, created once at startup and managed by Tauri.
///
/// Every field is a cheap handle, so the state can be cloned into background
/// work (chat generations, scheduled runs). Commands receive it via
/// `tauri::State<'_, AppState>` and pass it to services.
#[derive(Clone)]
pub struct AppState {
    pub info: Arc<AppInfo>,
    pub db: Database,
    pub vault: SecretVault,
    pub llm: Arc<dyn LanguageModel>,
    pub events: Arc<dyn EventSink>,
    /// Chat responses currently streaming.
    pub generations: Generations,
    pub scheduler: SchedulerHandle,
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use crate::{events::RecordingEvents, secrets::MemoryStore};

    /// State with an in-memory database, in-memory secrets and a fake model.
    pub fn state(llm: Arc<dyn LanguageModel>) -> (AppState, Arc<RecordingEvents>) {
        let events = Arc::new(RecordingEvents::default());
        let state = AppState {
            info: Arc::new(AppInfo {
                name: "ReMa".into(),
                version: "0.1.0".into(),
            }),
            db: Database::open_in_memory().unwrap(),
            vault: SecretVault::new(Arc::new(MemoryStore::default())),
            llm,
            events: events.clone(),
            generations: Generations::default(),
            scheduler: SchedulerHandle::default(),
        };
        (state, events)
    }
}
