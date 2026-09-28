//! Adding MCP servers from other apps' configuration, as Claude Code's
//! `claude mcp add-from-claude-desktop` and `add-json` do.
//!
//! Reads the `mcpServers` format of Claude Desktop, Claude Code
//! (`~/.claude.json`, user and project servers), Cursor and Windsurf, VS
//! Code's `servers`, a single server object, or JSON the user pastes.
//! `${VAR}`, `${VAR:-default}` and `${env:VAR}` are filled in from the
//! user's environment when the server is added, so it keeps working when
//! ReMa is opened from the Finder or the Dock; secrets then go to the OS
//! credential store like any other server's. The interface only sees names,
//! programs and addresses, never values.

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use specta::Type;

use crate::{
    accounts::locate,
    error::{AppError, AppResult},
    models::mcp::{McpAuth, McpEnvVar, McpServer, McpServerInput, McpTransport},
};

/// Apps whose MCP configuration ReMa can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum McpImportApp {
    ClaudeDesktop,
    ClaudeCode,
    Cursor,
    VsCode,
    Windsurf,
}

impl McpImportApp {
    pub const ALL: [Self; 5] = [
        Self::ClaudeDesktop,
        Self::ClaudeCode,
        Self::Cursor,
        Self::VsCode,
        Self::Windsurf,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop",
            Self::ClaudeCode => "Claude Code",
            Self::Cursor => "Cursor",
            Self::VsCode => "VS Code",
            Self::Windsurf => "Windsurf",
        }
    }
}

/// A server found in a configuration (no secret values).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpImportCandidate {
    pub name: String,
    /// `None` when ReMa cannot add it (see `problem`).
    pub transport: Option<McpTransport>,
    /// The program and arguments, or the address.
    pub summary: String,
    pub env_names: Vec<String>,
    /// A server with this name is already in ReMa.
    pub already_added: bool,
    /// Why it cannot be added.
    pub problem: Option<String>,
    /// Turned off in the other app (added turned off).
    pub disabled: bool,
}

/// One app's configuration file and its servers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpImportSource {
    pub app: McpImportApp,
    pub label: String,
    pub path: String,
    pub servers: Vec<McpImportCandidate>,
}

/// What to add: servers by name from an app's file, or from pasted JSON.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpImportRequest {
    pub app: Option<McpImportApp>,
    pub json: Option<String>,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpImportSkip {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpImportResult {
    pub added: Vec<McpServer>,
    pub skipped: Vec<McpImportSkip>,
}

/// Largest configuration file read (`~/.claude.json` holds more than MCP).
const MAX_FILE: u64 = 20 * 1024 * 1024;

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// The per-user configuration folder (`%APPDATA%`, `~/Library/Application
/// Support`, `$XDG_CONFIG_HOME` or `~/.config`).
fn config_dir(home: &Path) -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Roaming"))
    } else if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"))
    }
}

/// Where an app keeps its MCP servers.
pub fn config_path(app: McpImportApp, home: &Path) -> PathBuf {
    match app {
        McpImportApp::ClaudeDesktop => config_dir(home)
            .join("Claude")
            .join("claude_desktop_config.json"),
        McpImportApp::ClaudeCode => home.join(".claude.json"),
        McpImportApp::Cursor => home.join(".cursor").join("mcp.json"),
        McpImportApp::VsCode => config_dir(home).join("Code").join("User").join("mcp.json"),
        McpImportApp::Windsurf => home
            .join(".codeium")
            .join("windsurf")
            .join("mcp_config.json"),
    }
}

fn read_json(path: &Path) -> Option<Value> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    parse_json(&text).ok()
}

/// JSON, allowing the comments and trailing commas VS Code's files have.
fn parse_json(text: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(text).or_else(|error| {
        let cleaned = strip_jsonc(text);
        serde_json::from_str(&cleaned).map_err(|_| error)
    })
}

/// Removes `//` and `/* */` comments and trailing commas outside strings.
fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut in_string) = (0, false);
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

fn is_server(value: &Value) -> bool {
    value.as_object().is_some_and(|o| {
        o.contains_key("command") || o.contains_key("url") || o.contains_key("serverUrl")
    })
}

