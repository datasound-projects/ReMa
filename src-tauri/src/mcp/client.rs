//! One connection to an MCP server through the official Rust SDK (`rmcp`).
//!
//! The client speaks the current protocol revision (2026-07-28: per-request
//! metadata, `server/discover`) and falls back to the `initialize` handshake
//! for servers on earlier revisions, as the specification's backward
//! compatibility rules describe.

use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ContentBlock, Implementation,
        InitializeRequestParams, ProtocolVersion, Tool,
    },
    service::{ClientInitializeError, RunningService, ServiceError},
    transport::{
        auth::{AuthClient, AuthorizationManager, CredentialStore},
        streamable_http_client::{StreamableHttpClientTransportConfig, StreamableHttpError},
        StreamableHttpClientTransport, TokioChildProcess,
    },
    ClientLifecycleMode, ClientServiceExt, RoleClient,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use super::{config::LEGACY_NO_OAUTH, legacy_sse};
use crate::{
    accounts::locate,
    models::mcp::{McpAuth, McpToolInfo, McpTransport},
};

/// How long starting a server and listing its tools may take (a first
/// `npx -y` or `uvx` run downloads the server).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
/// How long one tool call may take. Long-running tools (builds, crawls)
/// are common; the user can stop an answer at any time.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Largest tool result given to a model (characters).
const MAX_RESULT_CHARS: usize = 40_000;
/// What a call gets once the server went away (its event stream ended,
/// its process exited): nothing is sent, and `McpContext::connect` opens
/// a new connection for the next request.
pub const CONNECTION_LOST: &str =
    "The connection to the server was lost; the next request opens a new one.";

/// Everything needed to connect, secrets included (never logged).
#[derive(Clone)]
pub struct ServerConfig {
    pub id: i64,
    pub name: String,
    pub transport: McpTransport,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: String,
    pub url: String,
    pub auth: McpAuth,
    pub header_name: String,
    pub secret: Option<String>,
}

/// Why a connection could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// OAuth: sign in first.
    NeedsSignIn(String),
    Failed(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedsSignIn(message) | Self::Failed(message) => f.write_str(message),
        }
    }
}

type Service = RunningService<RoleClient, InitializeRequestParams>;

/// A live connection.
pub struct Connection {
    service: Service,
    pub tools: Vec<Tool>,
    pub protocol_version: Option<String>,
    pub server_info: Option<String>,
}

impl Connection {
    /// Closed by ReMa, or the server went away (its process exited).
    pub fn is_closed(&self) -> bool {
        self.service.is_closed() || self.service.peer().is_transport_closed()
    }

    pub fn tool_infos(&self) -> Vec<McpToolInfo> {
        self.tools.iter().map(tool_info).collect()
    }

    /// Calls a tool; the result is flattened to text for the model.
    pub async fn call(
        &self,
        tool: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
        cancel: &CancellationToken,
    ) -> Result<(String, bool), String> {
        if self.is_closed() {
            return Err(CONNECTION_LOST.into());
        }
        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        tokio::select! {
            _ = cancel.cancelled() => Err("The request was stopped.".into()),
            result = tokio::time::timeout(CALL_TIMEOUT, self.service.call_tool(params)) => match result {
                Err(_) => Err(format!("The tool did not answer within {} seconds.", CALL_TIMEOUT.as_secs())),
                // rmcp fails every waiting call this way when the transport
                // ends (`receive` yielding `None`).
                Ok(Err(ServiceError::TransportClosed)) => Err(CONNECTION_LOST.into()),
                Ok(Err(error)) => Err(format!("The tool call failed: {}", short(&error.to_string()))),
                Ok(Ok(result)) => Ok(flatten(&result)),
            },
        }
    }

    /// Stops the connection (and a local server's process group).
    pub fn close(&self) {
        self.service.cancellation_token().cancel();
    }
}

pub fn tool_info(tool: &Tool) -> McpToolInfo {
    let annotations = tool.annotations.as_ref();
    McpToolInfo {
        name: tool.name.to_string(),
        title: tool
            .title
            .clone()
            .or_else(|| annotations.and_then(|a| a.title.clone())),
        description: tool
            .description
            .as_deref()
            .unwrap_or_default()
            .chars()
            .take(1_000)
            .collect(),
        // Only an explicit read-only hint counts; destructive wins.
        read_only: annotations
            .is_some_and(|a| a.read_only_hint == Some(true) && a.destructive_hint != Some(true)),
    }
}

