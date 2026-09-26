//! MCP (Model Context Protocol) servers: external tools and data sources a
//! model may use when the user makes them available in a chat.
//!
//! Configured → enabled in Settings → offered in the Chat + menu → selected
//! for a chat → a tool may be called by the model. Configuration is local;
//! secrets live in the OS credential store and never reach the interface.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::text_enum;

/// How ReMa talks to the server (the MCP specification's standard
/// transports).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    /// A local program ReMa starts; messages over its standard streams.
    Stdio,
    /// A remote server at an HTTPS URL (Streamable HTTP).
    Http,
}

text_enum!(McpTransport { Stdio => "stdio", Http => "http" });

/// Authentication for remote servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum McpAuth {
    None,
    /// `Authorization: Bearer <token>`.
    Bearer,
    /// A custom header with a secret value, e.g. `X-API-Key`.
    Header,
    /// OAuth sign-in in the browser (MCP authorization).
    Oauth,
}

text_enum!(McpAuth {
    None => "none",
    Bearer => "bearer",
    Header => "header",
    Oauth => "oauth",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum McpState {
    /// Turned off in Settings: not offered in Chat, no process runs.
    Disabled,
    /// Enabled; ReMa connects when it is needed.
    Disconnected,
    Connecting,
    Connected,
    /// OAuth: the user has to sign in first.
    NeedsSignIn,
    Error,
}

/// A tool the server offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    /// The server says the tool only reads (it still is only a hint).
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub state: McpState,
    /// What went wrong, or what to do.
    pub message: Option<String>,
    pub tools: Vec<McpToolInfo>,
    /// The protocol revision in use, e.g. "2026-07-28".
    pub protocol_version: Option<String>,
    /// The server's own name and version, if it reported them.
    pub server_info: Option<String>,
}

impl McpStatus {
    pub fn of(state: McpState) -> Self {
        Self {
            state,
            message: None,
            tools: Vec::new(),
            protocol_version: None,
            server_info: None,
        }
    }
}

/// A configured server, as the interface sees it (no secret values).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: i64,
    pub name: String,
    pub transport: McpTransport,
    pub command: String,
    pub args: Vec<String>,
    /// Environment variable names; their values are secret.
    pub env_names: Vec<String>,
    pub cwd: String,
    pub url: String,
    pub auth: McpAuth,
    pub header_name: String,
    /// A token or header value is stored (never shown).
    pub has_secret: bool,
    pub enabled: bool,
    pub status: McpStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

/// An environment variable in the editor. `value: None` keeps the stored
/// value of an existing variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpEnvVar {
    pub name: String,
    pub value: Option<String>,
}

/// What the add/edit form sends. Only the fields of `transport` (and
/// `auth`) are used; the rest are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpServerInput {
    pub name: String,
    pub transport: McpTransport,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<McpEnvVar>,
    pub cwd: String,
    pub url: String,
    pub auth: McpAuth,
    pub header_name: String,
    /// Bearer token or header value. `None` keeps the stored one.
    pub secret: Option<String>,
}

/// The result of "Test connection".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpTestResult {
    pub ok: bool,
    pub message: String,
    pub tools: Vec<McpToolInfo>,
}

/// MCP servers or their connections changed.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct McpChanged;
