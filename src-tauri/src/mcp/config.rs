//! Validating MCP server configuration before it is stored or used.
//!
//! Local servers are started as `program + argument array`, never through a
//! shell, so a shell interpreter as the program (which would bring string
//! commands back) is refused, as are relative paths and environment
//! variables that inject code into every program (`LD_PRELOAD`, `DYLD_*`).
//! Remote servers need HTTPS (plain HTTP only on this computer).

use std::path::Path;

use crate::{
    accounts::locate,
    error::{AppError, AppResult},
    models::mcp::{McpAuth, McpEnvVar, McpServerInput, McpTransport},
};

const MAX_NAME: usize = 60;
const MAX_COMMAND: usize = 1_024;
const MAX_ARGS: usize = 64;
const MAX_ARG: usize = 4_096;
const MAX_ENV: usize = 50;
const MAX_ENV_VALUE: usize = 16_384;
const MAX_SECRET: usize = 8_192;
const MAX_URL: usize = 2_000;

/// Programs that run command strings: using one as the server command would
/// reintroduce shell parsing.
const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "ash",
    "cmd",
    "powershell",
    "pwsh",
    "wsl",
];

/// A validated configuration. Secrets (`env` values, `secret`) are `None`
/// when the stored value should be kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidConfig {
    pub name: String,
    pub transport: McpTransport,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<McpEnvVar>,
    pub cwd: String,
    pub url: String,
    pub auth: McpAuth,
    pub header_name: String,
    pub secret: Option<String>,
}

fn no_control(value: &str) -> bool {
    !value.chars().any(|c| c == '\0' || c == '\n' || c == '\r')
}

fn validate_name(name: &str) -> AppResult<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(AppError::validation("Give the server a name."));
    }
    if name.chars().count() > MAX_NAME {
        return Err(AppError::validation("The name is too long."));
    }
    Ok(name)
}

/// A program name (looked up on `PATH`) or an absolute path to an
/// executable file.
pub fn validate_command(command: &str) -> AppResult<String> {
    let command = command.trim();
    if command.is_empty() {
        return Err(AppError::validation(
            "Enter the program that starts the server, e.g. npx or uvx.",
        ));
    }
    if command.len() > MAX_COMMAND || !no_control(command) {
        return Err(AppError::validation("The command is not valid."));
    }
    let path = Path::new(command);
    let base = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command)
        .to_lowercase();
    let base = base.strip_suffix(".exe").unwrap_or(&base);
    if SHELLS.contains(&base) {
        return Err(AppError::validation(
            "Run the server's program directly (for example npx, uvx, node or python), not through a shell.",
        ));
    }
    if command.contains('/') || command.contains('\\') {
        if !path.is_absolute() {
            return Err(AppError::validation(
                "Use a program name (found on PATH) or a full path, not a relative path.",
            ));
        }
        if !locate::is_executable_file(path) {
            return Err(AppError::validation(
                "No executable program was found at that path.",
            ));
        }
    } else if !command
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
    {
        return Err(AppError::validation(
            "Enter only the program name here and put its arguments in Arguments.",
        ));
    }
    Ok(command.to_string())
}

fn validate_args(args: &[String]) -> AppResult<Vec<String>> {
    if args.len() > MAX_ARGS {
        return Err(AppError::validation("Too many arguments."));
    }
    let args: Vec<String> = args
        .iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    if args.iter().any(|a| a.len() > MAX_ARG || a.contains('\0')) {
        return Err(AppError::validation("An argument is not valid."));
    }
    Ok(args)
}

/// Variables that make every program load foreign code.
fn injects_code(name: &str) -> bool {
    let upper = name.to_uppercase();
    upper.starts_with("LD_") || upper.starts_with("DYLD_") || upper == "_JAVA_OPTIONS"
}

