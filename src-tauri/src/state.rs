use std::{path::PathBuf, sync::Arc};

use tauri::PackageInfo;

use crate::{
    accounts::Accounts,
    analytics::AnalyticsContext,
    browser::BrowserContext,
    db::Database,
    events::EventSink,
    integrations::google::GoogleContext,
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
    /// ReMa's application data folder (database, profile documents).
    pub data_dir: Arc<PathBuf>,
    pub db: Database,
    pub vault: SecretVault,
    /// Account sign-in through the providers' official runtimes.
    pub accounts: Accounts,
    pub llm: Arc<dyn LanguageModel>,
    pub events: Arc<dyn EventSink>,
    /// Chat responses currently streaming.
    pub generations: Generations,
    pub scheduler: SchedulerHandle,
    pub google: GoogleContext,
    pub browser: BrowserContext,
    pub analytics: AnalyticsContext,
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use crate::{
        accounts::fake::FakeAccountRuntime, events::RecordingEvents,
        integrations::google::GoogleEndpoints, secrets::MemoryStore,
    };

    /// A fresh, empty folder under the system temp directory.
    pub fn temp_dir() -> PathBuf {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).unwrap();
        let name: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir().join(format!("rema-test-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// State with an in-memory database, in-memory secrets, a fake model and
    /// a temporary data folder.
    pub fn state(llm: Arc<dyn LanguageModel>) -> (AppState, Arc<RecordingEvents>) {
        let events = Arc::new(RecordingEvents::default());
        let state = AppState {
            info: Arc::new(AppInfo {
                name: "ReMa".into(),
                version: "0.1.0".into(),
            }),
            data_dir: Arc::new(temp_dir()),
            db: Database::open_in_memory().unwrap(),
            vault: SecretVault::new(Arc::new(MemoryStore::default())),
            accounts: Accounts::new(
                Arc::new(FakeAccountRuntime::signed_out()),
                Arc::new(FakeAccountRuntime::signed_out()),
            ),
            llm,
            events: events.clone(),
            generations: Generations::default(),
            scheduler: SchedulerHandle::default(),
            google: GoogleContext::new(GoogleEndpoints::default()),
            browser: Default::default(),
            analytics: Default::default(),
        };
        (state, events)
    }
}
