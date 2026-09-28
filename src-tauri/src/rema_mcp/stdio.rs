//! ReMa MCP for other apps: `ReMa mcp` serves ReMa's job tools over stdio
//! to Claude Desktop, Claude Code, Codex, Cursor, VS Code or any other MCP
//! client, from the same data folder as the app (its job cache and
//! settings). No window opens, and standard output carries only MCP
//! messages (diagnostics go to standard error).
//!
//! The other app's model has its own web search, so ReMa MCP searches
//! ReMa's job sources and the optional search service from Settings.
//! Turning ReMa MCP off in ReMa's Settings stops its tools here too.

use std::{path::PathBuf, time::Duration};

use rmcp::ServiceExt;
use serde::Serialize;
use specta::Type;
use tokio_util::sync::CancellationToken;

use serde_json::{json, Value};

use super::{
    engine::{Discovery, Session},
    server::RemaMcpServer,
};
use crate::{
    error::{AppError, AppResult},
    events::NullEvents,
    mcp::import::{config_path, McpImportApp},
    state::{AppInfo, AppState},
};

/// The first argument that starts ReMa MCP instead of the app.
pub const ARG: &str = "mcp";
const DATA_DIR_ARG: &str = "--data-dir";
/// ReMa's bundle identifier (`tauri.conf.json`), the name of its data folder.
const IDENTIFIER: &str = "cloud.datasound.rema";
/// How often the switch in ReMa's Settings is read again.
const SETTING_CHECK: Duration = Duration::from_secs(5);

pub fn requested() -> bool {
    std::env::args_os().nth(1).is_some_and(|a| a == ARG)
}

/// The folder the app keeps its data in (Tauri's `app_data_dir`).
pub fn default_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|h| !h.is_empty())
        .map(PathBuf::from);
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.map(|h| h.join(".local").join("share")))
    }?;
    Some(base.join(IDENTIFIER))
}

fn data_dir_from_args() -> Option<PathBuf> {
    let args: Vec<_> = std::env::args_os().collect();
    args.iter()
        .position(|a| a == DATA_DIR_ARG)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}

/// Runs ReMa MCP until the other app closes it; the process exit code.
pub fn run() -> i32 {
    let Some(data_dir) = data_dir_from_args().or_else(default_data_dir) else {
        eprintln!("ReMa MCP: ReMa's data folder could not be found.");
        return 1;
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("ReMa MCP: could not start: {error}");
            return 1;
        }
    };
    match runtime.block_on(serve(data_dir)) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("ReMa MCP: {message}");
            1
        }
    }
}

async fn serve(data_dir: PathBuf) -> Result<(), String> {
    let state = crate::build_state(
        data_dir,
        AppInfo {
            name: "ReMa".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        std::sync::Arc::new(NullEvents),
    )
    .map_err(|e| format!("ReMa's data could not be opened: {e}"))?;
    serve_on(state, tokio::io::stdin(), tokio::io::stdout()).await
}

/// Serves ReMa MCP over one byte stream pair until the client closes it or
/// ReMa MCP is turned off in Settings.
pub async fn serve_on<R, W>(state: AppState, input: R, output: W) -> Result<(), String>
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    if !super::ensure_default(&state).map_err(|e| e.to_string())? {
        return Err(
            "ReMa MCP is turned off. Turn it on in ReMa → Settings → MCP → Built in.".into(),
        );
    }
    let discovery = Discovery::beside_model_search(&state).await;
    let cancel = CancellationToken::new();
    // The switch in ReMa's Settings, read again now and then: turning ReMa
    // MCP off there stops it here.
    let watch = {
        let (state, cancel) = (state.clone(), cancel.clone());
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(SETTING_CHECK) => {}
                }
                if matches!(super::ensure_default(&state), Ok(false)) {
                    eprintln!("ReMa MCP: turned off in ReMa's Settings.");
                    cancel.cancel();
                    return;
                }
            }
        })
    };
    let server = RemaMcpServer::new(Session { state, discovery }, cancel.clone());
    let running = server
        .serve((input, output))
        .await
        .map_err(|e| format!("the MCP session could not start: {e}"))?;
    tokio::select! {
        _ = cancel.cancelled() => {}
        _ = running.waiting() => {}
    }
    cancel.cancel();
    watch.abort();
    Ok(())
}

/// How another app starts ReMa MCP (Settings shows it, with copies for
/// Claude Desktop, Claude Code, Codex and Cursor).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RemaMcpLaunch {
    pub command: String,
    pub args: Vec<String>,
}

/// This ReMa's program with the `mcp` argument (and its data folder when it
/// is not the usual one, e.g. a development build).
pub fn launch(state: &AppState) -> Option<RemaMcpLaunch> {
    let exe = std::env::current_exe().ok()?;
    let mut args = vec![ARG.to_string()];
    if default_data_dir().as_deref() != Some(state.data_dir.as_path()) {
        args.push(DATA_DIR_ARG.into());
        args.push(state.data_dir.to_string_lossy().into_owned());
    }
    Some(RemaMcpLaunch {
        command: exe.to_string_lossy().into_owned(),
        args,
    })
}

/// The name ReMa MCP gets in other apps (Claude Code allows letters,
/// digits, `-` and `_`).
pub const CLIENT_NAME: &str = "rema";

