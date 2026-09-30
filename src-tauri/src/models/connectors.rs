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
            Self::MailRead => "Mail — Read only",
            Self::CalendarRead => "Calendar — Read events",
            Self::CalendarWrite => "Calendar — Create and update events",
            Self::FreeBusy => "Availability — Read",
            Self::NetworkIdentity => "Identity",
            Self::NetworkProfile => "Your profile",
            Self::NetworkConnections => "Connection list (first-degree)",
        }
    }
}

/// Why connecting or checking a connector failed (Spec B §60). Each code
/// comes with an actionable message; none carries a provider token or code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConnectorErrorCode {
    /// The user cancelled or declined the provider's consent.
    UserCancelled,
    /// The browser sign-in was not finished in time.
    SignInTimedOut,
    /// The default browser could not be opened.
    BrowserUnavailable,
    /// The provider rejected ReMa's return address.
    RedirectMismatch,
    /// The browser returned a sign-in ReMa did not start (CSRF protection).
    InvalidState,
    /// The provider did not turn the sign-in into access for ReMa.
    TokenExchangeFailed,
    /// The user did not grant a permission the connector needs.
    ScopeNotGranted,
    /// An organization's policy requires an administrator's approval.
    ProviderAdminPolicy,
    /// The provider API is not enabled for ReMa's registration.
    ApiNotEnabled,
    /// The account has no mailbox or calendar ReMa can use.
    AccountNotSupported,
    /// Access was revoked or expired; the user must reconnect.
    ReauthRequired,
    /// The provider could not be reached.
    NetworkError,
    /// The provider has not verified ReMa's app for this account.
    OauthAppNotVerified,
    /// ReMa's own app registration was rejected (fixed by a ReMa update).
    ProviderConfigurationError,
    /// The system keychain did not answer, so the stored sign-in could not
    /// be read (the connection itself is unchanged).
    CredentialStoreUnavailable,
}

text_enum!(ConnectorErrorCode {
    UserCancelled => "USER_CANCELLED",
    SignInTimedOut => "SIGN_IN_TIMED_OUT",
    BrowserUnavailable => "BROWSER_UNAVAILABLE",
    RedirectMismatch => "REDIRECT_MISMATCH",
    InvalidState => "INVALID_STATE",
    TokenExchangeFailed => "TOKEN_EXCHANGE_FAILED",
    ScopeNotGranted => "SCOPE_NOT_GRANTED",
    ProviderAdminPolicy => "PROVIDER_ADMIN_POLICY",
    ApiNotEnabled => "API_NOT_ENABLED",
    AccountNotSupported => "ACCOUNT_NOT_SUPPORTED",
    ReauthRequired => "REAUTH_REQUIRED",
    NetworkError => "NETWORK_ERROR",
    OauthAppNotVerified => "OAUTH_APP_NOT_VERIFIED",
    ProviderConfigurationError => "PROVIDER_CONFIGURATION_ERROR",
    CredentialStoreUnavailable => "CREDENTIAL_STORE_UNAVAILABLE",
});

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
    /// Connecting failed, the connection check failed, or the last sync
    /// failed (retryable; `error_code` tells which).
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
    /// Why the last sign-in or connection check failed (none for a failed
    /// sync or a working connector).
    pub error_code: Option<ConnectorErrorCode>,
    /// When Google is expected to end this sign-in (ms since the epoch),
    /// an estimate of about 7 days after it was made: set only while the
    /// build says ReMa's Google app is in Testing.
    pub sign_in_ends_at: Option<i64>,
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

/// Where job-related email is read by a model (Spec B §64): the model of
/// "Job Mail & Interview Sync", or the default model it would start with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MailProcessing {
    /// The model, as ReMa names it.
    pub model: String,
    /// Who receives the text of job-related email: a provider ("OpenAI"),
    /// a server's host name, or "this computer".
    pub recipient: String,
    /// The model runs on this computer: mail leaves it only between ReMa and
    /// Google or Microsoft.
    pub on_device: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorsOverview {
    pub connectors: Vec<ConnectorStatus>,
    /// One per provider, in [`ProviderId::ALL`] order.
    pub accounts: Vec<ProviderAccount>,
    pub background: BackgroundSettings,
    /// None until a model is connected.
    pub mail_processing: Option<MailProcessing>,
    pub preferences: ConnectionPreferences,
    /// ReMa's app registration per sign-in provider (Google, Microsoft,
    /// LinkedIn): where it comes from and whether Settings can enter one.
    pub registrations: Vec<AppRegistration>,
}

