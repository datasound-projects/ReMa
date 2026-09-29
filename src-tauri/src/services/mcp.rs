//! MCP servers in Settings: configure, enable, connect, test, sign in.
//!
//! Configuration lives in the database; secret values (environment
//! variables, tokens, OAuth credentials) only in the OS credential store.
//! The interface sees variable names and whether a token is set, never a
//! value. New servers start disabled.

use std::{collections::BTreeMap, sync::Arc};

use rmcp::transport::auth::CredentialStore;

use crate::{
    db::mcp::{self as repo, McpConfig, McpServerRow},
    error::{AppError, AppResult},
    mcp::{
        client::{self, ConnectError, ServerConfig},
        config::{self, env_account, oauth_account, secret_account},
        import::{
            self, McpImportCandidate, McpImportRequest, McpImportResult, McpImportSkip,
            McpImportSource,
        },
        oauth::{self, KeychainCredentials},
    },
    models::mcp::{
        McpAuth, McpServer, McpServerInput, McpState, McpStatus, McpTestResult, McpTransport,
    },
    state::AppState,
    time::now_ms,
};

fn to_server(state: &AppState, row: McpServerRow) -> McpServer {
    let mut status = state.mcp.status(row.id, row.enabled);
    if state.mcp.signing_in(row.id) {
        status = McpStatus {
            message: Some("Finish signing in in your browser.".into()),
            ..McpStatus::of(McpState::Connecting)
        };
    }
    McpServer {
        id: row.id,
        name: row.name,
        transport: row.transport,
        command: row.command,
        args: row.args,
        env_names: row.env_names,
        cwd: row.cwd,
        url: row.url,
        // A token is required to save these kinds, so one is stored.
        has_secret: matches!(row.auth, McpAuth::Bearer | McpAuth::Header),
        auth: row.auth,
        header_name: row.header_name,
        enabled: row.enabled,
        status,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub fn list(state: &AppState) -> AppResult<Vec<McpServer>> {
    let rows = state.db.call(|c| repo::list(c))?;
    Ok(rows.into_iter().map(|row| to_server(state, row)).collect())
}

pub fn get(state: &AppState, id: i64) -> AppResult<McpServer> {
    let row = state.db.call(|c| repo::get(c, id))?;
    Ok(to_server(state, row))
}

async fn stored_env(state: &AppState, id: i64) -> AppResult<BTreeMap<String, String>> {
    Ok(state
        .vault
        .get_text(&env_account(id))
        .await?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default())
}

/// Resolves the environment of a form: new values, or the stored value of
/// a variable whose value was left unchanged.
async fn resolve_env(
    state: &AppState,
    id: Option<i64>,
    valid: &config::ValidConfig,
) -> AppResult<BTreeMap<String, String>> {
    let stored = match id {
        Some(id) => stored_env(state, id).await?,
        None => BTreeMap::new(),
    };
    let mut env = BTreeMap::new();
    for var in &valid.env {
        let value = match &var.value {
            Some(value) => value.clone(),
            None => stored
                .get(&var.name)
                .cloned()
                .ok_or_else(|| AppError::validation(format!("Enter a value for {}.", var.name)))?,
        };
        env.insert(var.name.clone(), value);
    }
    Ok(env)
}

/// Creates (`id` = `None`, disabled) or updates a server.
pub async fn save(
    state: &AppState,
    id: Option<i64>,
    input: McpServerInput,
) -> AppResult<McpServer> {
    let valid = config::validate(&input)?;
    let previous = match id {
        Some(id) => Some(state.db.call(|c| repo::get(c, id))?),
        None => None,
    };
    let needs_secret = matches!(valid.auth, McpAuth::Bearer | McpAuth::Header);
    let keeps_secret = previous
        .as_ref()
        .is_some_and(|p| matches!(p.auth, McpAuth::Bearer | McpAuth::Header));
    if needs_secret && valid.secret.as_deref().unwrap_or("").is_empty() && !keeps_secret {
        return Err(AppError::validation("Enter the token."));
    }
    let env = resolve_env(state, id, &valid).await?;
    let env_names: Vec<String> = valid.env.iter().map(|v| v.name.clone()).collect();

    let row_config = McpConfig {
        name: &valid.name,
        transport: valid.transport,
        command: &valid.command,
        args: &valid.args,
        env_names: &env_names,
        cwd: &valid.cwd,
        url: &valid.url,
        auth: valid.auth,
        header_name: &valid.header_name,
    };
    let id = state.db.call(|c| match id {
        Some(id) => {
            repo::update(c, id, &row_config, now_ms())?;
            Ok(id)
        }
        None => repo::insert(c, &row_config, now_ms()),
    })?;

    // Secrets: only what this kind of server uses is kept.
    if env.is_empty() {
        state.vault.delete_text(&env_account(id)).await?;
    } else {
        let raw = serde_json::to_string(&env).map_err(|e| AppError::internal(e.to_string()))?;
        state.vault.set_text(&env_account(id), &raw).await?;
    }
    match (&valid.secret, needs_secret) {
        (Some(secret), true) if !secret.is_empty() => {
            state.vault.set_text(&secret_account(id), secret).await?
        }
        (_, false) => state.vault.delete_text(&secret_account(id)).await?,
        _ => {}
    }
    let url_changed = previous.as_ref().is_some_and(|p| p.url != valid.url);
    if valid.auth != McpAuth::Oauth || url_changed {
        state.vault.delete_text(&oauth_account(id)).await?;
    }

    // A changed configuration takes effect on the next connection.
    state.mcp.disconnect(id);
    state.events.mcp_changed();
    get(state, id)
}

/// The apps on this computer with MCP servers ReMa can add (Claude
/// Desktop, Claude Code, Cursor, VS Code, Windsurf). Names and programs
/// only; values stay in Rust.
pub async fn import_sources(state: &AppState) -> AppResult<Vec<McpImportSource>> {
    let env = import::environment().await;
    let existing = server_names(state)?;
    let lookup = move |name: &str| env.get(name).cloned();
    tokio::task::spawn_blocking(move || import::sources(&lookup, &existing))
        .await
        .map_err(|e| AppError::internal(e.to_string()))
}

/// The servers in pasted JSON, as `import_sources` shows them.
pub async fn preview_import(state: &AppState, json: &str) -> AppResult<Vec<McpImportCandidate>> {
    let value = import::request_config(&McpImportRequest {
        app: None,
        json: Some(json.to_string()),
        names: Vec::new(),
    })?;
    let env = import::environment().await;
    let lookup = |name: &str| env.get(name).cloned();
    let found = import::candidates(&value, &lookup, &server_names(state)?);
    if found.is_empty() {
        return Err(AppError::validation(
            "No MCP servers were found. Paste an mcpServers block, e.g. from              claude_desktop_config.json.",
        ));
    }
    Ok(found)
}

/// Adds the chosen servers, turned on unless they were off in the other
/// app. Servers whose name is already in ReMa are left alone.
pub async fn import(state: &AppState, request: McpImportRequest) -> AppResult<McpImportResult> {
    let value = import::request_config(&request)?;
    let env = import::environment().await;
    let lookup = |name: &str| env.get(name).cloned();
    let servers = import::servers_in(&value);
    let mut existing = server_names(state)?;
    let mut result = McpImportResult {
        added: Vec::new(),
        skipped: Vec::new(),
    };
    for name in &request.names {
        let skip = |reason: String| McpImportSkip {
            name: name.clone(),
            reason,
        };
        let Some((_, config)) = servers.iter().find(|(n, _)| n == name) else {
            result
                .skipped
                .push(skip("It is no longer in that configuration.".into()));
            continue;
        };
        if existing.iter().any(|e| e.eq_ignore_ascii_case(name.trim())) {
            result
                .skipped
                .push(skip("A server with this name is already in ReMa.".into()));
            continue;
        }
        let (input, disabled) = match import::to_input(name, config, &lookup) {
            Ok(found) => found,
            Err(reason) => {
                result.skipped.push(skip(reason));
                continue;
            }
        };
        let server = match save(state, None, input).await {
            Ok(server) => server,
            Err(error) => {
                result.skipped.push(skip(error.to_string()));
                continue;
            }
        };
        existing.push(server.name.clone());
        let server = if disabled {
            server
        } else {
            set_enabled(state, server.id, true)?
        };
        result.added.push(server);
    }
    Ok(result)
}

fn server_names(state: &AppState) -> AppResult<Vec<String>> {
    Ok(list(state)?.into_iter().map(|s| s.name).collect())
}

pub async fn delete(state: &AppState, id: i64) -> AppResult<()> {
    state.mcp.disconnect(id);
    state.db.call(|c| repo::delete(c, id))?;
    for account in [env_account(id), secret_account(id), oauth_account(id)] {
        state.vault.delete_text(&account).await?;
    }
    state.events.mcp_changed();
    state.events.conversations_changed();
    Ok(())
}

/// Turns a server on or off. Turning it off closes its connection and
/// removes it from every chat's tools; turning it on connects in the
/// background so Settings shows whether it works.
pub fn set_enabled(state: &AppState, id: i64, enabled: bool) -> AppResult<McpServer> {
    state
        .db
        .call(|c| repo::set_enabled(c, id, enabled, now_ms()))?;
    if enabled {
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let _ = connect_server(&state, id).await;
        });
    } else {
        state.mcp.disconnect(id);
    }
    state.events.mcp_changed();
    get(state, id)
}

