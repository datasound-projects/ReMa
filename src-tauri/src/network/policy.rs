//! The provider data policy (NC §12, §39, §40; B28, B29): one place that
//! decides what ReMa may do with each kind of data from each source, for
//! each purpose. Callers ask before they fetch, show, send to a model
//! (local models included), store or derive; nothing relies on a developer
//! remembering a provider's terms.
//!
//! The rules follow the providers' published terms as reviewed on
//! [`REVIEWED_AT`]:
//!
//! - **LinkedIn API** (self-serve sign-in; Connections API only with
//!   LinkedIn's approval): the member's own identity may be kept as the
//!   connector account. First-degree connection data is shown for the
//!   session only, matched by ReMa, never sent to a model, never stored,
//!   never exported; and only for professional research. A login is not a
//!   sales integration: every Business purpose is denied.
//! - **XING API**: no integration exists for ReMa; member data would be
//!   denied for storage, social-graph use and (Business) marketing use.
//! - **Public sources** (company sites, public job postings, Wikidata,
//!   search results): company and job facts are research output and may be
//!   kept; public professional information about people (name, current
//!   title, public profile link) may be used for professional research and
//!   for identifying relevant buyer roles, never private contact details.
//! - **User-entered** data is the user's own.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};

/// The version of these rules; stored with research that relied on them.
pub const POLICY_VERSION: &str = "2026-09-27";
/// When the provider terms behind the rules were last reviewed.
pub const REVIEWED_AT: &str = "2026-09-27";

/// Why data is used (B28).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    ProfessionalResearch,
    ClientAcquisition,
    ContractSearch,
    GtmResearch,
}

impl Purpose {
    pub fn is_commercial(self) -> bool {
        self != Self::ProfessionalResearch
    }
}

/// What is done with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// ReMa requests it from the source.
    Fetch,
    /// Shown on screen.
    Display,
    /// Sent to a model (cloud or local).
    ModelProcess,
    /// Kept beyond the session: chat and run history, saved records.
    Store,
    /// Used to compute something else (a match, a score).
    Derive,
    /// Copied out of ReMa (drafts, exports, clipboard).
    Export,
}

/// Where data comes from.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    LinkedinApi,
    XingApi,
    /// A public page found or read on the web.
    PublicWeb,
    /// The company's own site.
    CompanyWebsite,
    /// A job posting from ReMa's job layer (Jobs MCP).
    JobsMcp,
    Wikidata,
    /// A page a model's hosted web search reported.
    ModelWebSearch,
    UserEntered,
}

impl DataSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::LinkedinApi => "LinkedIn (your connection)",
            Self::XingApi => "XING",
            Self::PublicWeb => "Public web page",
            Self::CompanyWebsite => "Company website",
            Self::JobsMcp => "Job posting",
            Self::Wikidata => "Wikidata",
            Self::ModelWebSearch => "Web search",
            Self::UserEntered => "Entered by you",
        }
    }

    /// Data an authenticated provider API returned about members.
    pub fn is_provider_api(self) -> bool {
        matches!(self, Self::LinkedinApi | Self::XingApi)
    }
}

/// What kind of data it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    /// The user's own account identity.
    Identity,
    /// The user's own profile.
    SelfProfile,
    /// A member of the user's first-degree network.
    FirstDegreeConnection,
    /// A person in a professional role (name, title, public profile).
    ProfessionalProfile,
    Company,
    Job,
}

/// How long data may be kept (NC §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Persistence {
    /// Not kept at all.
    Ephemeral,
    /// In memory while ReMa runs, cleared on disconnect and exit.
    Session,
    /// Cached briefly.
    ShortTtl,
    /// May be kept as ReMa records (chat, run history, saved items).
    PersistentPermitted,
}

/// Where data may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    Screen,
    Model,
    LocalDatabase,
    RunHistory,
    Clipboard,
}

/// One policy answer (B28: allowed, reason, expiry, destinations).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub allowed: bool,
    pub reason: String,
    pub persistence: Persistence,
    /// Longest time it may be cached, in seconds (None: not cached, or no
    /// limit for persistent data).
    pub max_age_secs: Option<u32>,
    pub destinations: Vec<Destination>,
    /// The terms the rule rests on.
    pub agreement: String,
    pub policy_version: String,
}

