//! The provider capability registry (NC §6, §51, §52): what each
//! professional network actually lets ReMa do. Values come from the
//! connector account's granted scopes and the provider's documented
//! offering, never from the presence of a Connect button.
//!
//! LinkedIn's self-service sign-in grants identity (`openid profile email`);
//! its Connections API is partner-approved and first-degree only; it has no
//! member or company search for ReMa and no second-degree traversal. XING
//! offers ReMa no desktop sign-in or API (see `connectors::xing`).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    connectors::{self, tokens},
    db::connectors::{self as repo, AccountRecord, AccountStatus},
    error::AppResult,
    models::connectors::{Capability, ProviderId},
    state::AppState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NetworkCapability {
    AuthenticateIdentity,
    ReadSelfProfile,
    SearchCompanies,
    SearchJobs,
    SearchPeople,
    ReadFirstDegreeConnections,
    ReadSecondDegreeConnections,
    ReadProfileDetails,
    PersistentStorageAllowed,
}

impl NetworkCapability {
    pub const ALL: [NetworkCapability; 9] = [
        Self::AuthenticateIdentity,
        Self::ReadSelfProfile,
        Self::SearchCompanies,
        Self::SearchJobs,
        Self::SearchPeople,
        Self::ReadFirstDegreeConnections,
        Self::ReadSecondDegreeConnections,
        Self::ReadProfileDetails,
        Self::PersistentStorageAllowed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::AuthenticateIdentity => "Identity",
            Self::ReadSelfProfile => "Your profile",
            Self::SearchCompanies => "Company search",
            Self::SearchJobs => "Job search",
            Self::SearchPeople => "People search",
            Self::ReadFirstDegreeConnections => "Connection-list access",
            Self::ReadSecondDegreeConnections => "Second-degree connections",
            Self::ReadProfileDetails => "Other members' profile details",
            Self::PersistentStorageAllowed => "Keeping member data",
        }
    }
}

/// Whether ReMa can connect to a provider at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAccess {
    /// No approved integration exists for ReMa.
    NotAvailable,
    NotConnected,
    Connected,
    /// Signed in before; the access expired or was revoked.
    ReconnectNeeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityItem {
    pub capability: NetworkCapability,
    pub label: String,
    /// Why it is (not) available, in plain words.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub provider: ProviderId,
    pub name: String,
    pub access: ProviderAccess,
    pub account_name: Option<String>,
    pub available: Vec<CapabilityItem>,
    pub unavailable: Vec<CapabilityItem>,
    /// One sentence for the provider card and for answers.
    pub summary: String,
    /// The scopes behind it (for "Advanced details").
    pub granted_scopes: Vec<String>,
}

impl ProviderCapabilities {
    pub fn has(&self, capability: NetworkCapability) -> bool {
        self.available.iter().any(|c| c.capability == capability)
    }
}

fn item(capability: NetworkCapability, reason: &str) -> CapabilityItem {
    CapabilityItem {
        capability,
        label: capability.label().to_string(),
        reason: reason.to_string(),
    }
}

