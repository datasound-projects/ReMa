//! The unified tool registry: every tool ReMa can give a model, under a
//! canonical id that does not depend on the model or provider that runs the
//! chat (`gmail.search`, `calendar.list_events`, `mcp.<server>.<tool>`).
//!
//! The registry is the contract of ReMa's tool router:
//!
//! - **Definitions** are ReMa's ([`crate::llm::ToolSpec`]); the provider
//!   adapters (Anthropic, OpenAI, Gemini, Codex) only translate them into
//!   each API's shape. Switching models changes nothing about which tools
//!   exist or how they run.
//! - **Execution** happens inside ReMa ([`crate::services::connector_tools`]
//!   for accounts, [`crate::services::chat_tools`] for MCP servers). A model
//!   receives the definition and the sanitized result, never a token, a
//!   refresh token, an authorization code or a client credential.
//! - **Model-facing names** are the stable wire names below (the ones in
//!   conversation history); the canonical id is how ReMa names the same
//!   tool across accounts: `mail.search` runs against Gmail and Outlook
//!   alike, so the canonical mail and calendar ids are provider-neutral
//!   and the provider-specific aliases (`gmail.search`,
//!   `outlook.search`) map onto them.

use crate::models::{
    chat::ChatConnector,
    connectors::{ConnectorId, ConnectorKind},
};

use super::connector_tools::{
    APPLICATIONS_APPEND_TIMELINE_EVENT, APPLICATIONS_FIND_MATCH, APPLICATIONS_UPDATE_STATUS,
    CALENDAR_CHECK_AVAILABILITY, CALENDAR_CREATE_EVENT, CALENDAR_LIST_EVENTS,
    CALENDAR_UPDATE_EVENT, MAIL_GET_MESSAGE, MAIL_GET_THREAD, MAIL_SEARCH,
};

/// Where a tool runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    /// A connected account of this kind (every provider of the kind).
    Connector(ConnectorKind),
    /// ReMa's application tracker (local data).
    Applications,
}

/// One tool of the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolRoute {
    /// The canonical id (`mail.search`).
    pub canonical: &'static str,
    /// The name a model sees (`mail_search`); stable, in `[a-z0-9_]`.
    pub model_name: &'static str,
    pub source: ToolSource,
    /// The tool only reads (no approval needed).
    pub read_only: bool,
    /// The chat toggle that turns it on.
    pub chat_connector: ChatConnector,
}

/// Every account-backed tool, in the order models are offered them.
pub const ROUTES: [ToolRoute; 10] = [
    ToolRoute {
        canonical: "mail.search",
        model_name: MAIL_SEARCH,
        source: ToolSource::Connector(ConnectorKind::Mail),
        read_only: true,
        chat_connector: ChatConnector::Gmail,
    },
    ToolRoute {
        canonical: "mail.get_message",
        model_name: MAIL_GET_MESSAGE,
        source: ToolSource::Connector(ConnectorKind::Mail),
        read_only: true,
        chat_connector: ChatConnector::Gmail,
    },
    ToolRoute {
        canonical: "mail.get_thread",
        model_name: MAIL_GET_THREAD,
        source: ToolSource::Connector(ConnectorKind::Mail),
        read_only: true,
        chat_connector: ChatConnector::Gmail,
    },
    ToolRoute {
        canonical: "calendar.list_events",
        model_name: CALENDAR_LIST_EVENTS,
        source: ToolSource::Connector(ConnectorKind::Calendar),
        read_only: true,
        chat_connector: ChatConnector::GoogleCalendar,
    },
    ToolRoute {
        canonical: "calendar.check_availability",
        model_name: CALENDAR_CHECK_AVAILABILITY,
        source: ToolSource::Connector(ConnectorKind::Calendar),
        read_only: true,
        chat_connector: ChatConnector::GoogleCalendar,
    },
    ToolRoute {
        canonical: "calendar.create_event",
        model_name: CALENDAR_CREATE_EVENT,
        source: ToolSource::Connector(ConnectorKind::Calendar),
        read_only: false,
        chat_connector: ChatConnector::GoogleCalendar,
    },
    ToolRoute {
        canonical: "calendar.update_event",
        model_name: CALENDAR_UPDATE_EVENT,
        source: ToolSource::Connector(ConnectorKind::Calendar),
        read_only: false,
        chat_connector: ChatConnector::GoogleCalendar,
    },
    ToolRoute {
        canonical: "applications.find_match",
        model_name: APPLICATIONS_FIND_MATCH,
        source: ToolSource::Applications,
        read_only: true,
        chat_connector: ChatConnector::Applications,
    },
    ToolRoute {
        canonical: "applications.update_status",
        model_name: APPLICATIONS_UPDATE_STATUS,
        source: ToolSource::Applications,
        read_only: false,
        chat_connector: ChatConnector::Applications,
    },
    ToolRoute {
        canonical: "applications.append_timeline_event",
        model_name: APPLICATIONS_APPEND_TIMELINE_EVENT,
        source: ToolSource::Applications,
        read_only: false,
        chat_connector: ChatConnector::Applications,
    },
];

