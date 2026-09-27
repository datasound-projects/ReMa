//! ReMa's side of ReMa MCP: an in-process session per chat answer. The
//! server and ReMa's MCP client talk JSON-RPC over an in-memory stream
//! (the same framing as stdio), so every call crosses the real MCP
//! boundary. No process, port or file is involved.

use std::sync::Arc;

use rmcp::ServiceExt;
use serde::Serialize;
use specta::Type;
use tokio_util::sync::CancellationToken;

use super::{
    engine::{Discovery, Session},
    server::RemaMcpServer,
    sources, store,
};
use crate::{
    error::{AppError, AppResult},
    llm::Endpoint,
    mcp::client::{self, Connection},
    state::AppState,
};

/// A chat's session with ReMa MCP; closed when dropped.
pub struct Hosted {
    pub connection: Arc<Connection>,
    cancel: CancellationToken,
}

impl Drop for Hosted {
    fn drop(&mut self) {
        self.connection.close();
        self.cancel.cancel();
    }
}

/// Opens a session: `endpoint` is the chat's model, whose own web search
/// adds to ReMa's job sources.
pub async fn open(
    state: &AppState,
    endpoint: Option<(&Endpoint, &str)>,
    cancel: &CancellationToken,
) -> Result<Hosted, String> {
    let discovery = Discovery::for_chat(state, endpoint).await;
    let session = Session {
        state: state.clone(),
        discovery,
    };
    let cancel = cancel.child_token();
    let (server_io, client_io) = tokio::io::duplex(256 * 1024);
    let server = RemaMcpServer::new(session, cancel.clone());
    let stop = cancel.clone();
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            tokio::select! {
                _ = stop.cancelled() => {}
                _ = running.waiting() => {}
            }
        }
    });
    let connection = client::connect_stream(client_io, &state.info.version)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Hosted {
        connection: Arc::new(connection),
        cancel,
    })
}

// ── Settings ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    /// Searching works (ReMa's own job sources need no setup).
    Ready,
    /// Recent source requests failed to connect.
    Offline,
    /// The built-in server did not answer.
    Error,
    /// Turned off.
    Disabled,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    pub name: String,
    /// "API", "Feed", "Page", "Links only".
    pub access: String,
    pub usable: bool,
    pub note: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RemaMcpStatus {
    pub enabled: bool,
    pub readiness: Readiness,
    pub message: String,
    /// The search backend new chats use by default.
    pub backend: Option<String>,
    /// Tool names listed over MCP (empty when not checked or disabled).
    pub tools: Vec<String>,
    pub sources: Vec<SourceSummary>,
    pub cached_jobs: u32,
    pub cached_searches: u32,
}

/// The default chat model and its endpoint, when one is set up.
async fn default_endpoint(state: &AppState) -> Option<(Endpoint, String)> {
    let model = crate::services::providers::catalog(state)
        .ok()?
        .default_model?;
    let endpoint = crate::services::providers::resolve_endpoint(state, &model.provider_id)
        .await
        .ok()?;
    Some((endpoint, model.model_id))
}

fn access_label(mode: super::contract::AcquisitionMode) -> &'static str {
    use super::contract::AcquisitionMode as M;
    match mode {
        M::AuthorizedApi => "API",
        M::DocumentedPublicFeed => "Public API / feed",
        M::PermittedPublicPage => "Page",
        M::SearchDiscoveryOnly => "Links only",
        M::Blocked => "Blocked",
    }
}

/// The card in Settings → MCP → Built-in. `check` opens a session and lists
/// the tools over MCP (a protocol check; nothing is searched).
pub async fn status(state: &AppState, check: bool) -> AppResult<RemaMcpStatus> {
    let enabled = super::is_enabled(state);
    let (jobs, searches) = state.db.call(|c| store::counts(c))?;
    // New chats use the default model; its hosted search is the fallback.
    let default = default_endpoint(state).await;
    let discovery =
        Discovery::for_chat(state, default.as_ref().map(|(e, m)| (e, m.as_str()))).await;
    let mut readiness = Readiness::Ready;
    let mut message = match (&discovery.service, &discovery.provider) {
        (Some(service), _) => format!(
            "Searches ReMa's job sources (employer boards and public job boards) and {}.",
            service.name()
        ),
        (None, Some(_)) => "Searches ReMa's job sources (employer boards and public job boards) \
                            and the chat model's own web search. No setup needed."
            .to_string(),
        (None, None) => "Searches ReMa's job sources: employer boards and public job boards. No \
                         setup needed."
            .to_string(),
    };
    let searchable = discovery.searches_the_web();
    let sources: Vec<SourceSummary> = sources::REGISTRY
        .iter()
        .map(|s| {
            let health = state.rema_mcp.health(s.id);
            SourceSummary {
                name: s.name.into(),
                access: access_label(s.mode).into(),
                usable: searchable
                    || s.mode != super::contract::AcquisitionMode::SearchDiscoveryOnly,
                note: s.restrictions.into(),
                last_error: health
                    .last_error
                    .filter(|(at, _)| health.last_success.is_none_or(|ok| ok < *at))
                    .map(|(_, m)| m),
            }
        })
        .collect();
    let offline = sources
        .iter()
        .filter_map(|s| s.last_error.as_deref())
        .filter(|e| e.contains("could not connect"))
        .count()
        >= 2;
    if offline {
        readiness = Readiness::Offline;
        message = "Recent requests to job sources could not connect. Check the internet \
                   connection."
            .into();
    }
    let mut tools = Vec::new();
    if enabled && check {
        match open(state, None, &CancellationToken::new()).await {
            Ok(hosted) => {
                tools = hosted
                    .connection
                    .tools
                    .iter()
                    .map(|t| t.name.to_string())
                    .collect()
            }
            Err(error) => {
                readiness = Readiness::Error;
                message = format!("The built-in server did not start: {error}");
            }
        }
    }
    if !enabled {
        readiness = Readiness::Disabled;
        message = "Turned off. Its job-search tools are not offered in chats.".into();
    }
    Ok(RemaMcpStatus {
        enabled,
        readiness,
        message,
        backend: discovery.name(),
        tools,
        sources,
        cached_jobs: jobs as u32,
        cached_searches: searches as u32,
    })
}

pub async fn set_enabled(state: &AppState, enabled: bool) -> AppResult<RemaMcpStatus> {
    super::set_enabled(state, enabled)?;
    state.events.mcp_changed();
    status(state, false).await
}

/// Clears cached jobs and searches (never chats, Analytics or Profile).
pub async fn clear_cache(state: &AppState) -> AppResult<RemaMcpStatus> {
    state.db.call(|c| store::clear(c))?;
    state.rema_mcp.feeds().clear();
    status(state, false).await
}

pub fn not_enabled() -> AppError {
    AppError::validation("ReMa MCP is turned off")
}