pub fn oauth_store(state: &AppState, id: i64) -> Arc<dyn CredentialStore> {
    Arc::new(KeychainCredentials::new(
        state.vault.clone(),
        oauth_account(id),
    ))
}

/// A stored server's full configuration, secrets included.
pub async fn server_config(state: &AppState, row: &McpServerRow) -> AppResult<ServerConfig> {
    let secret = match row.auth {
        McpAuth::Bearer | McpAuth::Header => state.vault.get_text(&secret_account(row.id)).await?,
        _ => None,
    };
    let env = if row.transport == McpTransport::Stdio {
        stored_env(state, row.id).await?.into_iter().collect()
    } else {
        Vec::new()
    };
    Ok(ServerConfig {
        id: row.id,
        name: row.name.clone(),
        transport: row.transport,
        command: row.command.clone(),
        args: row.args.clone(),
        env,
        cwd: row.cwd.clone(),
        url: row.url.clone(),
        auth: row.auth,
        header_name: row.header_name.clone(),
        secret,
    })
}

/// Connects an enabled server (or reuses its connection).
pub async fn connect_server(
    state: &AppState,
    id: i64,
) -> Result<Arc<client::Connection>, ConnectError> {
    let row = state
        .db
        .call(|c| repo::get(c, id))
        .map_err(|e| ConnectError::Failed(e.to_string()))?;
    if !row.enabled {
        return Err(ConnectError::Failed(format!(
            "{} is turned off in Settings.",
            row.name
        )));
    }
    let config = server_config(state, &row)
        .await
        .map_err(|e| ConnectError::Failed(e.to_string()))?;
    let store = (row.auth == McpAuth::Oauth).then(|| oauth_store(state, id));
    let events = state.events.clone();
    let result = state
        .mcp
        .connect(&config, &state.info.version, store, &move || {
            events.mcp_changed()
        })
        .await;
    // A server added by URL alone that asks for authorization is an OAuth
    // server from now on: Sign in appears on its row, and the sign-in runs
    // only when the user clicks it.
    if let (Err(ConnectError::NeedsSignIn(_)), McpAuth::None, true) =
        (&result, row.auth, row.transport.is_remote())
    {
        if let Err(error) = state
            .db
            .call(|c| repo::set_auth(c, id, McpAuth::Oauth, now_ms()))
        {
            eprintln!("[mcp] could not switch {} to OAuth: {error}", row.name);
        }
        state.events.mcp_changed();
    }
    result
}

