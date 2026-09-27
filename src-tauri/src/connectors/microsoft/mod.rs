//! Microsoft: ReMa registered in Microsoft Entra as a public desktop client
//! (no client secret), delegated Microsoft Graph permissions, and the Outlook
//! mail and calendar APIs.

pub mod calendar;
pub mod mail;

use super::oauth::OAuthApp;
use crate::models::connectors::{Capability, ConnectorId};

/// Identity and long-lived access (refresh tokens need `offline_access`).
pub const BASE_SCOPES: [&str; 5] = ["openid", "profile", "email", "offline_access", "User.Read"];
/// Read the signed-in user's mail. Never Mail.ReadWrite or Mail.Send.
pub const SCOPE_MAIL_READ: &str = "Mail.Read";
/// Read and manage the signed-in user's calendar events.
pub const SCOPE_CALENDARS_READWRITE: &str = "Calendars.ReadWrite";

/// Microsoft identity platform and Graph endpoints.
#[derive(Debug, Clone)]
pub struct MicrosoftEndpoints {
    pub authorize: String,
    pub token: String,
    pub graph: String,
}

/// Personal Microsoft accounts (Outlook.com) and work or school accounts.
fn tenant() -> &'static str {
    option_env!("REMA_MICROSOFT_TENANT")
        .filter(|v| !v.trim().is_empty())
        .unwrap_or("common")
}

impl Default for MicrosoftEndpoints {
    fn default() -> Self {
        Self::at(
            "https://login.microsoftonline.com",
            "https://graph.microsoft.com/v1.0",
        )
    }
}

impl MicrosoftEndpoints {
    pub fn at(login: &str, graph: &str) -> Self {
        let login = login.trim_end_matches('/');
        let tenant = tenant();
        Self {
            authorize: format!("{login}/{tenant}/oauth2/v2.0/authorize"),
            token: format!("{login}/{tenant}/oauth2/v2.0/token"),
            graph: graph.trim_end_matches('/').to_string(),
        }
    }

    /// Official endpoints, or (debug builds only) a local mock server set
    /// with `REMA_MICROSOFT_BASE_URL` for end-to-end tests.
    pub fn from_env() -> Self {
        #[cfg(debug_assertions)]
        if let Ok(base) = std::env::var("REMA_MICROSOFT_BASE_URL") {
            let base = base.trim_end_matches('/');
            return Self::at(base, &format!("{base}/graph/v1.0"));
        }
        Self::default()
    }
}

/// ReMa's Entra application (public client), set when ReMa is built
/// (`REMA_MICROSOFT_CLIENT_ID`). Debug builds may override it at run time
/// (`REMA_DEV_MICROSOFT_CLIENT_ID`). There is no client secret.
pub fn app() -> Option<OAuthApp> {
    #[cfg(debug_assertions)]
    if let Some(client_id) = std::env::var("REMA_DEV_MICROSOFT_CLIENT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        return Some(OAuthApp {
            client_id,
            client_secret: None,
        });
    }
    option_env!("REMA_MICROSOFT_CLIENT_ID")
        .filter(|v| !v.trim().is_empty())
        .map(|client_id| OAuthApp {
            client_id: client_id.to_string(),
            client_secret: None,
        })
}

pub fn scopes(connectors: &[ConnectorId]) -> Vec<String> {
    let mut scopes: Vec<&str> = BASE_SCOPES.to_vec();
    if connectors.contains(&ConnectorId::OutlookMail) {
        scopes.push(SCOPE_MAIL_READ);
    }
    if connectors.contains(&ConnectorId::OutlookCalendar) {
        scopes.push(SCOPE_CALENDARS_READWRITE);
    }
    scopes.into_iter().map(str::to_string).collect()
}

/// `https://graph.microsoft.com/Mail.Read` and `mail.read` both mean Mail.Read.
fn normalize(scope: &str) -> String {
    scope
        .trim()
        .trim_start_matches("https://graph.microsoft.com/")
        .to_ascii_lowercase()
}

pub fn allows(capability: Capability, granted: &[String]) -> bool {
    let has = |scope: &str| granted.iter().any(|g| normalize(g) == normalize(scope));
    match capability {
        Capability::MailRead => has(SCOPE_MAIL_READ),
        Capability::CalendarRead | Capability::CalendarWrite | Capability::FreeBusy => {
            has(SCOPE_CALENDARS_READWRITE)
        }
        Capability::NetworkIdentity
        | Capability::NetworkProfile
        | Capability::NetworkConnections => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegated_least_privilege_scopes() {
        let mail = scopes(&[ConnectorId::OutlookMail]);
        assert!(
            mail.contains(&"offline_access".to_string()),
            "refresh tokens"
        );
        assert!(mail.contains(&"Mail.Read".to_string()));
        assert!(!mail
            .iter()
            .any(|s| s == "Mail.ReadWrite" || s == "Mail.Send"));
        let both = scopes(&[ConnectorId::OutlookMail, ConnectorId::OutlookCalendar]);
        assert!(both.contains(&"Calendars.ReadWrite".to_string()));
    }

    #[test]
    fn reads_granted_scopes_in_any_form() {
        let granted = vec![
            "https://graph.microsoft.com/Mail.Read".to_string(),
            "user.read".to_string(),
        ];
        assert!(allows(Capability::MailRead, &granted));
        assert!(!allows(Capability::CalendarWrite, &granted));
    }

    #[test]
    fn uses_the_common_tenant_for_personal_and_work_accounts() {
        let endpoints = MicrosoftEndpoints::default();
        assert!(endpoints
            .authorize
            .starts_with("https://login.microsoftonline.com/"));
        assert!(endpoints.authorize.ends_with("/oauth2/v2.0/authorize"));
        assert_eq!(endpoints.graph, "https://graph.microsoft.com/v1.0");
    }
}