/// A provider's capabilities from its account (pure; tested directly).
pub fn of(
    provider: ProviderId,
    account: Option<&AccountRecord>,
    has_token: bool,
    sign_in_available: bool,
) -> ProviderCapabilities {
    let name = provider.name().to_string();
    let access = if !sign_in_available {
        ProviderAccess::NotAvailable
    } else {
        match account {
            None => ProviderAccess::NotConnected,
            Some(a) if a.status == AccountStatus::ReauthRequired || !has_token => {
                ProviderAccess::ReconnectNeeded
            }
            Some(_) => ProviderAccess::Connected,
        }
    };
    let granted: Vec<String> = account
        .filter(|_| access == ProviderAccess::Connected)
        .map(|a| a.granted_scopes.clone())
        .unwrap_or_default();
    let allows = |c: Capability| connectors::allows(provider, c, &granted);
    let mut available = Vec::new();
    let mut unavailable = Vec::new();
    match provider {
        ProviderId::Linkedin => {
            let connected = access == ProviderAccess::Connected;
            let not_connected = "Connect LinkedIn to use it.";
            for (capability, granted, missing) in [
                (
                    NetworkCapability::AuthenticateIdentity,
                    allows(Capability::NetworkIdentity),
                    not_connected,
                ),
                (
                    NetworkCapability::ReadSelfProfile,
                    allows(Capability::NetworkProfile),
                    not_connected,
                ),
            ] {
                if granted {
                    available.push(item(capability, "Granted when you signed in."));
                } else {
                    unavailable.push(item(capability, missing));
                }
            }
            if allows(Capability::NetworkConnections) {
                available.push(item(
                    NetworkCapability::ReadFirstDegreeConnections,
                    "LinkedIn approved this for ReMa and you granted it: your first-degree \
                     connections only.",
                ));
            } else {
                unavailable.push(item(
                    NetworkCapability::ReadFirstDegreeConnections,
                    if connected {
                        "LinkedIn has not granted ReMa access to connection lists (a \
                         partner-approved permission)."
                    } else {
                        "Needs LinkedIn sign-in and a partner-approved permission ReMa does \
                         not have."
                    },
                ));
            }
            unavailable.push(item(
                NetworkCapability::ReadSecondDegreeConnections,
                "LinkedIn's Connections API does not allow browsing a connection's connections.",
            ));
            for capability in [
                NetworkCapability::SearchPeople,
                NetworkCapability::SearchCompanies,
                NetworkCapability::SearchJobs,
                NetworkCapability::ReadProfileDetails,
            ] {
                unavailable.push(item(
                    capability,
                    "Not part of LinkedIn's sign-in; ReMa uses public sources and its job \
                     search instead.",
                ));
            }
            unavailable.push(item(
                NetworkCapability::PersistentStorageAllowed,
                "Member data from LinkedIn is shown for this session only, never stored.",
            ));
        }
        _ => {
            let reason = if provider == ProviderId::Xing {
                crate::connectors::xing::UNAVAILABLE.to_string()
            } else {
                "Not a professional network.".to_string()
            };
            for capability in NetworkCapability::ALL {
                unavailable.push(item(capability, &reason));
            }
        }
    }
    let summary = summary(provider, access, &available);
    ProviderCapabilities {
        provider,
        name,
        access,
        account_name: account
            .filter(|_| access == ProviderAccess::Connected)
            .and_then(|a| a.display_name.clone().or_else(|| a.email.clone())),
        available,
        unavailable,
        summary,
        granted_scopes: granted,
    }
}

fn summary(provider: ProviderId, access: ProviderAccess, available: &[CapabilityItem]) -> String {
    let name = provider.name();
    let connections = available
        .iter()
        .any(|c| c.capability == NetworkCapability::ReadFirstDegreeConnections);
    match access {
        ProviderAccess::NotAvailable if provider == ProviderId::Xing => {
            crate::connectors::xing::UNAVAILABLE.to_string()
        }
        ProviderAccess::NotAvailable => {
            format!("{name} sign-in is not available in this build of ReMa.")
        }
        ProviderAccess::NotConnected => format!(
            "{name} is not connected. ReMa researches companies, jobs and people from public \
             sources without it."
        ),
        ProviderAccess::ReconnectNeeded => format!(
            "{name} access has expired or was revoked. Reconnect it to use your identity again."
        ),
        ProviderAccess::Connected if connections => format!(
            "{name} is connected: identity, your profile and your first-degree connection list."
        ),
        ProviderAccess::Connected => format!(
            "{name} is connected for identity, but ReMa does not currently have permission to \
             read your connection list."
        ),
    }
}

/// Every professional network's capabilities right now.
pub async fn all(state: &AppState) -> AppResult<Vec<ProviderCapabilities>> {
    let mut out = Vec::new();
    for provider in [ProviderId::Linkedin, ProviderId::Xing] {
        let account = state.db.call(move |c| repo::account(c, provider))?;
        let has_token = tokens::usable(state, provider).await?;
        out.push(of(
            provider,
            account.as_ref(),
            has_token,
            state.connectors.app(provider).is_some(),
        ));
    }
    Ok(out)
}

/// What a relationship question can rely on (§22, §51): which provider can
/// answer it, or exactly why none can. Never "no connections" when the
/// graph was not checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationshipAccess {
    Available(ProviderId),
    Unavailable(String),
}