/// Connect / Reconnect in Settings.
pub async fn reconnect(state: &AppState, id: i64) -> AppResult<McpServer> {
    state.mcp.disconnect(id);
    let _ = connect_server(state, id).await;
    get(state, id)
}

pub fn disconnect(state: &AppState, id: i64) -> AppResult<McpServer> {
    state.mcp.disconnect(id);
    state.events.mcp_changed();
    get(state, id)
}

/// Tries the configuration in the form (secrets left unchanged come from
/// the stored server) with a fresh connection, then closes it.
pub async fn test(
    state: &AppState,
    id: Option<i64>,
    input: McpServerInput,
) -> AppResult<McpTestResult> {
    let valid = config::validate(&input)?;
    let env = resolve_env(state, id, &valid).await?;
    let secret = match (&valid.secret, id) {
        (Some(secret), _) if !secret.is_empty() => Some(secret.clone()),
        (_, Some(id)) => state.vault.get_text(&secret_account(id)).await?,
        _ => None,
    };
    let config = ServerConfig {
        id: id.unwrap_or(0),
        name: valid.name.clone(),
        transport: valid.transport,
        command: valid.command,
        args: valid.args,
        env: env.into_iter().collect(),
        cwd: valid.cwd,
        url: valid.url,
        auth: valid.auth,
        header_name: valid.header_name,
        secret,
    };
    let store = match (config.auth, id) {
        (McpAuth::Oauth, Some(id)) => Some(oauth_store(state, id)),
        _ => None,
    };
    Ok(
        match client::connect(&config, &state.info.version, store).await {
            Ok(conn) => {
                let tools = conn.tool_infos();
                conn.close();
                let version = conn
                    .protocol_version
                    .as_deref()
                    .map(|v| format!(" (MCP {v})"))
                    .unwrap_or_default();
                McpTestResult {
                    ok: true,
                    message: format!(
                        "Connected{version}. {} tool{} available.",
                        tools.len(),
                        if tools.len() == 1 { "" } else { "s" }
                    ),
                    tools,
                    requires_sign_in: false,
                }
            }
            Err(ConnectError::NeedsSignIn(_)) if id.is_none() => McpTestResult {
                ok: false,
                message: "This server requires sign-in. Save it, then click Sign in: the \
                          sign-in opens in your browser."
                    .into(),
                tools: Vec::new(),
                requires_sign_in: true,
            },
            Err(ConnectError::NeedsSignIn(message)) => McpTestResult {
                ok: false,
                message,
                tools: Vec::new(),
                requires_sign_in: true,
            },
            Err(error) => McpTestResult {
                ok: false,
                message: error.to_string(),
                tools: Vec::new(),
                requires_sign_in: false,
            },
        },
    )
}

