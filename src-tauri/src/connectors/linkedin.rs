//! LinkedIn: "Sign In with LinkedIn using OpenID Connect", the
//! self-service identity product (`openid profile email`), as a native
//! client with PKCE: no client secret ships in ReMa.
//!
//! Signing in is not a search API. Anything beyond identity (for example
//! first-degree connections, `r_1st_connections`) exists only when LinkedIn
//! approved it for ReMa's app: ReMa asks for such a scope only when the
//! build says the app is approved for it, and uses it only when the token
//! actually grants it (see [`crate::network::capabilities`]). No member
//! search, no second-degree traversal, no page scraping.

use serde_json::Value;

use super::oauth::OAuthApp;
use crate::models::connectors::Capability;

pub const SCOPE_OPENID: &str = "openid";
pub const SCOPE_PROFILE: &str = "profile";
pub const SCOPE_EMAIL: &str = "email";
/// The member's first-degree connections (restricted: LinkedIn approval).
pub const SCOPE_FIRST_DEGREE: &str = "r_1st_connections";

/// LinkedIn endpoints. Debug builds can point them at a local test server.
#[derive(Debug, Clone)]
pub struct LinkedinEndpoints {
    pub auth: String,
    pub token: String,
    pub userinfo: String,
    /// The REST API root (connections, when approved).
    pub api: String,
}

impl Default for LinkedinEndpoints {
    fn default() -> Self {
        Self {
            auth: "https://www.linkedin.com/oauth/v2/authorization".into(),
            token: "https://www.linkedin.com/oauth/v2/accessToken".into(),
            userinfo: "https://api.linkedin.com/v2/userinfo".into(),
            api: "https://api.linkedin.com".into(),
        }
    }
}

impl LinkedinEndpoints {
    pub fn at(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            auth: format!("{base}/linkedin/oauth/v2/authorization"),
            token: format!("{base}/linkedin/oauth/v2/accessToken"),
            userinfo: format!("{base}/linkedin/v2/userinfo"),
            api: format!("{base}/linkedin-api"),
        }
    }

    /// Official endpoints, or (debug builds only) a local mock server set
    /// with `REMA_LINKEDIN_BASE_URL` for end-to-end tests.
    pub fn from_env() -> Self {
        #[cfg(debug_assertions)]
        if let Ok(base) = std::env::var("REMA_LINKEDIN_BASE_URL") {
            return Self::at(&base);
        }
        Self::default()
    }
}

/// ReMa's LinkedIn app (a native PKCE client), set when ReMa is built
/// (`REMA_LINKEDIN_CLIENT_ID`); debug builds may override it at run time
/// (`REMA_DEV_LINKEDIN_CLIENT_ID`). Never a client secret.
pub fn app() -> Option<OAuthApp> {
    #[cfg(debug_assertions)]
    if let Some(client_id) = std::env::var("REMA_DEV_LINKEDIN_CLIENT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        return Some(OAuthApp {
            client_id,
            client_secret: None,
        });
    }
    option_env!("REMA_LINKEDIN_CLIENT_ID")
        .filter(|v| !v.trim().is_empty())
        .map(|client_id| OAuthApp {
            client_id: client_id.to_string(),
            client_secret: None,
        })
}

/// Scopes LinkedIn approved for ReMa's app beyond identity, as the build
/// records them (`REMA_LINKEDIN_APPROVED_SCOPES`, space-separated; debug
/// builds: `REMA_DEV_LINKEDIN_APPROVED_SCOPES`). Only known restricted
/// scopes are accepted.
pub fn approved_scopes() -> Vec<String> {
    #[cfg(debug_assertions)]
    if let Ok(scopes) = std::env::var("REMA_DEV_LINKEDIN_APPROVED_SCOPES") {
        return known(&scopes);
    }
    option_env!("REMA_LINKEDIN_APPROVED_SCOPES")
        .map(known)
        .unwrap_or_default()
}

fn known(scopes: &str) -> Vec<String> {
    scopes
        .split_whitespace()
        .filter(|s| *s == SCOPE_FIRST_DEGREE)
        .map(str::to_string)
        .collect()
}

/// The scopes a sign-in asks for: identity, plus approved extras.
pub fn scopes() -> Vec<String> {
    let mut scopes: Vec<String> = [SCOPE_OPENID, SCOPE_PROFILE, SCOPE_EMAIL]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for extra in approved_scopes() {
        if !scopes.contains(&extra) {
            scopes.push(extra);
        }
    }
    scopes
}

/// Whether the granted scopes allow a capability.
pub fn allows(capability: Capability, granted: &[String]) -> bool {
    let has = |scope: &str| granted.iter().any(|g| g == scope);
    match capability {
        Capability::NetworkIdentity => has(SCOPE_OPENID),
        Capability::NetworkProfile => has(SCOPE_PROFILE),
        Capability::NetworkConnections => has(SCOPE_FIRST_DEGREE),
        _ => false,
    }
}

/// Account details from the OpenID Connect userinfo response.
pub fn userinfo_profile(info: &Value) -> (Option<String>, Option<String>, Option<String>) {
    let text = |key: &str| {
        info.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let name = text("name").or_else(|| {
        let parts: Vec<String> = [text("given_name"), text("family_name")]
            .into_iter()
            .flatten()
            .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    });
    (text("sub"), text("email"), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_for_identity_only_unless_linkedin_approved_more() {
        // No approval recorded in a test build: identity scopes only.
        assert_eq!(scopes(), ["openid", "profile", "email"]);
        assert_eq!(
            known("r_1st_connections w_member_social"),
            ["r_1st_connections"]
        );
        let identity: Vec<String> = scopes();
        assert!(allows(Capability::NetworkIdentity, &identity));
        assert!(allows(Capability::NetworkProfile, &identity));
        assert!(!allows(Capability::NetworkConnections, &identity));
        let approved = vec!["openid".to_string(), SCOPE_FIRST_DEGREE.to_string()];
        assert!(allows(Capability::NetworkConnections, &approved));
        assert!(!allows(Capability::MailRead, &approved));
    }

    #[test]
    fn reads_the_userinfo_profile() {
        let info = serde_json::json!({
            "sub": "782bbtaQ", "name": "Ana Example", "given_name": "Ana",
            "family_name": "Example", "email": "ana@example.com", "email_verified": true
        });
        assert_eq!(
            userinfo_profile(&info),
            (
                Some("782bbtaQ".into()),
                Some("ana@example.com".into()),
                Some("Ana Example".into())
            )
        );
        let partial = serde_json::json!({ "sub": "x", "given_name": "Ana", "family_name": "E" });
        assert_eq!(userinfo_profile(&partial).2.as_deref(), Some("Ana E"));
    }
}