fn validate_env(env: &[McpEnvVar]) -> AppResult<Vec<McpEnvVar>> {
    if env.len() > MAX_ENV {
        return Err(AppError::validation("Too many environment variables."));
    }
    let mut out: Vec<McpEnvVar> = Vec::new();
    for var in env {
        let name = var.name.trim();
        if name.is_empty() && var.value.as_deref().unwrap_or("").is_empty() {
            continue;
        }
        let valid = !name.is_empty()
            && name.len() <= 128
            && name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(AppError::validation(format!(
                "“{name}” is not a valid environment variable name."
            )));
        }
        if injects_code(name) {
            return Err(AppError::validation(format!(
                "{name} cannot be set for an MCP server."
            )));
        }
        if out.iter().any(|v| v.name.eq_ignore_ascii_case(name)) {
            return Err(AppError::validation(format!("{name} is set twice.")));
        }
        if let Some(value) = &var.value {
            if value.len() > MAX_ENV_VALUE || value.contains('\0') {
                return Err(AppError::validation(format!(
                    "The value of {name} is not valid."
                )));
            }
        }
        out.push(McpEnvVar {
            name: name.to_string(),
            value: var.value.clone(),
        });
    }
    Ok(out)
}

fn validate_cwd(cwd: &str) -> AppResult<String> {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return Ok(String::new());
    }
    let path = Path::new(cwd);
    if !path.is_absolute() || !path.is_dir() || !no_control(cwd) {
        return Err(AppError::validation(
            "The working folder must be the full path of an existing folder.",
        ));
    }
    Ok(cwd.to_string())
}

/// HTTPS, or plain HTTP to this computer only; no credentials in the URL.
pub fn validate_url(url: &str) -> AppResult<String> {
    let url = url.trim();
    let invalid = || AppError::validation("Enter the server's URL, e.g. https://example.com/mcp.");
    if url.is_empty() || url.len() > MAX_URL {
        return Err(invalid());
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| invalid())?;
    let host = parsed.host_str().unwrap_or_default();
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1");
    match parsed.scheme() {
        "https" => {}
        "http" if local => {}
        "http" => {
            return Err(AppError::validation(
                "Use https:// (plain http is only allowed for servers on this computer).",
            ))
        }
        _ => return Err(invalid()),
    }
    if host.is_empty() {
        return Err(invalid());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AppError::validation(
            "Don't put credentials in the URL; use the authentication options instead.",
        ));
    }
    Ok(parsed.to_string())
}

fn validate_header_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    let token = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c));
    let lower = name.to_lowercase();
    let reserved = matches!(
        lower.as_str(),
        "host"
            | "content-length"
            | "content-type"
            | "accept"
            | "connection"
            | "transfer-encoding"
            | "cookie"
            | "origin"
    ) || lower.starts_with("mcp-");
    if !token || reserved {
        return Err(AppError::validation(
            "Enter a valid header name, e.g. X-API-Key.",
        ));
    }
    Ok(name.to_string())
}

fn validate_secret(secret: &Option<String>) -> AppResult<Option<String>> {
    match secret {
        None => Ok(None),
        Some(value) => {
            let value = value.trim();
            if value.len() > MAX_SECRET || !no_control(value) {
                return Err(AppError::validation("The token is not valid."));
            }
            Ok(Some(value.to_string()))
        }
    }
}

/// Checks everything the form sent. Only the fields of the chosen transport
/// (and authentication) are kept.
pub fn validate(input: &McpServerInput) -> AppResult<ValidConfig> {
    let name = validate_name(&input.name)?;
    match input.transport {
        McpTransport::Stdio => Ok(ValidConfig {
            name,
            transport: McpTransport::Stdio,
            command: validate_command(&input.command)?,
            args: validate_args(&input.args)?,
            env: validate_env(&input.env)?,
            cwd: validate_cwd(&input.cwd)?,
            url: String::new(),
            auth: McpAuth::None,
            header_name: String::new(),
            secret: None,
        }),
        McpTransport::Http => {
            let url = validate_url(&input.url)?;
            let header_name = match input.auth {
                McpAuth::Header => validate_header_name(&input.header_name)?,
                _ => String::new(),
            };
            let secret = match input.auth {
                McpAuth::Bearer | McpAuth::Header => validate_secret(&input.secret)?,
                _ => None,
            };
            Ok(ValidConfig {
                name,
                transport: McpTransport::Http,
                command: String::new(),
                args: Vec::new(),
                env: Vec::new(),
                cwd: String::new(),
                url,
                auth: input.auth,
                header_name,
                secret,
            })
        }
    }
}

/// Keychain account names for a server's secrets.
pub fn env_account(id: i64) -> String {
    format!("mcp:{id}:env")
}

pub fn secret_account(id: i64) -> String {
    format!("mcp:{id}:secret")
}