fn entries(map: &Map<String, Value>, out: &mut Vec<(String, Value)>) {
    for (name, config) in map {
        if is_server(config) && !out.iter().any(|(n, _)| n == name) {
            out.push((name.clone(), config.clone()));
        }
    }
}

/// The servers in a configuration, by name (sorted by name).
pub fn servers_in(value: &Value) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let Some(root) = value.as_object() else {
        return out;
    };
    if let Some(map) = root.get("mcpServers").and_then(Value::as_object) {
        entries(map, &mut out);
    }
    if let Some(map) = root.get("servers").and_then(Value::as_object) {
        entries(map, &mut out);
    }
    if let Some(map) = root
        .get("mcp")
        .and_then(|m| m.get("servers"))
        .and_then(Value::as_object)
    {
        entries(map, &mut out);
    }
    // Claude Code: servers added for one project.
    if let Some(projects) = root.get("projects").and_then(Value::as_object) {
        for project in projects.values() {
            if let Some(map) = project.get("mcpServers").and_then(Value::as_object) {
                entries(map, &mut out);
            }
        }
    }
    if out.is_empty() {
        if is_server(value) {
            // One server on its own (`claude mcp add-json`).
            let name = root
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("MCP server")
                .to_string();
            out.push((name, value.clone()));
        } else {
            // A bare map of servers.
            entries(root, &mut out);
        }
    }
    out
}