pub fn relationship_access(providers: &[ProviderCapabilities]) -> RelationshipAccess {
    if let Some(p) = providers
        .iter()
        .find(|p| p.has(NetworkCapability::ReadFirstDegreeConnections))
    {
        return RelationshipAccess::Available(p.provider);
    }
    let linkedin = providers
        .iter()
        .find(|p| p.provider == ProviderId::Linkedin);
    RelationshipAccess::Unavailable(match linkedin.map(|p| p.access) {
        Some(ProviderAccess::Connected) => "LinkedIn is connected for identity, but ReMa does \
            not currently have permission to read your connection list, so ReMa cannot tell \
            whom you know."
            .into(),
        Some(ProviderAccess::ReconnectNeeded) => "LinkedIn access has expired; even when \
            connected, ReMa has no permission to read your connection list, so it cannot tell \
            whom you know."
            .into(),
        _ => "No professional network that shares its connection list with ReMa is connected, \
            so ReMa cannot tell whom you know. It can still research relevant people."
            .into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(scopes: &[&str], status: AccountStatus) -> AccountRecord {
        AccountRecord {
            provider: ProviderId::Linkedin,
            account_id: Some("abc".into()),
            email: Some("ana@example.com".into()),
            display_name: Some("Ana Example".into()),
            granted_scopes: scopes.iter().map(|s| s.to_string()).collect(),
            status,
            status_reason: None,
            connected_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn linkedin_identity_only_is_not_search_or_network_access() {
        let a = account(&["openid", "profile", "email"], AccountStatus::Connected);
        let caps = of(ProviderId::Linkedin, Some(&a), true, true);
        assert_eq!(caps.access, ProviderAccess::Connected);
        assert!(caps.has(NetworkCapability::AuthenticateIdentity));
        assert!(caps.has(NetworkCapability::ReadSelfProfile));
        for missing in [
            NetworkCapability::ReadFirstDegreeConnections,
            NetworkCapability::ReadSecondDegreeConnections,
            NetworkCapability::SearchPeople,
            NetworkCapability::SearchCompanies,
            NetworkCapability::PersistentStorageAllowed,
        ] {
            assert!(!caps.has(missing), "{missing:?}");
        }
        assert!(caps
            .summary
            .contains("does not currently have permission to read your connection list"));
        assert_eq!(
            relationship_access(&[caps]),
            RelationshipAccess::Unavailable(
                "LinkedIn is connected for identity, but ReMa does not currently have \
                 permission to read your connection list, so ReMa cannot tell whom you know."
                    .into()
            )
        );
    }

    #[test]
    fn linkedin_first_degree_only_with_the_granted_permission() {
        let a = account(
            &["openid", "profile", "email", "r_1st_connections"],
            AccountStatus::Connected,
        );
        let caps = of(ProviderId::Linkedin, Some(&a), true, true);
        assert!(caps.has(NetworkCapability::ReadFirstDegreeConnections));
        assert!(!caps.has(NetworkCapability::ReadSecondDegreeConnections));
        assert_eq!(
            relationship_access(&[caps]),
            RelationshipAccess::Available(ProviderId::Linkedin)
        );
        // Revoked: nothing is available, whatever was granted before.
        let expired = account(
            &["openid", "r_1st_connections"],
            AccountStatus::ReauthRequired,
        );
        let caps = of(ProviderId::Linkedin, Some(&expired), true, true);
        assert_eq!(caps.access, ProviderAccess::ReconnectNeeded);
        assert!(caps.available.is_empty());
    }

    #[test]
    fn disconnected_and_xing_have_no_capabilities() {
        let caps = of(ProviderId::Linkedin, None, false, true);
        assert_eq!(caps.access, ProviderAccess::NotConnected);
        assert!(caps.available.is_empty());
        let xing = of(ProviderId::Xing, None, false, false);
        assert_eq!(xing.access, ProviderAccess::NotAvailable);
        assert!(xing.available.is_empty());
        assert!(xing.summary.contains("no sign-in for desktop apps"));
        assert!(matches!(
            relationship_access(&[caps, xing]),
            RelationshipAccess::Unavailable(reason) if reason.contains("cannot tell whom you know")
        ));
    }
}
