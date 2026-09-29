//! Live smoke test against a real remote MCP server that requires OAuth
//! sign-in: add by URL → the server is discovered as requiring
//! authentication → sign in in the system browser → tools/list → one
//! tools/call → "restart" → reconnects without the browser. Interactive:
//! a person signs in when the browser opens.
//!
//! ```sh
//! REMA_LIVE_MCP_URL=https://mcp.example.com/mcp pnpm test:live:mcp
//! ```
//!
//! `REMA_LIVE_MCP_TOOL` names a tool to call with empty arguments
//! (optional; the first read-only tool is used otherwise, and none when
//! every tool may write). Credentials stay in the test's in-memory
//! credential store.

use std::sync::Arc;

use crate::{
    error::{AppError, AppResult},
    llm::fake::FakeLanguageModel,
    mcp::client::ConnectError,
    models::mcp::{McpAuth, McpServerInput, McpState, McpTransport},
    services::mcp as service,
    state::testing,
};

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn open_browser(url: &str) -> AppResult<()> {
    println!("Opening the system browser for the sign-in (finish it there)…");
    open::that(url).map_err(|e| AppError::internal(format!("could not open the browser: {e}")))
}

#[tokio::test]
#[ignore = "interactive live sign-in; run with pnpm test:live:mcp"]
async fn remote_mcp_server_with_oauth() {
    let Some(url) = var("REMA_LIVE_MCP_URL") else {
        println!(
            "BLOCKED remote MCP discover → sign in → tools/list → tools/call: set \
             REMA_LIVE_MCP_URL to run it."
        );
        return;
    };
    let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
    let server = service::save(
        &state,
        None,
        McpServerInput {
            name: "Live MCP".into(),
            transport: McpTransport::Http,
            command: String::new(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: String::new(),
            url: url.clone(),
            auth: McpAuth::None,
            header_name: String::new(),
            secret: None,
        },
    )
    .await
    .unwrap();
    let id = server.id;
    // Enabled in the database only: `service::set_enabled` also connects in
    // the background, which would race with the explicit probe below.
    state
        .db
        .call(|c| crate::db::mcp::set_enabled(c, id, true, crate::time::now_ms()))
        .unwrap();

    println!("1. Probe {url}…");
    match service::connect_server(&state, id).await {
        Ok(_) => {
            println!("   the server needs no sign-in; nothing to test here");
            return;
        }
        Err(ConnectError::NeedsSignIn(message)) => {
            println!("   requires authentication: {message}")
        }
        Err(ConnectError::Failed(message)) => panic!("could not probe the server: {message}"),
    }
    assert_eq!(service::get(&state, id).unwrap().auth, McpAuth::Oauth);

    println!("2. Sign in (browser)…");
    let server = service::sign_in(&state, id, open_browser)
        .await
        .unwrap_or_else(|e| panic!("sign-in failed: {e}"));
    assert_eq!(
        server.status.state,
        McpState::Connected,
        "{:?}",
        server.status
    );
    println!(
        "   connected ({}), {} tool(s): {}",
        server
            .status
            .server_info
            .as_deref()
            .unwrap_or("unnamed server"),
        server.status.tools.len(),
        server
            .status
            .tools
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    println!("3. tools/call…");
    let tool = var("REMA_LIVE_MCP_TOOL").or_else(|| {
        server
            .status
            .tools
            .iter()
            .find(|t| t.read_only)
            .map(|t| t.name.clone())
    });
    match tool {
        Some(tool) => {
            let connection = state.mcp.live(id).expect("live");
            let (text, is_error) = connection
                .call(
                    &tool,
                    serde_json::Map::new(),
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await
                .unwrap_or_else(|e| panic!("{tool} failed: {e}"));
            println!(
                "   {tool}: {}{}",
                if is_error { "error: " } else { "" },
                text.chars().take(200).collect::<String>()
            );
        }
        None => println!("   skipped: no read-only tool and REMA_LIVE_MCP_TOOL not set"),
    }

    println!("4. Restart: reconnect from the stored sign-in, no browser…");
    state.mcp.disconnect(id);
    service::connect_server(&state, id).await.unwrap();
    assert_eq!(
        service::get(&state, id).unwrap().status.state,
        McpState::Connected
    );
    println!("OK remote MCP: discover → sign in → tools/list → tools/call → restart");
}
