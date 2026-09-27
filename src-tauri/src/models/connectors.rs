//! Connectors as the interface sees them: state, account and permissions.
//! Nothing here ever carries a token, code, verifier or client secret.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::text_enum;

/// The account provider behind a connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Google,
    Microsoft,
}

text_enum!(ProviderId {
    Google => "google",
    Microsoft => "microsoft",
});

impl ProviderId {
    pub const ALL: [ProviderId; 2] = [ProviderId::Google, ProviderId::Microsoft];

    pub fn name(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Microsoft => "Microsoft",
        }
    }
}

/// One connector card in Settings → Connectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorId {
    Gmail,
    GoogleCalendar,
    OutlookMail,
    OutlookCalendar,
}

text_enum!(ConnectorId {
    Gmail => "gmail",
    GoogleCalendar => "google_calendar",
    OutlookMail => "outlook_mail",
    OutlookCalendar => "outlook_calendar",
});

impl ConnectorId {
    pub const ALL: [ConnectorId; 4] = [
        ConnectorId::Gmail,
        ConnectorId::GoogleCalendar,
        ConnectorId::OutlookMail,
        ConnectorId::OutlookCalendar,
    ];

    pub fn provider(self) -> ProviderId {
        match self {
            Self::Gmail | Self::GoogleCalendar => ProviderId::Google,
            Self::OutlookMail | Self::OutlookCalendar => ProviderId::Microsoft,
        }
    }

    pub fn kind(self) -> ConnectorKind {
        match self {
            Self::Gmail | Self::OutlookMail => ConnectorKind::Mail,
            Self::GoogleCalendar | Self::OutlookCalendar => ConnectorKind::Calendar,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Gmail => "Gmail",
            Self::GoogleCalendar => "Google Calendar",
            Self::OutlookMail => "Outlook Mail",
            Self::OutlookCalendar => "Outlook Calendar",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Gmail => "Read job-related emails and track application updates.",
            Self::GoogleCalendar | Self::OutlookCalendar => {
                "Check availability and manage confirmed interviews."
            }
            Self::OutlookMail => "Read job-related Outlook emails and track application updates.",
        }
    }

    /// What ReMa may do with this connector.
    pub fn capabilities(self) -> &'static [Capability] {
        match self.kind() {
            ConnectorKind::Mail => &[Capability::MailRead],
            ConnectorKind::Calendar => &[
                Capability::CalendarRead,
                Capability::CalendarWrite,
                Capability::FreeBusy,
            ],
        }
    }

    /// The mail or calendar connector of a provider.
    pub fn of(provider: ProviderId, kind: ConnectorKind) -> Self {
        match (provider, kind) {
            (ProviderId::Google, ConnectorKind::Mail) => Self::Gmail,
            (ProviderId::Google, ConnectorKind::Calendar) => Self::GoogleCalendar,
            (ProviderId::Microsoft, ConnectorKind::Mail) => Self::OutlookMail,
            (ProviderId::Microsoft, ConnectorKind::Calendar) => Self::OutlookCalendar,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorKind {
    Mail,
    Calendar,
}

/// A permission a connector needs, mapped to provider scopes in Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    MailRead,
    CalendarRead,
    CalendarWrite,
    FreeBusy,
}

impl Capability {
    pub fn label(self) -> &'static str {
        match self {
            Self::MailRead => "Mail — Read",
            Self::CalendarRead => "Calendar — Read events",
            Self::CalendarWrite => "Calendar — Create and update events",
            Self::FreeBusy => "Calendar — Availability",
        }
    }
}

/// What a connector card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorState {
    /// Not added (shows `+`).
    Disconnected,
    /// Waiting for the browser sign-in.
    Connecting,
    Connected,
    /// Access was revoked or expired; the user must reconnect.
    ReauthRequired,
    /// Connected, but a permission this connector needs was not granted.
    PermissionMissing,
    /// A sync is running right now.
    Syncing,
    /// The last sync failed (retryable).
    Error,
    /// This build of ReMa has no sign-in configured for the provider.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PermissionView {
    pub capability: Capability,
    pub label: String,
    pub granted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorStatus {
    pub id: ConnectorId,
    pub provider: ProviderId,
    pub kind: ConnectorKind,
    pub name: String,
    /// "Google" / "Microsoft" (shown as "by Google").
    pub publisher: String,
    pub description: String,
    pub state: ConnectorState,
    /// The user added this connector (it may still need attention).
    pub enabled: bool,
    pub account_email: Option<String>,
    pub account_name: Option<String>,
    pub permissions: Vec<PermissionView>,
    /// Mail connectors: synchronize while ReMa runs.
    pub background_sync: Option<bool>,
    pub last_sync_started_at: Option<i64>,
    pub last_sync_at: Option<i64>,
    pub next_sync_at: Option<i64>,
    /// A short user-readable explanation for the current state.
    pub message: Option<String>,
    /// Technical details for "Show details" (no secrets).
    pub detail: Option<String>,
}

/// What happens when an email confirms an interview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum InterviewMode {
    /// Propose the event; the user adds it (default).
    Ask,
    /// Add it when the confirmation is unambiguous and the time is free.
    Auto,
}

text_enum!(InterviewMode {
    Ask => "ask",
    Auto => "auto",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorPreferences {
    pub interview_mode: InterviewMode,
    /// Minutes between background syncs (5–1440).
    pub sync_interval_minutes: u32,
    /// Free time required before and after an interview (0–120 minutes).
    pub prep_buffer_minutes: u32,
}

/// Explicit background execution options. Nothing runs once ReMa quits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSettings {
    /// Closing the window keeps ReMa running in the system tray.
    pub run_in_background: bool,
    /// ReMa starts (in the background) when the user logs in.
    pub start_at_login: bool,
    /// The platform offers a system tray (needed to run in the background).
    pub tray_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorsOverview {
    pub connectors: Vec<ConnectorStatus>,
    pub preferences: ConnectorPreferences,
    pub background: BackgroundSettings,
}

/// Connectors changed (state, account, sync).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ConnectorsChanged;