/// Where a provider's app registration (ReMa's public OAuth client) comes
/// from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationSource {
    /// Compiled into this copy of ReMa.
    Build,
    /// Entered in Settings → Connectors → Set up, kept on this computer.
    Settings,
    /// None: the provider's sign-in is unavailable until one is entered.
    None,
}

/// A provider's app registration as Settings shows it. Client IDs are
/// public identifiers; a client secret is never included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppRegistration {
    pub provider: ProviderId,
    pub source: RegistrationSource,
    /// The client ID in use, when there is one.
    pub client_id: Option<String>,
    /// Google: a Desktop client secret is stored (its value is never shown).
    pub client_secret_set: bool,
    /// Google: "testing", "production" or "" (not said).
    pub publishing_status: String,
    /// LinkedIn: approved restricted scopes, space-separated.
    pub approved_scopes: String,
    /// Settings can enter, change or remove it (the build carries none).
    pub editable: bool,
}

/// What Settings → Connectors → Set up sends.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppRegistrationInput {
    pub client_id: String,
    /// Google only. `None` keeps the stored secret; an empty string removes
    /// it; anything else replaces it.
    pub client_secret: Option<String>,
    /// Google: "testing", "production" or "".
    pub publishing_status: String,
    /// LinkedIn: approved restricted scopes, space-separated.
    pub approved_scopes: String,
}

impl std::fmt::Debug for AppRegistrationInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppRegistrationInput")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("publishing_status", &self.publishing_status)
            .field("approved_scopes", &self.approved_scopes)
            .finish()
    }
}

/// Connectors changed (state, account, sync).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ConnectorsChanged;

/// The state of one provider account's connection (the account-level
/// state machine behind every connector of that provider).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// No account is connected.
    Disconnected,
    /// Waiting for the browser sign-in.
    Connecting,
    /// Connected; requests get a valid access token.
    Connected,
    /// A renewal of the access token is in flight.
    Refreshing,
    /// The provider ended the grant; the user must sign in again.
    ReauthRequired,
    /// Connected, but a permission a connector needs was not granted.
    PermissionDenied,
    /// An organization's policy requires an administrator's approval.
    AdminApprovalRequired,
    /// The provider rejected ReMa (configuration, API not enabled, an
    /// unverified app) or the last sign-in failed for another reason.
    ProviderError,
    /// The provider could not be reached (or the keychain did not answer).
    Offline,
    /// This build of ReMa has no public app configuration for the provider.
    Unavailable,
}

/// One capability of a provider account as the account card lists it
/// ("✓ Gmail"): a connector of the provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityView {
    pub connector: ConnectorId,
    pub name: String,
    /// The user added this connector and the provider granted what it
    /// needs.
    pub granted: bool,
    /// The connector's own state (sync, permissions).
    pub state: ConnectorState,
}

/// A provider account as Settings → Connectors shows it: one card per
/// provider (Google, Microsoft), with the connectors it enables. Nothing
/// here carries a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccount {
    pub provider: ProviderId,
    pub name: String,
    pub state: ConnectionState,
    /// A stable id of the connection (`<provider>:<account id>`), for
    /// diagnostics; never a token.
    pub connection_id: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub capabilities: Vec<CapabilityView>,
    /// A short user-readable explanation for the current state.
    pub message: Option<String>,
    /// Technical details for "Show details" (no secrets).
    pub detail: Option<String>,
    pub error_code: Option<ConnectorErrorCode>,
    pub connected_at: Option<i64>,
    /// When the grant was last renewed (None: not since the sign-in).
    pub last_refreshed_at: Option<i64>,
    /// See [`ConnectorStatus::sign_in_ends_at`].
    pub sign_in_ends_at: Option<i64>,
    /// This build has the provider's public app configuration.
    pub available: bool,
}

/// Settings that decide how connections meet chats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionPreferences {
    /// A newly connected account is also added to chats that chose their
    /// own connectors (off: only chats using "all connected" see it).
    pub new_accounts_in_chats: bool,
}