/// Session data from LinkedIn is dropped after this.
pub const SESSION_TTL_SECS: u32 = 30 * 60;

fn agreement(source: DataSource) -> String {
    match source {
        DataSource::LinkedinApi => format!(
            "LinkedIn API Terms of Use; self-serve Sign In with LinkedIn; Connections API \
             restricted to approved developers (reviewed {REVIEWED_AT})"
        ),
        DataSource::XingApi => format!(
            "XING API Terms and Conditions: storage, social-graph and marketing restrictions \
             (reviewed {REVIEWED_AT})"
        ),
        DataSource::Wikidata => "Wikidata: CC0 public data".into(),
        DataSource::JobsMcp => "ReMa Jobs source registry (documented public feeds)".into(),
        DataSource::CompanyWebsite | DataSource::PublicWeb | DataSource::ModelWebSearch => {
            "Public pages read within robots rules; no authenticated pages".into()
        }
        DataSource::UserEntered => "The user's own data".into(),
    }
}

fn decision(
    source: DataSource,
    allowed: bool,
    reason: impl Into<String>,
    persistence: Persistence,
    destinations: &[Destination],
) -> Decision {
    Decision {
        allowed,
        reason: reason.into(),
        persistence: if allowed {
            persistence
        } else {
            Persistence::Ephemeral
        },
        max_age_secs: match (allowed, persistence) {
            (true, Persistence::Session) => Some(SESSION_TTL_SECS),
            (true, Persistence::ShortTtl) => Some(24 * 3600),
            _ => None,
        },
        destinations: if allowed {
            destinations.to_vec()
        } else {
            Vec::new()
        },
        agreement: agreement(source),
        policy_version: POLICY_VERSION.to_string(),
    }
}

/// The policy for one use of one kind of data.
pub fn check(
    source: DataSource,
    class: DataClass,
    purpose: Purpose,
    operation: Operation,
) -> Decision {
    use Destination::*;
    use Operation::*;
    let all = [Screen, Model, LocalDatabase, RunHistory, Clipboard];
    match source {
        DataSource::LinkedinApi => linkedin(class, purpose, operation),
        DataSource::XingApi => decision(
            source,
            false,
            if purpose.is_commercial() {
                "XING member data may not be used for client acquisition, outreach or GTM \
                 enrichment without an agreement that permits it."
            } else {
                "XING offers ReMa no API access; XING's terms restrict storing member profiles \
                 and social-graph data."
            },
            Persistence::Ephemeral,
            &[],
        ),
        DataSource::UserEntered => decision(
            source,
            true,
            "Your own data.",
            Persistence::PersistentPermitted,
            &all,
        ),
        // Public sources.
        _ => match class {
            DataClass::Company | DataClass::Job => decision(
                source,
                true,
                "Public company and job information.",
                Persistence::PersistentPermitted,
                &all,
            ),
            DataClass::ProfessionalProfile => {
                if operation == Export && purpose.is_commercial() {
                    // A draft may address a role or a named professional,
                    // never with harvested contact details.
                    decision(
                        source,
                        true,
                        "Public professional name and title only; no private contact details.",
                        Persistence::PersistentPermitted,
                        &[Screen, Clipboard],
                    )
                } else {
                    decision(
                        source,
                        true,
                        "Public professional information (name, current title, public profile \
                         link) for research; contact details are never collected.",
                        Persistence::PersistentPermitted,
                        &all,
                    )
                }
            }
            DataClass::Identity | DataClass::SelfProfile | DataClass::FirstDegreeConnection => {
                decision(
                    source,
                    false,
                    "Relationship and account data only come from a connected provider.",
                    Persistence::Ephemeral,
                    &[],
                )
            }
        },
    }
}