/// Fills in `${VAR}`, `${VAR:-default}` and `${env:VAR}`; `${userHome}` is
/// the home folder. Err names what is missing.
pub fn expand(text: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return Ok(out);
        };
        let inner = &after[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (inner, None),
        };
        let name = name.strip_prefix("env:").unwrap_or(name);
        let value = if name == "userHome" {
            home().map(|h| h.to_string_lossy().into_owned())
        } else if name.starts_with("input:") || name.contains(':') || name == "workspaceFolder" {
            return Err(format!(
                "It uses “${{{inner}}}”, which only its own app can fill in; add it by hand."
            ));
        } else {
            lookup(name).filter(|v| !v.is_empty())
        };
        match (value, default) {
            (Some(value), _) => out.push_str(&value),
            (None, Some(default)) => out.push_str(default),
            (None, None) => {
                return Err(format!(
                    "It needs the environment variable {name}, which is not set on this computer."
                ))
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn text_of(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn base_name(command: &str) -> String {
    let base = Path::new(command)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(command)
        .to_lowercase();
    base.strip_suffix(".exe").unwrap_or(&base).to_string()
}

/// A configuration turned into ReMa's form, and whether it was turned off
/// in the other app.
pub fn to_input(
    name: &str,
    config: &Value,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<(McpServerInput, bool), String> {
    let Some(config) = config.as_object() else {
        return Err("It is not a server configuration.".into());
    };
    let disabled = config.get("disabled").and_then(Value::as_bool) == Some(true)
        || config.get("enabled").and_then(Value::as_bool) == Some(false);
    let kind = text_of(config.get("type"))
        .or_else(|| text_of(config.get("transport")))
        .unwrap_or_default()
        .to_lowercase();
    let mut input = McpServerInput {
        name: name.trim().to_string(),
        transport: McpTransport::Stdio,
        command: String::new(),
        args: Vec::new(),
        env: Vec::new(),
        cwd: String::new(),
        url: String::new(),
        auth: McpAuth::None,
        header_name: String::new(),
        secret: None,
    };
    if let Some(command) = text_of(config.get("command")) {
        let mut command = expand(command, lookup)?;
        let mut args = Vec::new();
        for arg in config
            .get("args")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match arg {
                Value::String(s) => args.push(expand(s, lookup)?),
                Value::Number(n) => args.push(n.to_string()),
                _ => return Err("An argument is not text.".into()),
            }
        }
        // `cmd /c npx …` (the Windows advice for other apps): ReMa starts
        // the program itself.
        if base_name(&command) == "cmd"
            && args.first().is_some_and(|a| a.eq_ignore_ascii_case("/c"))
        {
            args.remove(0);
            if args.is_empty() {
                return Err("It has no program to start.".into());
            }
            command = args.remove(0);
        }
        if ["sh", "bash", "zsh", "fish", "powershell", "pwsh", "wsl"]
            .contains(&base_name(&command).as_str())
        {
            return Err(
                "It starts through a shell; add it with the program itself (for example npx or \
                 uvx) instead."
                    .into(),
            );
        }
        let cwd = match text_of(config.get("cwd")) {
            Some(cwd) => expand(cwd, lookup)?,
            None => String::new(),
        };
        // A program next to the configuration's working folder.
        if (command.starts_with("./") || command.starts_with(".\\")) && !cwd.is_empty() {
            command = Path::new(&cwd)
                .join(&command[2..])
                .to_string_lossy()
                .into_owned();
        }
        if let Some(env) = config.get("env").and_then(Value::as_object) {
            for (key, value) in env {
                let value = match value {
                    Value::String(s) => expand(s, lookup)?,
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => return Err(format!("The value of {key} is not text.")),
                };
                input.env.push(McpEnvVar {
                    name: key.clone(),
                    value: Some(value),
                });
            }
        }
        input.command = command;
        input.args = args;
        input.cwd = cwd;
        return Ok((input, disabled));
    }
    let Some(url) = text_of(config.get("url")).or_else(|| text_of(config.get("serverUrl"))) else {
        return Err("It has no program or address.".into());
    };
    // An HTTP+SSE server ("sse") is added like any remote server: ReMa
    // tries Streamable HTTP first and then the older transport, as the MCP
    // specification says (see `legacy_sse`).
    if matches!(kind.as_str(), "ws" | "websocket") {
        return Err(
            "It uses WebSocket, which is not an MCP transport ReMa (or the MCP specification) \
             supports; ask its provider for a Streamable HTTP address."
                .into(),
        );
    }
    input.transport = McpTransport::Http;
    input.url = expand(url, lookup)?;
    let headers: Vec<(String, String)> = match config.get("headers").and_then(Value::as_object) {
        Some(headers) => headers
            .iter()
            .map(|(k, v)| match v.as_str() {
                Some(v) => expand(v, lookup).map(|v| (k.clone(), v)),
                None => Err(format!("The header {k} is not text.")),
            })
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    match headers.as_slice() {
        [] if config.contains_key("oauth") => input.auth = McpAuth::Oauth,
        [] => {}
        [(name, value)] if name.eq_ignore_ascii_case("authorization") => {
            let token = value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
                .ok_or("Its Authorization header is not a Bearer token; add it by hand.")?;
            input.auth = McpAuth::Bearer;
            input.secret = Some(token.trim().to_string());
        }
        [(name, value)] => {
            input.auth = McpAuth::Header;
            input.header_name = name.clone();
            input.secret = Some(value.clone());
        }
        _ => return Err("It sends several headers; ReMa sends one per server.".into()),
    }
    Ok((input, disabled))
}

/// The user's environment for `${VAR}`: the login shell's, then ReMa's.
pub async fn environment() -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    for (name, value) in locate::login_environment().await {
        if let (Some(name), Some(value)) = (name.to_str(), value.to_str()) {
            env.insert(name.to_string(), value.to_string());
        }
    }
    env
}

fn summary(input: &McpServerInput) -> String {
    match input.transport {
        McpTransport::Stdio => std::iter::once(input.command.as_str())
            .chain(input.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        McpTransport::Http => input.url.clone(),
    }
}

/// What the interface shows for each server of a configuration.
pub fn candidates(
    value: &Value,
    lookup: &dyn Fn(&str) -> Option<String>,
    existing: &[String],
) -> Vec<McpImportCandidate> {
    servers_in(value)
        .into_iter()
        .map(|(name, config)| {
            let already_added = existing.iter().any(|e| e.eq_ignore_ascii_case(name.trim()));
            match to_input(&name, &config, lookup).and_then(|(input, disabled)| {
                super::config::validate(&input)
                    .map(|_| (input, disabled))
                    .map_err(|e| e.to_string())
            }) {
                Ok((input, disabled)) => McpImportCandidate {
                    name,
                    transport: Some(input.transport),
                    summary: summary(&input),
                    env_names: input.env.iter().map(|v| v.name.clone()).collect(),
                    already_added,
                    problem: None,
                    disabled,
                },
                Err(problem) => McpImportCandidate {
                    summary: config
                        .get("command")
                        .or_else(|| config.get("url"))
                        .or_else(|| config.get("serverUrl"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name,
                    transport: None,
                    env_names: Vec::new(),
                    already_added,
                    problem: Some(problem),
                    disabled: false,
                },
            }
        })
        .collect()
}

/// The configuration an import request names.
pub fn request_config(request: &McpImportRequest) -> AppResult<Value> {
    match (&request.json, request.app) {
        (Some(json), _) => parse_json(json)
            .map_err(|e| AppError::validation(format!("That is not valid JSON: {e}."))),
        (None, Some(app)) => {
            let home = home().ok_or_else(|| AppError::internal("no home folder"))?;
            read_json(&config_path(app, &home)).ok_or_else(|| {
                AppError::validation(format!("{}'s MCP settings could not be read.", app.label()))
            })
        }
        (None, None) => Err(AppError::validation("Choose where to add servers from.")),
    }
}

/// The apps on this computer that have MCP servers configured.
pub fn sources(
    lookup: &dyn Fn(&str) -> Option<String>,
    existing: &[String],
) -> Vec<McpImportSource> {
    let Some(home) = home() else {
        return Vec::new();
    };
    McpImportApp::ALL
        .into_iter()
        .filter_map(|app| {
            let path = config_path(app, &home);
            let value = read_json(&path)?;
            let servers = candidates(&value, lookup, existing);
            (!servers.is_empty()).then(|| McpImportSource {
                app,
                label: app.label().to_string(),
                path: path.to_string_lossy().into_owned(),
                servers,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn env(name: &str) -> Option<String> {
        match name {
            "GITHUB_TOKEN" => Some("ghp_secret".into()),
            "API_BASE" => Some("https://mcp.example.com".into()),
            _ => None,
        }
    }

    #[test]
    fn reads_claude_desktop_and_cursor_servers() {
        let config = json!({ "mcpServers": {
            "github": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-github"],
                "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "${GITHUB_TOKEN}" }
            },
            "files": { "command": "cmd", "args": ["/c", "npx", "-y", "files-mcp", 3] },
            "old": { "command": "uvx", "args": ["old-mcp"], "disabled": true },
            "shell": { "command": "bash", "args": ["-c", "run-server"] }
        }});
        let servers = servers_in(&config);
        let names: Vec<&str> = servers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["files", "github", "old", "shell"]);
        let get = |name: &str| servers.iter().find(|(n, _)| n == name).unwrap().1.clone();

        let (github, off) = to_input("github", &get("github"), &env).unwrap();
        assert!(!off);
        assert_eq!(github.transport, McpTransport::Stdio);
        assert_eq!(github.command, "npx");
        assert_eq!(github.args, ["-y", "@modelcontextprotocol/server-github"]);
        assert_eq!(github.env[0].value.as_deref(), Some("ghp_secret"));

        // The Windows `cmd /c` wrapper is dropped: ReMa starts npx itself.
        let (files, _) = to_input("files", &get("files"), &env).unwrap();
        assert_eq!(
            (files.command.as_str(), files.args.as_slice()),
            (
                "npx",
                &["-y".to_string(), "files-mcp".into(), "3".into()][..]
            )
        );
        assert!(to_input("old", &get("old"), &env).unwrap().1);
        assert!(to_input("shell", &get("shell"), &env)
            .unwrap_err()
            .contains("through a shell"));
    }

    #[test]
    fn reads_remote_servers_and_their_one_header() {
        let config = json!({ "servers": {
            "linear": { "type": "http", "url": "${API_BASE:-https://x.example}/mcp",
                        "headers": { "Authorization": "Bearer ${GITHUB_TOKEN}" } },
            "search": { "url": "https://search.example/mcp", "headers": { "X-API-Key": "k-1" } },
            "notion": { "type": "http", "url": "https://mcp.notion.com/mcp", "oauth": {} },
            "two": { "url": "https://two.example/mcp", "headers": { "A": "1", "B": "2" } },
            "legacy": { "type": "sse", "url": "https://old.example/sse" },
            "asks": { "type": "http", "url": "https://x.example/mcp",
                      "headers": { "Authorization": "Bearer ${input:token}" } }
        }});
        let servers = servers_in(&config);
        let get = |name: &str| servers.iter().find(|(n, _)| n == name).unwrap().1.clone();

        let (linear, _) = to_input("linear", &get("linear"), &env).unwrap();
        assert_eq!(linear.url, "https://mcp.example.com/mcp");
        assert_eq!(
            (linear.auth, linear.secret.as_deref()),
            (McpAuth::Bearer, Some("ghp_secret"))
        );
        let (search, _) = to_input("search", &get("search"), &env).unwrap();
        assert_eq!(
            (search.auth, search.header_name.as_str()),
            (McpAuth::Header, "X-API-Key")
        );
        assert_eq!(
            to_input("notion", &get("notion"), &env).unwrap().0.auth,
            McpAuth::Oauth
        );
        assert!(to_input("two", &get("two"), &env)
            .unwrap_err()
            .contains("several headers"));
        // The older HTTP+SSE transport is added as a remote server.
        let (legacy, _) = to_input("legacy", &get("legacy"), &env).unwrap();
        assert_eq!(
            (legacy.transport, legacy.url.as_str()),
            (McpTransport::Http, "https://old.example/sse")
        );
        assert!(to_input("asks", &get("asks"), &env)
            .unwrap_err()
            .contains("by hand"));
    }

    #[test]
    fn expands_variables_with_defaults_and_names_what_is_missing() {
        assert_eq!(
            expand("a ${env:GITHUB_TOKEN} b", &env).unwrap(),
            "a ghp_secret b"
        );
        assert_eq!(expand("${MISSING:-fallback}", &env).unwrap(), "fallback");
        assert_eq!(expand("no vars", &env).unwrap(), "no vars");
        assert!(expand("${MISSING}", &env).unwrap_err().contains("MISSING"));
        assert!(expand("${workspaceFolder}/x", &env).is_err());
    }

    #[test]
    fn reads_claude_code_project_servers_single_servers_and_comments() {
        let claude_code = json!({
            "numStartups": 3,
            "mcpServers": { "user": { "command": "uvx", "args": ["user-mcp"] } },
            "projects": { "/Users/ana/app": { "mcpServers": {
                "project": { "type": "stdio", "command": "node", "args": ["server.js"] },
                "user": { "command": "other" }
            }}}
        });
        let names: Vec<String> = servers_in(&claude_code)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(
            names,
            ["user", "project"],
            "user servers first; the first of a name wins"
        );

        let single = json!({ "type": "http", "url": "https://one.example/mcp" });
        assert_eq!(servers_in(&single).len(), 1);

        let jsonc = "{ // VS Code\n \"servers\": { \"a\": { \"command\": \"npx\", \"args\": [\"x\",], }, }, }";
        let value = parse_json(jsonc).unwrap();
        assert_eq!(servers_in(&value)[0].0, "a");
    }

    #[test]
    fn candidates_say_what_is_already_added_and_what_cannot_be() {
        let config = json!({ "mcpServers": {
            "GitHub": { "command": "npx", "args": ["-y", "gh-mcp"], "env": { "T": "${GITHUB_TOKEN}" } },
            "needs": { "command": "npx", "env": { "T": "${NOT_SET}" } }
        }});
        let found = candidates(&config, &env, &["github".to_string()]);
        assert!(found[0].already_added);
        assert_eq!(found[0].summary, "npx -y gh-mcp");
        assert_eq!(found[0].env_names, ["T"]);
        assert!(found[1].problem.as_deref().unwrap().contains("NOT_SET"));
        // Values never reach the interface.
        assert!(!serde_json::to_string(&found)
            .unwrap()
            .contains("ghp_secret"));
    }

    #[test]
    fn knows_where_each_app_keeps_its_servers() {
        let home = Path::new("/home/ana");
        assert!(config_path(McpImportApp::ClaudeCode, home).ends_with(".claude.json"));
        assert!(config_path(McpImportApp::Cursor, home).ends_with(".cursor/mcp.json"));
        assert!(config_path(McpImportApp::ClaudeDesktop, home)
            .ends_with("Claude/claude_desktop_config.json"));
        assert!(config_path(McpImportApp::VsCode, home).ends_with("Code/User/mcp.json"));
    }
}