/// The route of a model-facing name.
pub fn route_of(model_name: &str) -> Option<&'static ToolRoute> {
    ROUTES.iter().find(|r| r.model_name == model_name)
}

/// The canonical id of a model-facing name, or the id itself for an alias.
/// Provider aliases (`gmail.search`, `outlook.search`, `google_calendar.
/// list_events`, `outlook_calendar.list_events`) resolve to the neutral id.
pub fn canonical(name: &str) -> Option<&'static str> {
    if let Some(route) = route_of(name) {
        return Some(route.canonical);
    }
    let (prefix, rest) = name.split_once('.')?;
    let kind = match prefix {
        "gmail" | "outlook" | "outlook_mail" => "mail",
        "google_calendar" | "outlook_calendar" | "calendar" => "calendar",
        "applications" => "applications",
        "mail" => "mail",
        _ => return None,
    };
    ROUTES
        .iter()
        .find(|r| r.canonical == format!("{kind}.{rest}"))
        .map(|r| r.canonical)
}

/// The canonical id of an MCP server's tool (`mcp.<server>.<tool>`); the
/// model-facing name is [`super::chat_tools::exposed_name`].
pub fn mcp_canonical(server: &str, tool: &str) -> String {
    let slug: String = server
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    format!(
        "mcp.{}.{tool}",
        if slug.is_empty() { "server" } else { &slug }
    )
}

/// The connectors whose account a route runs against.
pub fn providers_of(route: &ToolRoute) -> Vec<ConnectorId> {
    match route.source {
        ToolSource::Connector(kind) => ConnectorId::ALL
            .into_iter()
            .filter(|id| id.kind() == kind)
            .collect(),
        ToolSource::Applications => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_ids_and_model_names_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for route in ROUTES {
            assert!(seen.insert(route.canonical), "{}", route.canonical);
            assert!(seen.insert(route.model_name), "{}", route.model_name);
            assert!(route.canonical.contains('.'));
            assert!(route
                .model_name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
            assert_eq!(route.canonical.replace('.', "_"), route.model_name);
        }
    }

    #[test]
    fn provider_aliases_resolve_to_the_neutral_tool() {
        assert_eq!(canonical("gmail.search"), Some("mail.search"));
        assert_eq!(canonical("outlook.search"), Some("mail.search"));
        assert_eq!(canonical("mail_search"), Some("mail.search"));
        assert_eq!(
            canonical("google_calendar.list_events"),
            Some("calendar.list_events")
        );
        assert_eq!(
            canonical("outlook_calendar.create_event"),
            Some("calendar.create_event")
        );
        assert_eq!(canonical("linkedin.search"), None);
        assert_eq!(canonical("mail.send"), None);
    }

    #[test]
    fn mail_and_calendar_tools_run_against_every_provider_of_their_kind() {
        let mail = route_of(MAIL_SEARCH).unwrap();
        assert_eq!(
            providers_of(mail),
            [ConnectorId::Gmail, ConnectorId::OutlookMail]
        );
        let calendar = route_of(CALENDAR_LIST_EVENTS).unwrap();
        assert_eq!(
            providers_of(calendar),
            [ConnectorId::GoogleCalendar, ConnectorId::OutlookCalendar]
        );
        assert!(providers_of(route_of(APPLICATIONS_FIND_MATCH).unwrap()).is_empty());
    }

    #[test]
    fn mcp_tools_get_a_server_scoped_id() {
        assert_eq!(
            mcp_canonical("Linear MCP", "list_issues"),
            "mcp.linear_mcp.list_issues"
        );
        assert_eq!(mcp_canonical("", "x"), "mcp.server.x");
    }

    #[test]
    fn writes_need_approval_and_reads_do_not() {
        for route in ROUTES {
            let writes = route.canonical.contains("create")
                || route.canonical.contains("update")
                || route.canonical.contains("append");
            assert_eq!(!route.read_only, writes, "{}", route.canonical);
        }
    }
}