fn linkedin(class: DataClass, purpose: Purpose, operation: Operation) -> Decision {
    use Destination::*;
    use Operation::*;
    let source = DataSource::LinkedinApi;
    if purpose.is_commercial() {
        return decision(
            source,
            false,
            "A LinkedIn sign-in does not authorize commercial lead enrichment; LinkedIn data is \
             not used in Business.",
            Persistence::Ephemeral,
            &[],
        );
    }
    match class {
        DataClass::Identity | DataClass::SelfProfile => match operation {
            Fetch | Display | Derive => decision(
                source,
                true,
                "Your own account, shown in Connectors.",
                Persistence::PersistentPermitted,
                &[Screen],
            ),
            // The connector account (name, email) is the only record kept.
            Store => decision(
                source,
                true,
                "Kept as your connector account only.",
                Persistence::PersistentPermitted,
                &[LocalDatabase],
            ),
            ModelProcess | Export => decision(
                source,
                false,
                "ReMa does not send your LinkedIn account details to a model or elsewhere.",
                Persistence::Ephemeral,
                &[],
            ),
        },
        DataClass::FirstDegreeConnection => match operation {
            Fetch | Display | Derive => decision(
                source,
                true,
                "Your first-degree connections, shown to you for this session only.",
                Persistence::Session,
                &[Screen],
            ),
            ModelProcess => decision(
                source,
                false,
                "Connection data from LinkedIn is matched by ReMa and never sent to a model.",
                Persistence::Ephemeral,
                &[],
            ),
            Store => decision(
                source,
                false,
                "Connection data from LinkedIn is not stored: not in chats, run history or \
                 saved records.",
                Persistence::Ephemeral,
                &[],
            ),
            Export => decision(
                source,
                false,
                "Connection data from LinkedIn is not exported.",
                Persistence::Ephemeral,
                &[],
            ),
        },
        DataClass::ProfessionalProfile | DataClass::Company | DataClass::Job => decision(
            source,
            false,
            "LinkedIn grants ReMa no member, company or job search.",
            Persistence::Ephemeral,
            &[],
        ),
    }
}

/// The policy, or an error naming why the use is not allowed.
pub fn require(
    source: DataSource,
    class: DataClass,
    purpose: Purpose,
    operation: Operation,
) -> AppResult<Decision> {
    let decision = check(source, class, purpose, operation);
    if decision.allowed {
        Ok(decision)
    } else {
        Err(AppError::permission(decision.reason))
    }
}

/// The policy summary per source and class (NC §39's fields), for the
/// "how ReMa uses network data" details and for tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDataPolicy {
    pub source: DataSource,
    pub class: DataClass,
    pub may_fetch: bool,
    pub may_display: bool,
    pub may_cache: bool,
    pub max_cache_ttl_secs: Option<u32>,
    pub may_persist: bool,
    pub may_send_to_model: bool,
    pub persistence: Persistence,
}

pub fn summary(source: DataSource, class: DataClass, purpose: Purpose) -> ProviderDataPolicy {
    let fetch = check(source, class, purpose, Operation::Fetch);
    let display = check(source, class, purpose, Operation::Display);
    let store = check(source, class, purpose, Operation::Store);
    let model = check(source, class, purpose, Operation::ModelProcess);
    ProviderDataPolicy {
        source,
        class,
        may_fetch: fetch.allowed,
        may_display: display.allowed,
        may_cache: display.allowed && display.persistence > Persistence::Ephemeral,
        max_cache_ttl_secs: display.max_age_secs,
        may_persist: store.allowed,
        may_send_to_model: model.allowed,
        persistence: display.persistence.max(store.persistence),
    }
}

