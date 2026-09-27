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
    Linkedin,
    Xing,
}

text_enum!(ProviderId {
    Google => "google",
    Microsoft => "microsoft",
    Linkedin => "linkedin",
    Xing => "xing",
});

impl ProviderId {
    pub const ALL: [ProviderId; 4] = [
        ProviderId::Google,
        ProviderId::Microsoft,
        ProviderId::Linkedin,
        ProviderId::Xing,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Microsoft => "Microsoft",
            Self::Linkedin => "LinkedIn",
            Self::Xing => "XING",
        }
    }

    /// Its index in per-provider arrays.
    pub fn index(self) -> usize {
        match self {
            Self::Google => 0,
            Self::Microsoft => 1,
            Self::Linkedin => 2,
            Self::Xing => 3,
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
    Linkedin,
    Xing,
}

text_enum!(ConnectorId {
    Gmail => "gmail",
    GoogleCalendar => "google_calendar",
    OutlookMail => "outlook_mail",
    OutlookCalendar => "outlook_calendar",
    Linkedin => "linkedin",
    Xing => "xing",
});

impl ConnectorId {
    pub const ALL: [ConnectorId; 6] = [
        ConnectorId::Gmail,
        ConnectorId::GoogleCalendar,
        ConnectorId::OutlookMail,
        ConnectorId::OutlookCalendar,
        ConnectorId::Linkedin,
        ConnectorId::Xing,
    ];

    pub fn provider(self) -> ProviderId {
        match self {
            Self::Gmail | Self::GoogleCalendar => ProviderId::Google,
            Self::OutlookMail | Self::OutlookCalendar => ProviderId::Microsoft,
            Self::Linkedin => ProviderId::Linkedin,
            Self::Xing => ProviderId::Xing,
        }
    }

    pub fn kind(self) -> ConnectorKind {
        match self {
            Self::Gmail | Self::OutlookMail => ConnectorKind::Mail,
            Self::GoogleCalendar | Self::OutlookCalendar => ConnectorKind::Calendar,
            Self::Linkedin | Self::Xing => ConnectorKind::Network,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Gmail => "Gmail",
            Self::GoogleCalendar => "Google Calendar",
            Self::OutlookMail => "Outlook Mail",
            Self::OutlookCalendar => "Outlook Calendar",
            Self::Linkedin => "LinkedIn",
            Self::Xing => "XING",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Gmail => "Read job-related emails and track application updates.",
            Self::GoogleCalendar | Self::OutlookCalendar => {
                "Check availability and manage confirmed interviews."
            }
            Self::OutlookMail => "Read job-related Outlook emails and track application updates.",
            Self::Linkedin => {
                "Connect your professional identity and the network capabilities LinkedIn grants \
                 ReMa."
            }
            Self::Xing => {
                "Connect your XING identity and the professional capabilities XING grants ReMa."
            }
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
            ConnectorKind::Network => &[
                Capability::NetworkIdentity,
                Capability::NetworkProfile,
                Capability::NetworkConnections,
            ],
        }
    }

    /// The mail or calendar connector of a provider (Google or Microsoft).
    pub fn of(provider: ProviderId, kind: ConnectorKind) -> Self {
        match (provider, kind) {
            (ProviderId::Google, ConnectorKind::Calendar) => Self::GoogleCalendar,
            (ProviderId::Google, _) => Self::Gmail,
            (ProviderId::Microsoft, ConnectorKind::Calendar) => Self::OutlookCalendar,
            (ProviderId::Microsoft, _) => Self::OutlookMail,
            (ProviderId::Linkedin, _) => Self::Linkedin,
            (ProviderId::Xing, _) => Self::Xing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorKind {
    Mail,
    Calendar,
    /// A professional network (LinkedIn, XING).
    Network,
}

/// A permission a connector needs, mapped to provider scopes in Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    MailRead,
    CalendarRead,
    CalendarWrite,
    FreeBusy,
    /// Sign-in identity (name, account id, email).
    NetworkIdentity,
    /// The member's own profile.
    NetworkProfile,
    /// The member's first-degree connection list (partner-approved only).
    NetworkConnections,
}

impl Capability {
    pub fn label(self) -> &'static str {
        match self {
            Self::MailRead => "Mail — Read",
            Self::CalendarRead => "Calendar — Read events",
            Self::CalendarWrite => "Calendar — Create and update events",
            Self::FreeBusy => "Calendar — Availability",
            Self::NetworkIdentity => "Identity",
            Self::NetworkProfile => "Your profile",
            Self::NetworkConnections => "Connection list (first-degree)",
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
    pub last_sync_started_at: Option<i64>,
    /// The last successful use by "Job Mail & Interview Sync" (mail) or its
    /// calendar step (calendars).
    pub last_sync_at: Option<i64>,
    /// A short user-readable explanation for the current state.
    pub message: Option<String>,
    /// Technical details for "Show details" (no secrets).
    pub detail: Option<String>,
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
    pub background: BackgroundSettings,
}

/// Connectors changed (state, account, sync).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ConnectorsChanged;
