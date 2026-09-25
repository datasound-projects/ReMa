use serde::Serialize;
use specta::Type;

/// Health of the backend core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum BackendStatus {
    Ready,
}

/// Response of the `get_app_status` command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub status: BackendStatus,
    pub app: String,
    pub version: String,
}