/// OAuth sign-in in the browser, then connects. `open` shows the page.
pub async fn sign_in(
    state: &AppState,
    id: i64,
    open: impl Fn(&str) -> AppResult<()>,
) -> AppResult<McpServer> {
    let mut row = state.db.call(|c| repo::get(c, id))?;
    if row.auth == McpAuth::None && row.transport == McpTransport::Http {
        // Sign in on a server added by URL alone: it is an OAuth server.
        state
            .db
            .call(|c| repo::set_auth(c, id, McpAuth::Oauth, now_ms()))?;
        row.auth = McpAuth::Oauth;
    }
    if row.auth != McpAuth::Oauth {
        return Err(AppError::validation("This server does not use OAuth."));
    }
    state.mcp.disconnect(id);
    let cancel = state.mcp.begin_sign_in(id);
    state.events.mcp_changed();
    let result = oauth::sign_in(&row.name, &row.url, oauth_store(state, id), open, &cancel).await;
    state.mcp.end_sign_in(id);
    state.events.mcp_changed();
    result?;
    if row.enabled {
        let _ = connect_server(state, id).await;
    }
    get(state, id)
}

pub fn cancel_sign_in(state: &AppState, id: i64) -> AppResult<McpServer> {
    state.mcp.cancel_sign_in(id);
    state.events.mcp_changed();
    get(state, id)
}

/// Forgets the OAuth credentials and closes the connection.
pub async fn sign_out(state: &AppState, id: i64) -> AppResult<McpServer> {
    state.mcp.disconnect(id);
    state.vault.delete_text(&oauth_account(id)).await?;
    state.events.mcp_changed();
    get(state, id)
}