fn short(message: &str) -> String {
    let line = message.lines().next().unwrap_or_default();
    line.chars().take(300).collect()
}

/// A tool result as text: text blocks as they are, other content named.
fn flatten(result: &CallToolResult) -> (String, bool) {
    let mut parts: Vec<String> = Vec::new();
    for block in &result.content {
        match block {
            ContentBlock::Text(text) => parts.push(text.text.clone()),
            ContentBlock::Image(image) => parts.push(format!("[image: {}]", image.mime_type)),
            ContentBlock::Audio(audio) => parts.push(format!("[audio: {}]", audio.mime_type)),
            ContentBlock::Resource(resource) => parts.push(
                serde_json::to_string(&resource.resource).unwrap_or_else(|_| "[resource]".into()),
            ),
            ContentBlock::ResourceLink(link) => {
                parts.push(format!("[resource: {} {}]", link.name, link.uri))
            }
            _ => parts.push("[unsupported content]".into()),
        }
    }
    if parts.is_empty() {
        if let Some(structured) = &result.structured_content {
            parts.push(structured.to_string());
        }
    }
    let mut text = parts.join("\n\n");
    if text.chars().count() > MAX_RESULT_CHARS {
        text = text.chars().take(MAX_RESULT_CHARS).collect::<String>() + "\n[… result shortened]";
    }
    (text, result.is_error == Some(true))
}

fn client_info(version: &str) -> InitializeRequestParams {
    InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("ReMa", version.to_string()),
    )
}

fn lifecycle() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        legacy_version: Some(ProtocolVersion::V_2025_11_25),
    }
}

/// The last lines a local server printed on stderr (to explain failures).
#[derive(Clone, Default)]
pub struct StderrTail(Arc<Mutex<VecDeque<String>>>);

impl StderrTail {
    fn watch(&self, stderr: tokio::process::ChildStderr) {
        let tail = self.0.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut tail = tail.lock().unwrap();
                tail.push_back(line.chars().take(300).collect());
                if tail.len() > 20 {
                    tail.pop_front();
                }
            }
        });
    }

    fn hint(&self) -> Option<String> {
        let tail = self.0.lock().unwrap();
        tail.iter()
            .rev()
            .find(|l| {
                let lower = l.to_lowercase();
                lower.contains("error") || lower.contains("not found") || lower.contains("denied")
            })
            .or(tail.back())
            .map(|l| l.trim().to_string())
    }
}

async fn spawn_local(
    config: &ServerConfig,
) -> Result<(TokioChildProcess, StderrTail), ConnectError> {
    let located = if config.command.contains('/') || config.command.contains('\\') {
        Some(locate::Located::at(PathBuf::from(&config.command)))
    } else {
        locate::which_as_shell(&config.command).await
    };
    let Some(located) = located else {
        return Err(ConnectError::Failed(format!(
            "“{}” was not found on this computer. Install it, or enter its full path.",
            config.command
        )));
    };

    // Like Claude Code: the server gets ReMa's whole environment, plus what
    // the user's login shell sets up (an app opened from the Finder or the
    // Dock does not have it), then its own variables.
    let mut command = tokio::process::Command::new(&located.path);
    command.args(&config.args);
    let login = locate::login_environment().await;
    for (name, value) in login {
        command.env(name, value);
    }
    // ReMa's own settings (debug and test overrides) stay with ReMa.
    for (name, _) in std::env::vars_os().chain(login.clone()) {
        if name.to_string_lossy().starts_with("REMA_") {
            command.env_remove(name);
        }
    }
    command.env(
        "PATH",
        locate::extended_path_with(
            &located.path_dirs,
            login
                .get(std::ffi::OsStr::new("PATH"))
                .map(|p| p.as_os_str()),
        ),
    );
    for (name, value) in &config.env {
        command.env(name, value);
    }
    let cwd = if config.cwd.is_empty() {
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
    } else {
        PathBuf::from(&config.cwd)
    };
    command.current_dir(cwd);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut wrapped = process_wrap::tokio::CommandWrap::from(command);
    #[cfg(unix)]
    wrapped.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    wrapped.wrap(process_wrap::tokio::JobObject);
    wrapped.wrap(process_wrap::tokio::KillOnDrop);

    let (transport, stderr) = TokioChildProcess::builder(wrapped)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ConnectError::Failed(format!("Could not start “{}”: {e}", config.command)))?;
    let tail = StderrTail::default();
    if let Some(stderr) = stderr {
        tail.watch(stderr);
    }
    Ok((transport, tail))
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("ReMa/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .build()
        .unwrap_or_default()
}

