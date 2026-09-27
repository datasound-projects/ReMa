//! Google: ReMa's own "Desktop app" OAuth client, the scopes each connector
//! needs (least privilege), and the Gmail and Calendar APIs.

pub mod calendar;
pub mod gmail;

use super::oauth::OAuthApp;
use crate::models::connectors::{Capability, ConnectorId};

/// Identify the account (`openid email profile`).
pub const SCOPE_OPENID: &str = "openid";
pub const SCOPE_EMAIL: &str = "email";
pub const SCOPE_PROFILE: &str = "profile";
/// Read messages and search the mailbox. A restricted scope: see
/// docs/connectors/compliance.md. No modify, compose or send scope.
pub const SCOPE_GMAIL_READONLY: &str = "https://www.googleapis.com/auth/gmail.readonly";
/// Read events and create/update the events ReMa manages.
pub const SCOPE_CALENDAR_EVENTS: &str = "https://www.googleapis.com/auth/calendar.events";
/// Availability (free/busy) only.
pub const SCOPE_CALENDAR_FREEBUSY: &str = "https://www.googleapis.com/auth/calendar.freebusy";

/// Google endpoints. Debug builds can point them at a local test server.
#[derive(Debug, Clone)]
pub struct GoogleEndpoints {
    pub auth: String,
    pub token: String,
    pub revoke: String,
    pub gmail: String,
    pub calendar: String,
}

impl Default for GoogleEndpoints {
    fn default() -> Self {
        Self {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
            gmail: "https://gmail.googleapis.com/gmail/v1".into(),
            calendar: "https://www.googleapis.com/calendar/v3".into(),
        }
    }
}

impl GoogleEndpoints {
    pub fn at(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            auth: format!("{base}/o/oauth2/v2/auth"),
            token: format!("{base}/token"),
            revoke: format!("{base}/revoke"),
            gmail: format!("{base}/gmail/v1"),
            calendar: format!("{base}/calendar/v3"),
        }
    }

    /// Official endpoints, or (debug builds only) a local mock server set
    /// with `REMA_GOOGLE_BASE_URL` for end-to-end tests.
    pub fn from_env() -> Self {
        #[cfg(debug_assertions)]
        if let Ok(base) = std::env::var("REMA_GOOGLE_BASE_URL") {
            return Self::at(&base);
        }
        Self::default()
    }
}

/// ReMa's Google OAuth client, set when ReMa is built
/// (`REMA_GOOGLE_CLIENT_ID`, `REMA_GOOGLE_CLIENT_SECRET`). Debug builds may
/// override it at run time for development (`REMA_DEV_GOOGLE_CLIENT_ID`).
pub fn app() -> Option<OAuthApp> {
    #[cfg(debug_assertions)]
    if let Some(client_id) = std::env::var("REMA_DEV_GOOGLE_CLIENT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        return Some(OAuthApp {
            client_id,
            client_secret: std::env::var("REMA_DEV_GOOGLE_CLIENT_SECRET")
                .ok()
                .filter(|v| !v.trim().is_empty()),
        });
    }
    option_env!("REMA_GOOGLE_CLIENT_ID")
        .filter(|v| !v.trim().is_empty())
        .map(|client_id| OAuthApp {
            client_id: client_id.to_string(),
            client_secret: option_env!("REMA_GOOGLE_CLIENT_SECRET")
                .filter(|v| !v.trim().is_empty())
                .map(str::to_string),
        })
}

/// The scopes for a set of connectors: identity plus each connector's own.
/// Installed apps get no incremental authorization, so a sign-in always asks
/// for the union of the enabled Google connectors.
pub fn scopes(connectors: &[ConnectorId]) -> Vec<String> {
    let mut scopes = vec![SCOPE_OPENID, SCOPE_EMAIL, SCOPE_PROFILE];
    if connectors.contains(&ConnectorId::Gmail) {
        scopes.push(SCOPE_GMAIL_READONLY);
    }
    if connectors.contains(&ConnectorId::GoogleCalendar) {
        scopes.push(SCOPE_CALENDAR_EVENTS);
        scopes.push(SCOPE_CALENDAR_FREEBUSY);
    }
    scopes.into_iter().map(str::to_string).collect()
}

/// Whether the granted scopes allow a capability.
pub fn allows(capability: Capability, granted: &[String]) -> bool {
    let has = |scope: &str| granted.iter().any(|g| g == scope);
    match capability {
        Capability::MailRead => has(SCOPE_GMAIL_READONLY),
        Capability::CalendarRead | Capability::CalendarWrite => has(SCOPE_CALENDAR_EVENTS),
        Capability::FreeBusy => has(SCOPE_CALENDAR_FREEBUSY),
        Capability::NetworkIdentity
        | Capability::NetworkProfile
        | Capability::NetworkConnections => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_for_the_union_of_enabled_connectors_only() {
        assert_eq!(
            scopes(&[ConnectorId::Gmail]),
            ["openid", "email", "profile", SCOPE_GMAIL_READONLY]
        );
        let both = scopes(&[ConnectorId::Gmail, ConnectorId::GoogleCalendar]);
        assert!(both.contains(&SCOPE_GMAIL_READONLY.to_string()));
        assert!(both.contains(&SCOPE_CALENDAR_EVENTS.to_string()));
        assert!(both.contains(&SCOPE_CALENDAR_FREEBUSY.to_string()));
        // Never broad mail or send scopes.
        for scope in &both {
            assert!(!scope.contains("gmail.modify") && !scope.contains("gmail.send"));
            assert!(!scope.contains("gmail.compose") && scope != "https://mail.google.com/");
            assert_ne!(scope, "https://www.googleapis.com/auth/calendar");
        }
    }

    #[test]
    fn capabilities_follow_granted_scopes() {
        let granted = vec![SCOPE_CALENDAR_EVENTS.to_string()];
        assert!(allows(Capability::CalendarWrite, &granted));
        assert!(!allows(Capability::FreeBusy, &granted));
        assert!(!allows(Capability::MailRead, &granted));
    }
}