/// The enabled servers among `ids`, in order; disabled or removed ones are
/// dropped (they are never exposed to a model).
pub fn enabled_selection(state: &AppState, ids: &[i64]) -> AppResult<Vec<McpServerRow>> {
    let rows = state.db.call(|c| repo::list(c))?;
    let mut out: Vec<McpServerRow> = Vec::new();
    for id in ids {
        if out.iter().any(|r| r.id == *id) {
            continue;
        }
        if let Some(row) = rows.iter().find(|r| r.id == *id && r.enabled) {
            out.push(row.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{llm::fake::FakeLanguageModel, models::mcp::McpEnvVar, state::testing};

    fn state() -> AppState {
        testing::state(Arc::new(FakeLanguageModel::replying(&[]))).0
    }

    fn remote(name: &str, auth: McpAuth, secret: Option<&str>) -> McpServerInput {
        McpServerInput {
            name: name.into(),
            transport: McpTransport::Http,
            command: String::new(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: String::new(),
            url: "http://127.0.0.1:9/mcp".into(),
            auth,
            header_name: String::new(),
            secret: secret.map(str::to_string),
        }
    }

    fn local(name: &str, env: Vec<McpEnvVar>) -> McpServerInput {
        McpServerInput {
            transport: McpTransport::Stdio,
            command: "npx".into(),
            args: vec!["-y".into(), "some-server".into()],
            env,
            url: String::new(),
            ..remote(name, McpAuth::None, None)
        }
    }

    #[tokio::test]
    async fn saves_servers_disabled_with_secrets_in_the_keychain() {
        let state = state();
        let server = save(
            &state,
            None,
            local(
                "LinkedIn MCP",
                vec![McpEnvVar {
                    name: "API_KEY".into(),
                    value: Some("s3cret".into()),
                }],
            ),
        )
        .await
        .unwrap();
        assert!(
            !server.enabled,
            "new servers are never enabled automatically"
        );
        assert_eq!(server.status.state, McpState::Disabled);
        assert_eq!(server.env_names, ["API_KEY"]);
        // The value is only in the credential store, never in the row.
        let raw_row: String = state
            .db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT name || command || args || env_names || url FROM mcp_servers",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert!(!raw_row.contains("s3cret"));
        let config = server_config(&state, &state.db.call(|c| repo::get(c, server.id)).unwrap())
            .await
            .unwrap();
        assert_eq!(
            config.env,
            vec![("API_KEY".to_string(), "s3cret".to_string())]
        );

        // Editing without a new value keeps the stored one.
        let mut edit = local(
            "LinkedIn MCP",
            vec![McpEnvVar {
                name: "API_KEY".into(),
                value: None,
            }],
        );
        edit.args.push("--verbose".into());
        let edited = save(&state, Some(server.id), edit).await.unwrap();
        assert_eq!(edited.args.len(), 3);
        assert_eq!(
            stored_env(&state, server.id).await.unwrap()["API_KEY"],
            "s3cret"
        );

        // A new variable needs a value.
        let missing = local(
            "LinkedIn MCP",
            vec![McpEnvVar {
                name: "OTHER".into(),
                value: None,
            }],
        );
        assert!(save(&state, Some(server.id), missing).await.is_err());

        // Names are unique.
        assert!(save(&state, None, local("linkedin mcp", Vec::new()))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn token_servers_need_a_token_and_forget_it_when_switching() {
        let state = state();
        assert!(save(&state, None, remote("Jobs", McpAuth::Bearer, None))
            .await
            .is_err());
        let server = save(&state, None, remote("Jobs", McpAuth::Bearer, Some("tok")))
            .await
            .unwrap();
        assert!(server.has_secret);
        assert_eq!(
            state
                .vault
                .get_text(&secret_account(server.id))
                .await
                .unwrap()
                .as_deref(),
            Some("tok")
        );
        // Keeping the token on edit.
        save(
            &state,
            Some(server.id),
            remote("Jobs", McpAuth::Bearer, None),
        )
        .await
        .unwrap();
        assert!(state
            .vault
            .get_text(&secret_account(server.id))
            .await
            .unwrap()
            .is_some());
        // Switching to no authentication removes it.
        save(&state, Some(server.id), remote("Jobs", McpAuth::None, None))
            .await
            .unwrap();
        assert!(state
            .vault
            .get_text(&secret_account(server.id))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn saves_and_tests_servers_on_the_older_http_sse_transport() {
        let state = state();
        let input = McpServerInput {
            transport: McpTransport::Sse,
            ..remote("Old", McpAuth::Bearer, Some("tok"))
        };
        let server = save(&state, None, input.clone()).await.unwrap();
        assert_eq!(server.transport, McpTransport::Sse);
        assert!(server.has_secret);
        assert_eq!(get(&state, server.id).unwrap().transport, McpTransport::Sse);
        // Nothing listens there: the stream is asked for directly (no
        // Streamable HTTP probe), and the failure names the connection.
        let result = test(&state, Some(server.id), input).await.unwrap();
        assert!(!result.ok);
        assert!(
            result.message.starts_with("Could not connect:"),
            "{}",
            result.message
        );
        // OAuth is refused at the form.
        let oauth = McpServerInput {
            transport: McpTransport::Sse,
            ..remote("Old OAuth", McpAuth::Oauth, None)
        };
        let error = save(&state, None, oauth).await.unwrap_err().to_string();
        assert!(error.contains("older HTTP+SSE transport"), "{error}");
    }

    #[tokio::test]
    async fn enabling_disabling_and_removing() {
        let state = state();
        let a = save(&state, None, remote("A", McpAuth::None, None))
            .await
            .unwrap();
        let b = save(&state, None, remote("B", McpAuth::None, None))
            .await
            .unwrap();
        let c = save(&state, None, remote("C", McpAuth::None, None))
            .await
            .unwrap();

        // Only enabled servers can be made available to a model.
        assert!(enabled_selection(&state, &[a.id, b.id, c.id])
            .unwrap()
            .is_empty());
        set_enabled(&state, a.id, true).unwrap();
        set_enabled(&state, c.id, true).unwrap();
        let selected: Vec<i64> = enabled_selection(&state, &[c.id, b.id, a.id, c.id])
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(
            selected,
            [c.id, a.id],
            "selection order kept, disabled dropped"
        );

        let off = set_enabled(&state, c.id, false).unwrap();
        assert_eq!(off.status.state, McpState::Disabled);
        assert_eq!(
            enabled_selection(&state, &[c.id, a.id]).unwrap()[0].id,
            a.id
        );

        state
            .vault
            .set_text(&oauth_account(a.id), "{}")
            .await
            .unwrap();
        delete(&state, a.id).await.unwrap();
        assert!(get(&state, a.id).is_err());
        assert!(state
            .vault
            .get_text(&oauth_account(a.id))
            .await
            .unwrap()
            .is_none());
    }

    fn fixture(era: &str, extra: Vec<McpEnvVar>) -> Option<McpServerInput> {
        let ok = std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            eprintln!("node is not installed; skipping");
            return None;
        }
        let mut env = vec![McpEnvVar {
            name: "FAKE_MCP_ERA".into(),
            value: Some(era.into()),
        }];
        env.extend(extra);
        Some(McpServerInput {
            args: vec![concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/mcp_test_server.mjs"
            )
            .into()],
            command: "node".into(),
            env,
            ..local("Fixture", Vec::new())
        })
    }

    #[tokio::test]
    async fn adds_servers_from_another_apps_configuration() {
        let state = state();
        std::env::set_var("MCP_IMPORT_TEST_TOKEN", "tok-123");
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/mcp_test_server.mjs"
        );
        let json = serde_json::json!({ "mcpServers": {
            "Fixture": {
                "command": "node",
                "args": [fixture],
                "env": { "FAKE_MCP_ERA": "modern", "FAKE_MCP_TOKEN": "${MCP_IMPORT_TEST_TOKEN}" }
            },
            "Remote": {
                "type": "http",
                "url": "https://mcp.example.com/mcp",
                "headers": { "Authorization": "Bearer ${MCP_IMPORT_TEST_TOKEN}" },
                "disabled": true
            },
            "Shell": { "command": "bash", "args": ["-c", "x"] }
        }})
        .to_string();

        let preview = preview_import(&state, &json).await.unwrap();
        assert_eq!(preview.len(), 3);
        assert!(preview[2].problem.is_some());
        assert!(!serde_json::to_string(&preview).unwrap().contains("tok-123"));

        let request = |names: &[&str]| McpImportRequest {
            app: None,
            json: Some(json.clone()),
            names: names.iter().map(|n| n.to_string()).collect(),
        };
        let result = import(&state, request(&["Fixture", "Remote", "Shell"]))
            .await
            .unwrap();
        let added: Vec<(&str, bool)> = result
            .added
            .iter()
            .map(|s| (s.name.as_str(), s.enabled))
            .collect();
        // Turned on unless it was off in the other app.
        assert_eq!(added, [("Fixture", true), ("Remote", false)]);
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("shell"));
        let remote = &result.added[1];
        assert_eq!((remote.auth, remote.has_secret), (McpAuth::Bearer, true));
        assert_eq!(
            state
                .vault
                .get_text(&secret_account(remote.id))
                .await
                .unwrap()
                .as_deref(),
            Some("tok-123")
        );

        // The imported local server works, with its variables filled in.
        if which_node().is_some() {
            let conn = connect_server(&state, result.added[0].id).await.unwrap();
            let (text, _) = conn
                .call(
                    "env",
                    Default::default(),
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await
                .unwrap();
            let seen: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(seen["token"], "tok-123");
        }

        // Importing again leaves the servers alone.
        let again = import(&state, request(&["Fixture"])).await.unwrap();
        assert!(again.added.is_empty());
        assert!(again.skipped[0].reason.contains("already"));
    }

    fn which_node() -> Option<std::path::PathBuf> {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|d| d.join("node"))
                .find(|p| p.is_file())
        })
    }

    #[tokio::test]
    async fn tests_modern_and_legacy_servers_over_stdio() {
        let state = state();
        for (era, version) in [("modern", "2026-07-28"), ("legacy", "2025-11-25")] {
            let Some(input) = fixture(era, Vec::new()) else {
                return;
            };
            let result = test(&state, None, input).await.unwrap();
            assert!(result.ok, "{era}: {}", result.message);
            assert!(
                result.message.contains(version),
                "{era}: {}",
                result.message
            );
            let names: Vec<&str> = result.tools.iter().map(|t| t.name.as_str()).collect();
            assert_eq!(names, ["echo", "add_note", "env"]);
            assert!(result.tools[0].read_only);
            assert!(!result.tools[1].read_only);
        }
    }

    #[tokio::test]
    async fn local_servers_get_the_users_environment_and_their_own() {
        let state = state();
        // Like Claude Code: the user's environment reaches the server (a
        // token or proxy set in a shell profile), ReMa's own settings do not.
        std::env::set_var("REMA_PARENT_SECRET", "parent-only");
        std::env::set_var("MCP_FIXTURE_USER_SETTING", "from-the-user");
        let Some(input) = fixture(
            "modern",
            vec![McpEnvVar {
                name: "FAKE_MCP_TOKEN".into(),
                value: Some("server-token".into()),
            }],
        ) else {
            return;
        };
        let server = save(&state, None, input).await.unwrap();
        set_enabled(&state, server.id, true).unwrap();
        let conn = connect_server(&state, server.id).await.unwrap();
        let (text, is_error) = conn
            .call(
                "env",
                Default::default(),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!is_error);
        let seen: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(seen["token"], "server-token");
        assert_eq!(seen["parentSecret"], serde_json::Value::Null);
        assert_eq!(seen["userSetting"], "from-the-user");
        assert_eq!(seen["hasPath"], true);
        assert_eq!(
            get(&state, server.id).unwrap().status.state,
            McpState::Connected
        );
        assert_eq!(
            get(&state, server.id)
                .unwrap()
                .status
                .protocol_version
                .as_deref(),
            Some("2026-07-28")
        );

        // Disabling stops the server; its connection is gone.
        set_enabled(&state, server.id, false).unwrap();
        assert!(state.mcp.live(server.id).is_none());
        assert!(conn.is_closed());
    }

    #[tokio::test]
    async fn reports_why_a_server_failed_to_start() {
        let state = state();
        let Some(input) = fixture(
            "modern",
            vec![McpEnvVar {
                name: "FAKE_MCP_CRASH".into(),
                value: Some("1".into()),
            }],
        ) else {
            return;
        };
        let result = test(&state, None, input).await.unwrap();
        assert!(!result.ok);
        assert!(
            result.message.contains("missing configuration"),
            "{}",
            result.message
        );

        let mut missing = local("Missing", Vec::new());
        missing.command = "rema-no-such-program".into();
        let result = test(&state, None, missing).await.unwrap();
        assert!(!result.ok);
        assert!(
            result.message.contains("was not found"),
            "{}",
            result.message
        );
    }
}