fn http_config(config: &ServerConfig) -> Result<StreamableHttpClientTransportConfig, ConnectError> {
    let mut transport = StreamableHttpClientTransportConfig::with_uri(config.url.clone());
    match (config.auth, &config.secret) {
        (McpAuth::Bearer, Some(token)) => transport = transport.auth_header(token.clone()),
        (McpAuth::Header, Some(value)) => {
            let name = reqwest::header::HeaderName::from_bytes(config.header_name.as_bytes())
                .map_err(|_| ConnectError::Failed("The header name is not valid.".into()))?;
            let mut value = reqwest::header::HeaderValue::from_str(value)
                .map_err(|_| ConnectError::Failed("The header value is not valid.".into()))?;
            value.set_sensitive(true);
            transport = transport.custom_headers(HashMap::from([(name, value)]));
        }
        (McpAuth::Bearer | McpAuth::Header, None) => {
            return Err(ConnectError::Failed(
                "Add the token in the server settings.".into(),
            ))
        }
        _ => {}
    }
    Ok(transport)
}

/// The token headers of a server without OAuth, for the HTTP+SSE session
/// (Streamable HTTP takes them through its own configuration).
fn auth_headers(config: &ServerConfig) -> Result<reqwest::header::HeaderMap, ConnectError> {
    let mut headers = reqwest::header::HeaderMap::new();
    let (name, value) = match (config.auth, &config.secret) {
        (McpAuth::Bearer, Some(token)) => {
            (reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
        }
        (McpAuth::Header, Some(value)) => (
            reqwest::header::HeaderName::from_bytes(config.header_name.as_bytes())
                .map_err(|_| ConnectError::Failed("The header name is not valid.".into()))?,
            value.clone(),
        ),
        _ => return Ok(headers),
    };
    let mut value = reqwest::header::HeaderValue::from_str(&value)
        .map_err(|_| ConnectError::Failed("The header value is not valid.".into()))?;
    value.set_sensitive(true);
    headers.insert(name, value);
    Ok(headers)
}

fn is_unauthorized(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("auth required")
        || lower.contains("authorization required")
        || lower.contains("401")
        || lower.contains("unauthorized")
        // A 403 with a challenge (rmcp's words).
        || lower.contains("insufficient scope")
}

/// The HTTP status of the answer that ended Streamable HTTP's handshake,
/// when it ended on one: rmcp keeps it in the transport error's text
/// ("HTTP 405 Method Not Allowed: …"). Connection failures, timeouts,
/// authentication challenges (401/403 with `WWW-Authenticate`) and answers
/// that were JSON-RPC messages have none.
pub fn handshake_status(error: &ClientInitializeError) -> Option<u16> {
    match error {
        // Discover was refused, then the legacy `initialize`: the latter
        // is the POST the specification's fallback rule is about.
        ClientInitializeError::LegacyFallbackFailed { fallback, .. } => handshake_status(fallback),
        ClientInitializeError::TransportError { error, .. } => {
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error.error.as_ref());
            while let Some(current) = source {
                if let Some(StreamableHttpError::UnexpectedServerResponse(text)) =
                    current.downcast_ref::<StreamableHttpError<reqwest::Error>>()
                {
                    return status_in(text);
                }
                source = current.source();
            }
            None
        }
        _ => None,
    }
}