/// Adds ReMa MCP to another app's MCP settings file (Claude Desktop,
/// Cursor, VS Code, Windsurf), keeping its other servers; the file is backed
/// up first. Claude Code manages its own file: it gets a command to run.
pub fn add_to(state: &AppState, app: McpImportApp) -> AppResult<PathBuf> {
    let launch =
        launch(state).ok_or_else(|| AppError::internal("ReMa's program could not be located"))?;
    let key = match app {
        McpImportApp::VsCode => "servers",
        McpImportApp::ClaudeDesktop | McpImportApp::Cursor | McpImportApp::Windsurf => "mcpServers",
        McpImportApp::ClaudeCode => {
            return Err(AppError::validation(
                "Add ReMa MCP to Claude Code with the claude mcp add command shown here.",
            ))
        }
    };
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or_else(|| AppError::internal("no home folder"))?;
    let path = config_path(app, &home);
    write_entry(&path, app, key, &launch)?;
    Ok(path)
}

/// Puts ReMa MCP under `key` in the JSON file at `path`.
fn write_entry(
    path: &std::path::Path,
    app: McpImportApp,
    key: &str,
    launch: &RemaMcpLaunch,
) -> AppResult<()> {
    let mut root: Value = match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => serde_json::from_str(&text).map_err(|_| {
            AppError::validation(format!(
                "{} could not be read as JSON; add ReMa MCP to it by hand.",
                path.display()
            ))
        })?,
        _ => json!({}),
    };
    let Some(object) = root.as_object_mut() else {
        return Err(AppError::validation(format!(
            "{} is not an MCP settings file; add ReMa MCP to it by hand.",
            path.display()
        )));
    };
    let servers = object.entry(key).or_insert_with(|| json!({}));
    let Some(servers) = servers.as_object_mut() else {
        return Err(AppError::validation(format!(
            "The {key} in {} is not a list of servers; add ReMa MCP by hand.",
            path.display()
        )));
    };
    let mut entry = json!({ "command": &launch.command, "args": &launch.args });
    if app == McpImportApp::VsCode {
        entry["type"] = json!("stdio");
    }
    servers.insert(CLIENT_NAME.into(), entry);

    if path.exists() {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".rema-backup");
        std::fs::copy(path, &backup)?;
    } else if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text =
        serde_json::to_string_pretty(&root).map_err(|e| AppError::internal(e.to_string()))?;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".rema-tmp");
    std::fs::write(&temporary, text + "\n")?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, mcp::client, state::testing};

    #[tokio::test]
    async fn another_app_lists_and_calls_rema_mcp_over_stdio() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let (read, write) = tokio::io::split(server_io);
        let serving = tokio::spawn(serve_on(state.clone(), read, write));

        let connection = client::connect_stream(client_io, "test").await.unwrap();
        let names: Vec<String> = connection
            .tools
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert_eq!(
            names,
            [
                "search_jobs",
                "get_job",
                "get_jobs",
                "search_similar_jobs",
                "source_status"
            ]
        );
        let (text, is_error) = connection
            .call(
                "source_status",
                Default::default(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!is_error, "{text}");

        // The other app closes the session: ReMa MCP ends cleanly.
        connection.close();
        assert!(tokio::time::timeout(Duration::from_secs(5), serving)
            .await
            .unwrap()
            .unwrap()
            .is_ok());
    }

    #[tokio::test]
    async fn does_not_start_while_turned_off_in_settings() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        super::super::set_enabled(&state, false).unwrap();
        let (server_io, _client_io) = tokio::io::duplex(1024);
        let (read, write) = tokio::io::split(server_io);
        let error = serve_on(state, read, write).await.unwrap_err();
        assert!(error.contains("turned off"), "{error}");
    }

    #[test]
    fn adds_itself_to_another_apps_settings_and_keeps_its_servers() {
        let dir = testing::temp_dir();
        let path = dir.join("Claude").join("claude_desktop_config.json");
        let launch = RemaMcpLaunch {
            command: "/Applications/ReMa.app/Contents/MacOS/ReMa".into(),
            args: vec!["mcp".into()],
        };
        // A new file.
        write_entry(&path, McpImportApp::ClaudeDesktop, "mcpServers", &launch).unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["mcpServers"]["rema"]["args"], json!(["mcp"]));

        // An existing file: other servers and settings stay, a backup is made.
        std::fs::write(
            &path,
            r#"{"globalShortcut":"x","mcpServers":{"github":{"command":"npx","args":["gh"]}}}"#,
        )
        .unwrap();
        write_entry(&path, McpImportApp::ClaudeDesktop, "mcpServers", &launch).unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["globalShortcut"], "x");
        assert_eq!(written["mcpServers"]["github"]["command"], "npx");
        assert_eq!(
            written["mcpServers"]["rema"]["command"],
            "/Applications/ReMa.app/Contents/MacOS/ReMa"
        );
        assert!(dir
            .join("Claude")
            .join("claude_desktop_config.json.rema-backup")
            .is_file());

        // VS Code names the transport.
        let vscode = dir.join("mcp.json");
        write_entry(&vscode, McpImportApp::VsCode, "servers", &launch).unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&vscode).unwrap()).unwrap();
        assert_eq!(written["servers"]["rema"]["type"], "stdio");

        // A file that is not JSON is left alone.
        std::fs::write(&vscode, "not json").unwrap();
        assert!(write_entry(&vscode, McpImportApp::VsCode, "servers", &launch).is_err());
        assert_eq!(std::fs::read_to_string(&vscode).unwrap(), "not json");
    }
}
