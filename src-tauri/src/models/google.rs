use serde::{Deserialize, Serialize};
use specta::Type;

/// A Google Workspace capability ReMa can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GoogleService {
    Gmail,
    Calendar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GoogleClientSource {
    /// Compiled into this build of ReMa.
    Builtin,
    /// Entered by the user in Settings.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GoogleServiceStatus {
    /// The user wants ReMa to use this service.
    pub enabled: bool,
    /// Google granted the permission this service needs.
    pub granted: bool,
}

/// Google Workspace connection as shown in Settings. Never contains tokens.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GoogleStatus {
    pub client: Option<GoogleClientSource>,
    pub connected: bool,
    /// Google rejected the stored authorization; the user must reconnect.
    pub needs_reconnect: bool,
    pub email: Option<String>,
    pub gmail: GoogleServiceStatus,
    pub calendar: GoogleServiceStatus,
    /// A sign-in is waiting for the browser.
    pub connecting: bool,
}

/// The Google connection changed.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct GoogleChanged;