/// Items whose use is allowed; the rest are counted, not silently lost.
pub fn keep<T>(
    items: Vec<T>,
    purpose: Purpose,
    operation: Operation,
    of: impl Fn(&T) -> (DataSource, DataClass),
) -> (Vec<T>, usize) {
    let before = items.len();
    let kept: Vec<T> = items
        .into_iter()
        .filter(|item| {
            let (source, class) = of(item);
            check(source, class, purpose, operation).allowed
        })
        .collect();
    let removed = before - kept.len();
    (kept, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_OPERATIONS: [Operation; 6] = [
        Operation::Fetch,
        Operation::Display,
        Operation::ModelProcess,
        Operation::Store,
        Operation::Derive,
        Operation::Export,
    ];
    const COMMERCIAL: [Purpose; 3] = [
        Purpose::ClientAcquisition,
        Purpose::ContractSearch,
        Purpose::GtmResearch,
    ];

    #[test]
    fn linkedin_connections_are_shown_for_the_session_and_never_stored_or_sent() {
        let p = summary(
            DataSource::LinkedinApi,
            DataClass::FirstDegreeConnection,
            Purpose::ProfessionalResearch,
        );
        assert!(p.may_fetch && p.may_display);
        assert!(!p.may_persist && !p.may_send_to_model);
        assert_eq!(p.persistence, Persistence::Session);
        assert_eq!(p.max_cache_ttl_secs, Some(SESSION_TTL_SECS));
        assert!(require(
            DataSource::LinkedinApi,
            DataClass::FirstDegreeConnection,
            Purpose::ProfessionalResearch,
            Operation::Store
        )
        .is_err());
    }

    #[test]
    fn a_linkedin_login_does_not_authorize_sales_enrichment() {
        for purpose in COMMERCIAL {
            for class in [
                DataClass::Identity,
                DataClass::FirstDegreeConnection,
                DataClass::ProfessionalProfile,
            ] {
                for operation in ALL_OPERATIONS {
                    let d = check(DataSource::LinkedinApi, class, purpose, operation);
                    assert!(!d.allowed, "{purpose:?} {class:?} {operation:?}");
                    assert!(d.destinations.is_empty());
                }
            }
        }
    }

    #[test]
    fn xing_member_data_is_denied_for_every_purpose() {
        for purpose in [Purpose::ProfessionalResearch, Purpose::ClientAcquisition] {
            for operation in ALL_OPERATIONS {
                let d = check(
                    DataSource::XingApi,
                    DataClass::ProfessionalProfile,
                    purpose,
                    operation,
                );
                assert!(!d.allowed);
                assert_eq!(d.persistence, Persistence::Ephemeral);
            }
        }
        let commercial = check(
            DataSource::XingApi,
            DataClass::ProfessionalProfile,
            Purpose::ClientAcquisition,
            Operation::ModelProcess,
        );
        assert!(commercial.reason.contains("client acquisition"));
    }

    #[test]
    fn public_professional_information_is_research_output() {
        let p = summary(
            DataSource::CompanyWebsite,
            DataClass::ProfessionalProfile,
            Purpose::ProfessionalResearch,
        );
        assert!(p.may_fetch && p.may_display && p.may_persist && p.may_send_to_model);
        let draft = check(
            DataSource::PublicWeb,
            DataClass::ProfessionalProfile,
            Purpose::GtmResearch,
            Operation::Export,
        );
        assert!(draft.allowed);
        assert!(!draft.destinations.contains(&Destination::Model));
        // Relationship data never comes from public pages.
        assert!(
            !check(
                DataSource::PublicWeb,
                DataClass::FirstDegreeConnection,
                Purpose::ProfessionalResearch,
                Operation::Display
            )
            .allowed
        );
        let decision = check(
            DataSource::Wikidata,
            DataClass::Company,
            Purpose::ClientAcquisition,
            Operation::Store,
        );
        assert!(decision.allowed);
        assert_eq!(decision.policy_version, POLICY_VERSION);
    }

    #[test]
    fn keep_filters_and_counts() {
        let items = vec![
            (DataSource::LinkedinApi, DataClass::FirstDegreeConnection),
            (DataSource::CompanyWebsite, DataClass::ProfessionalProfile),
            (DataSource::XingApi, DataClass::ProfessionalProfile),
        ];
        let (kept, removed) = keep(
            items,
            Purpose::ProfessionalResearch,
            Operation::ModelProcess,
            |i| *i,
        );
        assert_eq!(
            kept,
            [(DataSource::CompanyWebsite, DataClass::ProfessionalProfile)]
        );
        assert_eq!(removed, 2);
    }
}
