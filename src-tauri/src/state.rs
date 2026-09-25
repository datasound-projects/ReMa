use tauri::PackageInfo;

/// Shared application state, created once at startup and managed by Tauri.
///
/// Commands receive it via `tauri::State<'_, AppState>` and pass it to
/// services. Future long-lived resources (settings, storage handles, caches)
/// belong here; anything mutable must use interior mutability
/// (e.g. `Mutex`/`RwLock`) because commands may run concurrently.
#[derive(Debug)]
pub struct AppState {
    pub app_name: String,
    pub version: String,
}

impl AppState {
    pub fn new(package: &PackageInfo) -> Self {
        Self {
            app_name: package.name.clone(),
            version: package.version.to_string(),
        }
    }
}