/// The status in "HTTP 405 Method Not Allowed: …".
fn status_in(text: &str) -> Option<u16> {
    text.strip_prefix("HTTP ")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Whether Streamable HTTP failed the way a server on the older HTTP+SSE
/// transport fails it: a plain 400, 404 or 405 to the POST (the
/// specification's backwards-compatibility rule), so the event stream is
/// worth a GET. Not when the server could not be reached or timed out,
/// answered with a server error, asked for authentication, or answered
/// with a JSON-RPC error (a server that reads JSON-RPC POSTs is not on the
/// older transport).
pub fn legacy_candidate(error: &ClientInitializeError) -> bool {
    matches!(handshake_status(error), Some(400 | 404 | 405))
}

/// What went wrong, in rmcp's words: the transport's own message rather
/// than the wrapper's ("discover and legacy initialize both failed").
pub fn init_error_message(error: &ClientInitializeError) -> String {
    match error {
        ClientInitializeError::LegacyFallbackFailed { fallback, .. } => {
            init_error_message(fallback)
        }
        ClientInitializeError::TransportError { error, .. } => error.error.to_string(),
        other => other.to_string(),
    }
}

/// Opens an HTTP+SSE session and its handshake. `streamable` is the
/// Streamable HTTP failure that led here, the one to explain when the
/// server turns out not to be on the older transport either.
async fn open_legacy(
    config: &ServerConfig,
    client_version: &str,
    streamable: Option<ClientInitializeError>,
) -> Result<Service, ConnectError> {
    let headers = auth_headers(config)?;
    match legacy_sse::open(&http_client(), &config.url, &headers).await {
        legacy_sse::Probe::Legacy(session) => client_info(client_version)
            .serve_with_lifecycle(session, lifecycle())
            .await
            .map_err(|e| describe_failure(&init_error_message(&e), config.auth)),
        legacy_sse::Probe::Refused(why) => Err(ConnectError::Failed(why)),
        // The Streamable HTTP answer (a 400/404/405 moments ago) is the one
        // to explain when there is one.
        legacy_sse::Probe::Unreachable(why) => Err(match streamable {
            Some(error) => describe_failure(&init_error_message(&error), config.auth),
            None => ConnectError::Failed(format!("Could not connect: {}", short(&why))),
        }),
        legacy_sse::Probe::NotLegacy => Err(match streamable {
            Some(error) => describe_failure(&init_error_message(&error), config.auth),
            None => ConnectError::Failed(format!(
                "No HTTP+SSE event stream naming a message address was found at {}. Check the \
                 address (usually the server's /sse address), or choose Remote server (HTTP) for \
                 a Streamable HTTP server.",
                config.url
            )),
        }),
    }
}

fn describe_failure(error: &str, auth: McpAuth) -> ConnectError {
    if is_unauthorized(error) {
        return match auth {
            McpAuth::Oauth => ConnectError::NeedsSignIn("Sign in to connect.".into()),
            McpAuth::None => ConnectError::Failed(
                "The server requires authentication. Choose OAuth or a token in its settings."
                    .into(),
            ),
            _ => ConnectError::Failed("The server rejected the token.".into()),
        };
    }
    ConnectError::Failed(format!("Could not connect: {}", short(error)))
}

/// Starts (stdio) or opens (HTTP) a connection and lists the server's tools.
pub async fn connect(
    config: &ServerConfig,
    client_version: &str,
    oauth_store: Option<Arc<dyn CredentialStore>>,
) -> Result<Connection, ConnectError> {
    let attempt = async {
        let mut tail = None;
        let service: Result<Service, String> = match config.transport {
            McpTransport::Stdio => {
                let (transport, stderr) = spawn_local(config).await?;
                tail = Some(stderr);
                client_info(client_version)
                    .serve_with_lifecycle(transport, lifecycle())
                    .await
                    .map_err(|e| init_error_message(&e))
            }
            McpTransport::Http | McpTransport::Sse => {
                let transport_config = http_config(config)?;
                if config.auth == McpAuth::Oauth {
                    if config.transport == McpTransport::Sse {
                        return Err(ConnectError::Failed(LEGACY_NO_OAUTH.into()));
                    }
                    let store = oauth_store
                        .ok_or_else(|| ConnectError::NeedsSignIn("Sign in to connect.".into()))?;
                    let mut manager = AuthorizationManager::new(config.url.as_str())
                        .await
                        .map_err(|e| describe_failure(&e.to_string(), config.auth))?;
                    manager.set_credential_store(SharedStore(store));
                    let signed_in = manager
                        .initialize_from_store()
                        .await
                        .map_err(|e| describe_failure(&e.to_string(), config.auth))?;
                    if !signed_in {
                        return Err(ConnectError::NeedsSignIn("Sign in to connect.".into()));
                    }
                    let client = AuthClient::new(http_client(), manager);
                    let transport =
                        StreamableHttpClientTransport::with_client(client, transport_config);
                    client_info(client_version)
                        .serve_with_lifecycle(transport, lifecycle())
                        .await
                        .map_err(|e| init_error_message(&e))
                } else if config.transport == McpTransport::Sse {
                    // Known to be on the older transport: no probing.
                    Ok(open_legacy(config, client_version, None).await?)
                } else {
                    let transport =
                        StreamableHttpClientTransport::with_client(http_client(), transport_config);
                    match client_info(client_version)
                        .serve_with_lifecycle(transport, lifecycle())
                        .await
                    {
                        Ok(service) => Ok(service),
                        // Refused the way a server on the older HTTP+SSE
                        // transport refuses a POST: the specification says
                        // to try its event stream next (see `legacy_sse`).
                        Err(error) if legacy_candidate(&error) => {
                            Ok(open_legacy(config, client_version, Some(error)).await?)
                        }
                        Err(error) => Err(init_error_message(&error)),
                    }
                }
            }
        };
        let service = service.map_err(|error| {
            let hint = tail.as_ref().and_then(StderrTail::hint);
            match (config.transport, hint) {
                (McpTransport::Stdio, Some(hint)) => {
                    ConnectError::Failed(format!("The server did not start: {}", short(&hint)))
                }
                (McpTransport::Stdio, None) => {
                    ConnectError::Failed(format!("The server did not start: {}", short(&error)))
                }
                (McpTransport::Http | McpTransport::Sse, _) => {
                    describe_failure(&error, config.auth)
                }
            }
        })?;
        let tools = match service.list_all_tools().await {
            Ok(tools) => tools,
            Err(error) => {
                let _ = service.cancel().await;
                return Err(ConnectError::Failed(format!(
                    "The server did not list its tools: {}",
                    short(&error.to_string())
                )));
            }
        };
        let peer = service.peer_info();
        Ok(Connection {
            protocol_version: peer.as_ref().map(|p| p.protocol_version.to_string()),
            server_info: peer.as_ref().and_then(|p| {
                p.server_info
                    .as_ref()
                    .map(|i| format!("{} {}", i.name, i.version).trim().to_string())
            }),
            tools,
            service,
        })
    };
    match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
        Ok(result) => result,
        Err(_) => Err(ConnectError::Failed(format!(
            "The server did not respond within {} seconds.",
            CONNECT_TIMEOUT.as_secs()
        ))),
    }
}

