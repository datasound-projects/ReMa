use serde::Serialize;

/// Health of the backend core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendStatus {
    Ready,
}

/// Response of the `get_app_status` command.
///
/// Mirrored in TypeScript by `AppStatus` in `src/types/system.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub status: BackendStatus,
    pub app: String,
    pub version: String,
}
