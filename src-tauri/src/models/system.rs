use serde::{Deserialize, Serialize};
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

/// ReMa's color theme. Light is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    Light,
    Dark,
}

impl Appearance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    /// The window's canvas before the page paints (matches the launch intro).
    pub fn window_rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Light => (0xf8, 0xf7, 0xf4),
            Self::Dark => (0x1f, 0x1e, 0x1d),
        }
    }
}