/// Connects over an already-open byte stream (ReMa MCP, the built-in
/// server, runs in-process on the other end) and lists the tools.
pub async fn connect_stream<T>(io: T, client_version: &str) -> Result<Connection, ConnectError>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let attempt = async {
        let service = client_info(client_version)
            .serve_with_lifecycle(io, lifecycle())
            .await
            .map_err(|e| {
                ConnectError::Failed(format!("Could not connect: {}", short(&e.to_string())))
            })?;
        let tools = match service.list_all_tools().await {
            Ok(tools) => tools,
            Err(error) => {
                let _ = service.cancel().await;
                return Err(ConnectError::Failed(format!(
                    "The server did not list its tools: {}",
                    short(&error.to_string())
                )));
            }
        };
        let peer = service.peer_info();
        Ok(Connection {
            protocol_version: peer.as_ref().map(|p| p.protocol_version.to_string()),
            server_info: peer.as_ref().and_then(|p| {
                p.server_info
                    .as_ref()
                    .map(|i| format!("{} {}", i.name, i.version).trim().to_string())
            }),
            tools,
            service,
        })
    };
    match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
        Ok(result) => result,
        Err(_) => Err(ConnectError::Failed(
            "The server did not respond in time.".into(),
        )),
    }
}

/// `rmcp` takes a store by value; ours is shared.
pub(crate) struct SharedStore(pub Arc<dyn CredentialStore>);

#[async_trait::async_trait]
impl CredentialStore for SharedStore {
    async fn load(
        &self,
    ) -> Result<Option<rmcp::transport::auth::StoredCredentials>, rmcp::transport::auth::AuthError>
    {
        self.0.load().await
    }

