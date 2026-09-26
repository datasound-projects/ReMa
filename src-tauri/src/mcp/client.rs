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
    service::RunningService,
    transport::{
        auth::{AuthClient, AuthorizationManager, CredentialStore},
        streamable_http_client::StreamableHttpClientTransportConfig,
        StreamableHttpClientTransport, TokioChildProcess,
    },
    ClientLifecycleMode, ClientServiceExt, RoleClient,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::{
    accounts::locate,
    models::mcp::{McpAuth, McpToolInfo, McpTransport},
};

/// How long starting a server and listing its tools may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(45);
/// How long one tool call may take.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(180);
/// Largest tool result given to a model (characters).
const MAX_RESULT_CHARS: usize = 40_000;

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
        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        tokio::select! {
            _ = cancel.cancelled() => Err("The request was stopped.".into()),
            result = tokio::time::timeout(CALL_TIMEOUT, self.service.call_tool(params)) => match result {
                Err(_) => Err(format!("The tool did not answer within {} seconds.", CALL_TIMEOUT.as_secs())),
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

/// Environment variables passed on from ReMa (like the official MCP SDKs'
/// default environment); everything else a server needs is configured.
const INHERITED_ENV: &[&str] = if cfg!(windows) {
    &[
        "APPDATA",
        "HOMEDRIVE",
        "HOMEPATH",
        "LOCALAPPDATA",
        "PATHEXT",
        "PROCESSOR_ARCHITECTURE",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "SYSTEMDRIVE",
        "SYSTEMROOT",
        "TEMP",
        "TMP",
        "USERNAME",
        "USERPROFILE",
        "WINDIR",
    ]
} else {
    &[
        "HOME", "LOGNAME", "SHELL", "TERM", "USER", "LANG", "LC_ALL", "TMPDIR",
    ]
};

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
        locate::which(&config.command).await
    };
    let Some(located) = located else {
        return Err(ConnectError::Failed(format!(
            "“{}” was not found on this computer. Install it, or enter its full path.",
            config.command
        )));
    };

    let mut command = tokio::process::Command::new(&located.path);
    command.args(&config.args).env_clear();
    for name in INHERITED_ENV {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("PATH", locate::extended_path(&located.path_dirs));
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

fn describe_failure(error: &str, auth: McpAuth) -> ConnectError {
    let lower = error.to_lowercase();
    let unauthorized = lower.contains("auth required")
        || lower.contains("authorization required")
        || lower.contains("401")
        || lower.contains("unauthorized");
    if unauthorized {
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
                    .map_err(|e| e.to_string())
            }
            McpTransport::Http => {
                let transport_config = http_config(config)?;
                if config.auth == McpAuth::Oauth {
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
                        .map_err(|e| e.to_string())
                } else {
                    let transport =
                        StreamableHttpClientTransport::with_client(http_client(), transport_config);
                    client_info(client_version)
                        .serve_with_lifecycle(transport, lifecycle())
                        .await
                        .map_err(|e| e.to_string())
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
                (McpTransport::Http, _) => describe_failure(&error, config.auth),
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