pub fn oauth_account(id: i64) -> String {
    format!("mcp:{id}:oauth")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio(command: &str, args: &[&str]) -> McpServerInput {
        McpServerInput {
            name: "Files".into(),
            transport: McpTransport::Stdio,
            command: command.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: Vec::new(),
            cwd: String::new(),
            url: String::new(),
            auth: McpAuth::None,
            header_name: String::new(),
            secret: None,
        }
    }

    fn http(url: &str, auth: McpAuth) -> McpServerInput {
        McpServerInput {
            transport: McpTransport::Http,
            url: url.into(),
            auth,
            ..stdio("", &[])
        }
    }

    #[test]
    fn accepts_programs_with_argument_arrays() {
        let config = validate(&stdio(
            "npx",
            &[
                "-y",
                "@modelcontextprotocol/server-filesystem",
                "/Users/ana/My Documents",
                "",
            ],
        ))
        .unwrap();
        assert_eq!(config.command, "npx");
        assert_eq!(
            config.args.len(),
            3,
            "empty arguments are dropped, spaces kept"
        );
        assert_eq!(config.args[2], "/Users/ana/My Documents");
        assert!(config.url.is_empty());
        assert!(validate(&stdio("uvx", &["mcp-server-fetch"])).is_ok());
        assert!(validate(&stdio("mcp_server.v2", &[])).is_ok());
    }

    #[test]
    fn rejects_unsafe_process_configuration() {
        for (command, why) in [
            ("bash", "a shell"),
            ("/bin/sh", "a shell by path"),
            ("powershell.exe", "a Windows shell"),
            ("npx -y server", "arguments in the command"),
            ("npx && rm -rf ~", "shell syntax"),
            ("./server", "a relative path"),
            ("bin/server", "a relative path"),
            ("/definitely/not/here", "a missing program"),
            ("", "nothing"),
        ] {
            assert!(validate(&stdio(command, &[])).is_err(), "{why}: {command}");
        }
        let mut input = stdio("npx", &[]);
        input.env = vec![McpEnvVar {
            name: "LD_PRELOAD".into(),
            value: Some("/tmp/evil.so".into()),
        }];
        assert!(validate(&input).is_err());
        input.env = vec![McpEnvVar {
            name: "DYLD_INSERT_LIBRARIES".into(),
            value: Some("x".into()),
        }];
        assert!(validate(&input).is_err());
        input.env = vec![McpEnvVar {
            name: "1BAD".into(),
            value: Some("x".into()),
        }];
        assert!(validate(&input).is_err());
        input.env = vec![
            McpEnvVar {
                name: "API_KEY".into(),
                value: Some("a".into()),
            },
            McpEnvVar {
                name: "api_key".into(),
                value: Some("b".into()),
            },
        ];
        assert!(validate(&input).is_err(), "duplicates");
        let mut input = stdio("npx", &["ok", "bad\0arg"]);
        assert!(validate(&input).is_err());
        input.args = vec![];
        input.cwd = "relative/folder".into();
        assert!(validate(&input).is_err());
    }

    #[test]
    fn remote_servers_need_https_and_clean_headers() {
        let ok = validate(&http("https://mcp.example.com/mcp", McpAuth::None)).unwrap();
        assert_eq!(ok.url, "https://mcp.example.com/mcp");
        assert!(ok.command.is_empty(), "stdio fields are dropped");
        assert!(validate(&http("http://127.0.0.1:8080/mcp", McpAuth::None)).is_ok());
        assert!(validate(&http("http://example.com/mcp", McpAuth::None)).is_err());
        assert!(validate(&http("ftp://example.com", McpAuth::None)).is_err());
        assert!(validate(&http("https://user:pw@example.com/mcp", McpAuth::None)).is_err());

        let mut header = http("https://mcp.example.com/mcp", McpAuth::Header);
        header.header_name = "X-API-Key".into();
        header.secret = Some("abc123".into());
        let config = validate(&header).unwrap();
        assert_eq!(config.header_name, "X-API-Key");
        assert_eq!(config.secret.as_deref(), Some("abc123"));
        for bad in ["Host", "Mcp-Method", "X Api", ""] {
            header.header_name = bad.into();
            assert!(validate(&header).is_err(), "{bad}");
        }
        let mut bearer = http("https://mcp.example.com/mcp", McpAuth::Bearer);
        bearer.secret = Some("token\r\nX-Evil: 1".into());
        assert!(validate(&bearer).is_err(), "no header injection");
    }
}