    async fn save(
        &self,
        credentials: rmcp::transport::auth::StoredCredentials,
    ) -> Result<(), rmcp::transport::auth::AuthError> {
        self.0.save(credentials).await
    }

    async fn clear(&self) -> Result<(), rmcp::transport::auth::AuthError> {
        self.0.clear().await
    }
}

#[cfg(test)]
mod tests {
    use rmcp::transport::DynamicTransportError;

    use super::*;

    fn transport_error(
        error: StreamableHttpError<reqwest::Error>,
        context: &'static str,
    ) -> ClientInitializeError {
        ClientInitializeError::TransportError {
            error: DynamicTransportError::from_parts(
                "streamable http",
                std::any::TypeId::of::<()>(),
                Box::new(error),
            ),
            context: context.into(),
        }
    }

    /// The error rmcp gives when Streamable HTTP's POST was answered with
    /// `status` and a plain body (what its reqwest client produces).
    fn answered(status: &str) -> ClientInitializeError {
        transport_error(
            StreamableHttpError::UnexpectedServerResponse(format!("HTTP {status}: nope").into()),
            "send initialize request",
        )
    }

    /// The same after discover was refused first (rmcp's Auto mode).
    fn wrapped(fallback: ClientInitializeError) -> ClientInitializeError {
        ClientInitializeError::LegacyFallbackFailed {
            discover: Box::new(ClientInitializeError::JsonRpcError(
                rmcp::model::ErrorData::invalid_request(
                    "server/discover rejected with HTTP 405 Method Not Allowed: nope",
                    None,
                ),
            )),
            fallback: Box::new(fallback),
        }
    }

    #[tokio::test]
    async fn only_a_plain_400_404_or_405_makes_a_legacy_candidate() {
        for status in ["400 Bad Request", "404 Not Found", "405 Method Not Allowed"] {
            assert!(legacy_candidate(&answered(status)), "{status}");
            assert!(legacy_candidate(&wrapped(answered(status))), "{status}");
        }
        for status in [
            "401 Unauthorized",
            "403 Forbidden",
            "408 Request Timeout",
            "429 Too Many Requests",
            "500 Internal Server Error",
            "502 Bad Gateway",
            "503 Service Unavailable",
        ] {
            assert!(!legacy_candidate(&answered(status)), "{status}");
            assert!(!legacy_candidate(&wrapped(answered(status))), "{status}");
            assert_eq!(
                handshake_status(&answered(status)),
                status.split(' ').next().unwrap().parse().ok()
            );
        }

        // A 401/403 with a challenge, a JSON-RPC error, a closed stream
        // and rmcp's other failures: no status, no fallback.
        let challenge = transport_error(
            StreamableHttpError::AuthRequired(
                rmcp::transport::streamable_http_client::AuthRequiredError::new(
                    "Bearer realm=\"mcp\"".into(),
                ),
            ),
            "send initialize request",
        );
        assert!(!legacy_candidate(&challenge));
        assert!(!legacy_candidate(&wrapped(challenge)));
        let json_rpc = ClientInitializeError::JsonRpcError(
            rmcp::model::ErrorData::invalid_request("bad initialize", None),
        );
        assert!(!legacy_candidate(&json_rpc));
        assert!(!legacy_candidate(&wrapped(json_rpc)));
        assert!(!legacy_candidate(&ClientInitializeError::ConnectionClosed(
            "discover response".into()
        )));
        assert!(!legacy_candidate(&ClientInitializeError::Cancelled));

        // A request that never reached a server (here: an address reqwest
        // cannot use; a refused connection or a timeout is the same kind).
        let unreachable = reqwest::Client::new()
            .get("http://")
            .send()
            .await
            .unwrap_err();
        let unreachable = transport_error(
            StreamableHttpError::Client(unreachable),
            "send discover request",
        );
        assert_eq!(handshake_status(&unreachable), None);
        assert!(!legacy_candidate(&unreachable));

        // The message keeps the transport's words, not the wrapper's.
        let message = init_error_message(&wrapped(answered("405 Method Not Allowed")));
        assert!(message.contains("HTTP 405 Method Not Allowed"), "{message}");
    }
}
