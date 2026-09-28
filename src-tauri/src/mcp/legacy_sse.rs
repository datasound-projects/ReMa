//! MCP's HTTP+SSE transport (protocol 2024-11-05), for servers that have
//! not moved to Streamable HTTP. rmcp removed it (0.11); the MCP
//! specification still describes how a client reaches such a server
//! ("Backwards compatibility" in Transports): when a POST of `initialize`
//! to the URL fails, GET the URL expecting an event stream whose first
//! event, `endpoint`, names where to POST messages; replies arrive as
//! `message` events on that stream. Claude Code offers the same
//! (`--transport sse`, deprecated).
//!
//! Only what the specification defines, with ReMa's limits: the message
//! address must be on the stream's own origin (a server cannot send ReMa's
//! requests, and its token, to another site), the stream must name it
//! within 20 seconds, at most a few messages may come before it, and the
//! usual connect and call timeouts apply. OAuth sign-in is not offered on
//! this transport; a server that asks for authentication gets a token
//! configured in its settings.
//!
//! When the event stream ends, the session is over: `receive` yields
//! `None`, rmcp fails the calls still waiting with "transport closed" and
//! marks the connection closed, and the next use of the server opens a
//! new session (see `McpContext::connect`).

use std::{borrow::Cow, fmt, future::Future, time::Duration};

use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, ACCEPT, CONTENT_TYPE},
    StatusCode, Url,
};
use rmcp::{
    model::{ClientJsonRpcMessage, ServerJsonRpcMessage},
    service::RoleClient,
    transport::Transport,
};
use tokio::sync::mpsc;

use crate::llm::sse::{SseEvent, SseParser};

/// How long the stream may take to name its message address.
const ENDPOINT_WAIT: Duration = Duration::from_secs(20);
/// How many messages the stream may carry before naming its message
/// address (a server has nothing to say before then; a few are kept for
/// rmcp rather than lost). More is a server ReMa will not use: the
/// messages cannot be buffered without bound, nor dropped silently.
pub const MAX_EARLY_MESSAGES: usize = 64;

#[derive(Debug)]
pub struct LegacySseError(String);

impl fmt::Display for LegacySseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LegacySseError {}

/// An open HTTP+SSE session.
pub struct LegacySse {
    http: reqwest::Client,
    endpoint: Url,
    headers: HeaderMap,
    incoming: mpsc::Receiver<ServerJsonRpcMessage>,
    reader: tokio::task::JoinHandle<()>,
}

/// What a GET of the server's URL showed.
pub enum Probe {
    /// An HTTP+SSE server; the session is open.
    Legacy(LegacySse),
    /// Not an event stream naming a message address.
    NotLegacy,
    /// No answer at all (the request could not be sent).
    Unreachable(String),
    /// An HTTP+SSE server ReMa will not use, and why.
    Refused(String),
}

/// A JSON-RPC message from a `message` event (or an unnamed one).
fn message_of(event: &SseEvent) -> Option<ServerJsonRpcMessage> {
    match event.event.as_deref() {
        None | Some("message") => serde_json::from_str(&event.data).ok(),
        _ => None,
    }
}

/// Where the server asks for messages: resolved against the stream's URL
/// and on the same origin, or why not.
pub fn message_address(stream: &Url, named: &str) -> Result<Url, String> {
    let address = stream
        .join(named.trim())
        .map_err(|_| "The server named a message address that is not a URL.".to_string())?;
    if address.origin() != stream.origin() {
        return Err(format!(
            "The server asked ReMa to send its messages to another site ({}); ReMa sends them \
             only to the server's own address.",
            address.host_str().unwrap_or("unknown")
        ));
    }
    Ok(address)
}

/// What a server that answers the stream request with 401 or 403 gets:
/// there is no OAuth on this transport, only a configured token.
pub fn unauthorized_message(status: StatusCode, token_sent: bool) -> String {
    if token_sent {
        format!("The server rejected the token (HTTP {}).", status.as_u16())
    } else {
        format!(
            "The server requires authentication (HTTP {}). {}",
            status.as_u16(),
            super::config::LEGACY_NO_OAUTH
        )
    }
}

/// The stream's first events: its message address, or why there is none.
enum Named {
    Address(String),
    /// The stream ended (or failed) before naming one.
    Ended,
    /// More than `MAX_EARLY_MESSAGES` messages came first.
    Flooded,
}

/// Opens the event stream at `url` and waits for its message address.
pub async fn open(http: &reqwest::Client, url: &str, headers: &HeaderMap) -> Probe {
    let Ok(stream_url) = Url::parse(url) else {
        return Probe::NotLegacy;
    };
    let response = match http
        .get(stream_url.clone())
        .headers(headers.clone())
        .header(ACCEPT, "text/event-stream")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return Probe::Unreachable(error.to_string()),
    };
    let status = response.status();
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Probe::Refused(unauthorized_message(status, !headers.is_empty()));
    }
    let is_stream = status.is_success()
        && response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
    if !is_stream {
        return Probe::NotLegacy;
    }
    let mut body = response.bytes_stream();
    let mut parser = SseParser::default();
    // Messages before the address (and in the same chunk as it) are kept
    // in order and handed over once the session is open.
    let mut early: Vec<ServerJsonRpcMessage> = Vec::new();
    let named = tokio::time::timeout(ENDPOINT_WAIT, async {
        let mut named = None;
        while named.is_none() {
            let Some(Ok(bytes)) = body.next().await else {
                return Named::Ended;
            };
            for event in parser.push(&bytes) {
                if named.is_none() && event.event.as_deref() == Some("endpoint") {
                    named = Some(event.data.clone());
                } else if let Some(message) = message_of(&event) {
                    if early.len() >= MAX_EARLY_MESSAGES {
                        return Named::Flooded;
                    }
                    early.push(message);
                }
            }
        }
        Named::Address(named.unwrap_or_default())
    })
    .await;
    let named = match named {
        Ok(Named::Address(named)) => named,
        Ok(Named::Flooded) => {
            return Probe::Refused(format!(
                "The server sent more than {MAX_EARLY_MESSAGES} messages before its endpoint \
                 event; ReMa cannot use it."
            ))
        }
        Ok(Named::Ended) | Err(_) => return Probe::NotLegacy,
    };
    let endpoint = match message_address(&stream_url, &named) {
        Ok(endpoint) => endpoint,
        Err(why) => return Probe::Refused(why),
    };
    let (tx, incoming) = mpsc::channel(64);
    let reader = tokio::spawn(async move {
        for message in early {
            if tx.send(message).await.is_err() {
                return;
            }
        }
        // The stream ending or failing ends the session: dropping `tx`
        // makes `receive` yield `None`.
        while let Some(Ok(bytes)) = body.next().await {
            for event in parser.push(&bytes) {
                if let Some(message) = message_of(&event) {
                    if tx.send(message).await.is_err() {
                        return;
                    }
                }
            }
        }
    });
    Probe::Legacy(LegacySse {
        http: http.clone(),
        endpoint,
        headers: headers.clone(),
        incoming,
        reader,
    })
}

impl Transport<RoleClient> for LegacySse {
    type Error = LegacySseError;

    fn name() -> Cow<'static, str> {
        "HTTP+SSE (2024-11-05)".into()
    }

    fn send(
        &mut self,
        item: ClientJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let request = self
            .http
            .post(self.endpoint.clone())
            .headers(self.headers.clone())
            .json(&item);
        async move {
            let response = request
                .send()
                .await
                .map_err(|e| LegacySseError(format!("The server could not be reached: {e}")))?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(LegacySseError(format!(
                    "The server refused a message ({})",
                    response.status()
                )))
            }
        }
    }

    fn receive(&mut self) -> impl Future<Output = Option<ServerJsonRpcMessage>> + Send {
        self.incoming.recv()
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.reader.abort();
        std::future::ready(Ok(()))
    }
}

impl Drop for LegacySse {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::Notify,
    };

    use super::*;
    use crate::{
        mcp::{
            client::{connect, ConnectError, ServerConfig},
            config::LEGACY_NO_OAUTH,
            McpContext,
        },
        models::mcp::{McpAuth, McpTransport},
    };

    /// The reply of a 2024-11-05 server to one request.
    fn reply(request: &serde_json::Value) -> Option<serde_json::Value> {
        let id = request.get("id")?.clone();
        let result = match request["method"].as_str()? {
            "initialize" => serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "legacy-fixture", "version": "1.0.0" }
            }),
            "tools/list" => serde_json::json!({ "tools": [{
                "name": "echo",
                "description": "Echoes its text.",
                "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } } }
            }]}),
            "tools/call" => serde_json::json!({
                "content": [{ "type": "text", "text": format!("echo: {}", request["params"]["arguments"]["text"].as_str().unwrap_or("")) }]
            }),
            "ping" => serde_json::json!({}),
            // What a 2024-11-05 server says to newer methods (rmcp tries
            // `server/discover` first).
            _ => {
                return Some(serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": "Method not found" }
                }))
            }
        };
        Some(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    /// How the mock server answers.
    #[derive(Clone)]
    struct Behaviour {
        /// What the stream names as the message address.
        endpoint: &'static str,
        /// The status of a POST to /sse (Streamable HTTP's first try).
        post_status: u16,
        /// A `WWW-Authenticate` challenge with a 401/403 to that POST.
        post_challenge: bool,
        /// The status GET /sse gets instead of the stream (0: the stream).
        get_status: u16,
        /// `message` events sent before `endpoint`.
        early_messages: usize,
    }

    impl Default for Behaviour {
        fn default() -> Self {
            Self {
                endpoint: "/messages?sessionId=s1",
                post_status: 405,
                post_challenge: false,
                get_status: 0,
                early_messages: 0,
            }
        }
    }

    /// Handles on a running mock server.
    struct Fixture {
        base: String,
        /// Each request line with its Authorization header.
        seen: Arc<Mutex<Vec<String>>>,
        /// Ends the open event stream (the server drops the connection).
        drop_stream: Arc<Notify>,
        /// How many of the next GETs answer 503 (a server restarting).
        refuse_gets: Arc<AtomicUsize>,
    }

    impl Fixture {
        fn seen(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }

        fn count(&self, prefix: &str) -> usize {
            self.seen().iter().filter(|l| l.starts_with(prefix)).count()
        }
    }

    /// A server that speaks only HTTP+SSE: GET /sse opens the stream and
    /// names `endpoint`; POSTs there are answered on the stream. A POST to
    /// /sse (Streamable HTTP's first try) is refused (405 unless told
    /// otherwise). It records each request line with its Authorization
    /// header.
    async fn legacy_server(behaviour: Behaviour) -> Fixture {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fixture = Fixture {
            base,
            seen: Arc::new(Mutex::new(Vec::new())),
            drop_stream: Arc::new(Notify::new()),
            refuse_gets: Arc::new(AtomicUsize::new(0)),
        };
        let (to_stream, from_posts) = mpsc::unbounded_channel::<String>();
        let from_posts = Arc::new(tokio::sync::Mutex::new(from_posts));
        let log = fixture.seen.clone();
        let drop_stream = fixture.drop_stream.clone();
        let refuse_gets = fixture.refuse_gets.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let to_stream = to_stream.clone();
                let from_posts = from_posts.clone();
                let log = log.clone();
                let drop_stream = drop_stream.clone();
                let refuse_gets = refuse_gets.clone();
                let b = behaviour.clone();
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 8192];
                    let (head, body) = loop {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&raw).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let length = text[..end]
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if raw.len() >= end + 4 + length {
                                break (
                                    text[..end].to_string(),
                                    text[end + 4..end + 4 + length].to_string(),
                                );
                            }
                        }
                    };
                    let line = head.lines().next().unwrap_or("").to_string();
                    let auth = head
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
                        .unwrap_or("")
                        .to_string();
                    // The method and echoed text of a message, to see what
                    // was sent after a reconnection.
                    let sent = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .map(|r| {
                            format!(
                                " | {} {}",
                                r["method"].as_str().unwrap_or(""),
                                r["params"]["arguments"]["text"].as_str().unwrap_or("")
                            )
                        })
                        .unwrap_or_default();
                    log.lock().unwrap().push(format!("{line} | {auth}{sent}"));
                    let status_line = |status: u16, challenge: bool| {
                        let reason = match status {
                            400 => "Bad Request",
                            401 => "Unauthorized",
                            403 => "Forbidden",
                            404 => "Not Found",
                            405 => "Method Not Allowed",
                            503 => "Service Unavailable",
                            _ => "Internal Server Error",
                        };
                        let challenge = if challenge {
                            "WWW-Authenticate: Bearer realm=\"mcp\"\r\n"
                        } else {
                            ""
                        };
                        format!(
                            "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain\r\n{challenge}Content-Length: 6\r\n\r\nnope\r\n"
                        )
                    };
                    if line.starts_with("GET /sse") {
                        let refused = refuse_gets
                            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                            .is_ok();
                        if refused {
                            let _ = socket.write_all(status_line(503, false).as_bytes()).await;
                            return;
                        }
                        if b.get_status != 0 {
                            let _ = socket
                                .write_all(
                                    status_line(b.get_status, b.get_status == 401).as_bytes(),
                                )
                                .await;
                            return;
                        }
                        let mut open = String::from(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n",
                        );
                        for i in 0..b.early_messages {
                            // What a server may say before the handshake
                            // (rmcp accepts logging then).
                            open.push_str(&format!(
                                "event: message\ndata: {{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{{\"level\":\"info\",\"data\":\"early {i}\"}}}}\n\n"
                            ));
                        }
                        open.push_str(&format!("event: endpoint\ndata: {}\n\n", b.endpoint));
                        socket.write_all(open.as_bytes()).await.unwrap();
                        let mut posts = from_posts.lock().await;
                        loop {
                            tokio::select! {
                                message = posts.recv() => {
                                    let Some(message) = message else { return };
                                    let event = format!("event: message\ndata: {message}\n\n");
                                    if socket.write_all(event.as_bytes()).await.is_err() {
                                        return;
                                    }
                                }
                                _ = drop_stream.notified() => return,
                            }
                        }
                    } else if line.starts_with("POST /messages") {
                        let request: serde_json::Value = serde_json::from_str(&body).unwrap();
                        if let Some(answer) = reply(&request) {
                            let _ = to_stream.send(answer.to_string());
                        }
                        let _ = socket
                            .write_all(
                                b"HTTP/1.1 202 Accepted\r\nContent-Length: 8\r\n\r\nAccepted",
                            )
                            .await;
                    } else {
                        let _ = socket
                            .write_all(status_line(b.post_status, b.post_challenge).as_bytes())
                            .await;
                    }
                });
            }
        });
        fixture
    }

    fn config(url: String) -> ServerConfig {
        ServerConfig {
            id: 1,
            name: "legacy".into(),
            transport: McpTransport::Http,
            command: String::new(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: String::new(),
            url,
            auth: McpAuth::Bearer,
            header_name: String::new(),
            secret: Some("legacy-token".into()),
        }
    }

    fn arguments(text: &str) -> serde_json::Map<String, serde_json::Value> {
        let mut arguments = serde_json::Map::new();
        arguments.insert("text".into(), text.into());
        arguments
    }

    #[tokio::test]
    async fn a_server_on_the_older_http_sse_transport_connects_lists_and_calls() {
        let server = legacy_server(Behaviour::default()).await;
        let connection = connect(&config(format!("{}/sse", server.base)), "0.1.0", None)
            .await
            .unwrap();
        assert_eq!(connection.protocol_version.as_deref(), Some("2024-11-05"));
        assert_eq!(
            connection.server_info.as_deref(),
            Some("legacy-fixture 1.0.0")
        );
        assert_eq!(connection.tools.len(), 1);
        let (text, is_error) = connection
            .call(
                "echo",
                arguments("hello"),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!is_error);
        assert!(text.contains("echo: hello"), "{text}");
        let seen = server.seen();
        // Streamable HTTP first (refused), then the stream, then messages;
        // the token goes with every request.
        assert!(seen[0].starts_with("POST /sse"), "{seen:?}");
        assert!(seen.iter().any(|l| l.starts_with("GET /sse")));
        assert!(seen
            .iter()
            .any(|l| l.starts_with("POST /messages?sessionId=s1")));
        assert!(seen
            .iter()
            .filter(|l| l.starts_with("POST /messages") || l.starts_with("GET /sse"))
            .all(|l| l
                .to_ascii_lowercase()
                .contains("| authorization: bearer legacy-token")));
    }

    /// Against a real HTTP+SSE server, e.g. the official Python SDK's
    /// (`MCPServer(...).run(transport="sse")`):
    /// `REMA_LIVE_SSE_URL=http://127.0.0.1:8765/sse cargo test live_legacy_sse -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_legacy_sse_server() {
        let Some(url) = std::env::var("REMA_LIVE_SSE_URL").ok() else {
            println!("BLOCKED live HTTP+SSE server: set REMA_LIVE_SSE_URL to run it.");
            return;
        };
        let mut server = config(url.clone());
        server.auth = McpAuth::None;
        server.secret = None;
        let connection = connect(&server, "0.1.0", None).await.unwrap();
        let tools: Vec<String> = connection
            .tools
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        println!(
            "connected to {url}: protocol {:?}, server {:?}, tools {tools:?}",
            connection.protocol_version, connection.server_info
        );
        let (text, is_error) = connection
            .call(
                "echo",
                arguments("from ReMa"),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        println!("echo -> {text} (error: {is_error})");
        assert!(!is_error && text.contains("echo: from ReMa"));
        println!("VERIFIED live HTTP+SSE server at {url}");
    }

    #[tokio::test]
    async fn a_stream_naming_another_site_is_refused() {
        let server = legacy_server(Behaviour {
            endpoint: "https://evil.example/collect",
            ..Behaviour::default()
        })
        .await;
        let error = match connect(&config(format!("{}/sse", server.base)), "0.1.0", None).await {
            Err(ConnectError::Failed(why)) => why,
            other => panic!("{:?}", other.map(|c| c.tools.len())),
        };
        assert!(error.contains("another site (evil.example)"), "{error}");
        assert_eq!(server.count("POST /collect"), 0);
    }

    #[test]
    fn the_message_address_stays_on_the_servers_origin() {
        let stream = Url::parse("https://mcp.example.com/sse").unwrap();
        assert_eq!(
            message_address(&stream, "/messages?sessionId=abc")
                .unwrap()
                .as_str(),
            "https://mcp.example.com/messages?sessionId=abc"
        );
        assert_eq!(
            message_address(&stream, "https://mcp.example.com/m")
                .unwrap()
                .as_str(),
            "https://mcp.example.com/m"
        );
        for elsewhere in [
            "https://evil.example/collect",
            "http://mcp.example.com/messages",
            "https://mcp.example.com:8443/messages",
            "//evil.example/x",
        ] {
            assert!(message_address(&stream, elsewhere).is_err(), "{elsewhere}");
        }
    }

    /// The specification's fallback is for a server that does not take the
    /// POST at all (400, 404, 405). A server that is down (5xx), or asks
    /// for authentication (401, 403), is reported as such: no stream is
    /// requested, and the token is not sent again.
    #[tokio::test]
    async fn falls_back_to_the_stream_only_on_a_plain_400_404_or_405() {
        for status in [400u16, 404, 405] {
            let server = legacy_server(Behaviour {
                post_status: status,
                ..Behaviour::default()
            })
            .await;
            let connection = connect(&config(format!("{}/sse", server.base)), "0.1.0", None)
                .await
                .unwrap_or_else(|e| panic!("{status}: {e}"));
            assert_eq!(connection.tools.len(), 1, "{status}");
            assert_eq!(server.count("GET /sse"), 1, "{status}");
        }
        for (status, challenge) in [
            (500u16, false),
            (503, false),
            (401, false),
            (401, true),
            (403, false),
            (403, true),
        ] {
            let server = legacy_server(Behaviour {
                post_status: status,
                post_challenge: challenge,
                ..Behaviour::default()
            })
            .await;
            let error = connect(&config(format!("{}/sse", server.base)), "0.1.0", None)
                .await
                .err()
                .unwrap_or_else(|| panic!("{status} connected"))
                .to_string();
            assert_eq!(
                server.count("GET /sse"),
                0,
                "{status} (challenge {challenge}): {error}"
            );
            if status == 401 || challenge {
                // A configured token the server did not accept.
                assert!(error.contains("rejected the token"), "{status}: {error}");
            } else {
                assert!(error.contains(&status.to_string()), "{status}: {error}");
            }
        }
    }

    #[tokio::test]
    async fn a_server_that_cannot_be_reached_is_not_tried_as_a_stream() {
        // A port nothing listens on.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/sse", listener.local_addr().unwrap());
        drop(listener);
        let error = connect(&config(url.clone()), "0.1.0", None)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(error.starts_with("Could not connect:"), "{error}");
        // The same for a server marked as HTTP+SSE.
        let mut marked = config(url);
        marked.transport = McpTransport::Sse;
        let error = connect(&marked, "0.1.0", None)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(error.starts_with("Could not connect:"), "{error}");
    }

    #[tokio::test]
    async fn a_server_marked_as_http_sse_is_opened_without_probing_streamable_http() {
        let server = legacy_server(Behaviour::default()).await;
        let mut config = config(format!("{}/sse", server.base));
        config.transport = McpTransport::Sse;
        let connection = connect(&config, "0.1.0", None).await.unwrap();
        assert_eq!(connection.protocol_version.as_deref(), Some("2024-11-05"));
        let seen = server.seen();
        assert!(seen[0].starts_with("GET /sse"), "{seen:?}");
        assert_eq!(server.count("POST /sse"), 0);

        // Such a server that is not one gets told so, with the address.
        let plain = legacy_server(Behaviour {
            get_status: 404,
            ..Behaviour::default()
        })
        .await;
        config.url = format!("{}/sse", plain.base);
        let error = connect(&config, "0.1.0", None)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("No HTTP+SSE event stream") && error.contains(&plain.base),
            "{error}"
        );

        // OAuth is not offered on it.
        config.auth = McpAuth::Oauth;
        config.secret = None;
        let error = connect(&config, "0.1.0", None)
            .await
            .err()
            .unwrap()
            .to_string();
        assert_eq!(error, LEGACY_NO_OAUTH);
    }

    #[tokio::test]
    async fn a_few_messages_before_the_endpoint_are_kept_and_a_flood_is_refused() {
        let server = legacy_server(Behaviour {
            early_messages: 3,
            ..Behaviour::default()
        })
        .await;
        let connection = connect(&config(format!("{}/sse", server.base)), "0.1.0", None)
            .await
            .unwrap();
        let (text, _) = connection
            .call(
                "echo",
                arguments("still works"),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(text.contains("echo: still works"), "{text}");

        let flood = legacy_server(Behaviour {
            early_messages: MAX_EARLY_MESSAGES + 1,
            ..Behaviour::default()
        })
        .await;
        let error = match connect(&config(format!("{}/sse", flood.base)), "0.1.0", None).await {
            Err(ConnectError::Failed(why)) => why,
            other => panic!("{:?}", other.map(|c| c.tools.len())),
        };
        assert_eq!(
            error,
            format!(
                "The server sent more than {MAX_EARLY_MESSAGES} messages before its endpoint \
                 event; ReMa cannot use it."
            )
        );
        assert_eq!(flood.count("POST /messages"), 0);
    }

    #[tokio::test]
    async fn a_legacy_server_asking_for_authentication_names_the_token_option() {
        let server = legacy_server(Behaviour {
            get_status: 401,
            ..Behaviour::default()
        })
        .await;
        let mut anonymous = config(format!("{}/sse", server.base));
        anonymous.auth = McpAuth::None;
        anonymous.secret = None;
        let error = connect(&anonymous, "0.1.0", None)
            .await
            .err()
            .unwrap()
            .to_string();
        assert_eq!(
            error,
            format!("The server requires authentication (HTTP 401). {LEGACY_NO_OAUTH}")
        );
        assert!(
            error.contains("does not support OAuth sign-in on the older HTTP+SSE transport")
                && error.contains("bearer token or an API-key header"),
            "{error}"
        );
        // Nothing was posted anywhere.
        assert_eq!(server.count("POST /messages"), 0);

        // With a token the server did not accept, that is what is said.
        let error = connect(&config(format!("{}/sse", server.base)), "0.1.0", None)
            .await
            .err()
            .unwrap()
            .to_string();
        assert_eq!(error, "The server rejected the token (HTTP 401).");
    }

    async fn wait_until(what: &str, condition: impl Fn() -> bool) {
        for _ in 0..100 {
            if condition() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("{what} did not happen in time");
    }

    #[tokio::test]
    async fn a_lost_stream_fails_calls_and_the_next_use_reconnects_with_backoff() {
        let server = legacy_server(Behaviour::default()).await;
        let config = config(format!("{}/sse", server.base));
        let context = McpContext::with_backoff([Duration::from_millis(100); 3]);
        let cancel = tokio_util::sync::CancellationToken::new();
        let connection = context
            .connect(&config, "0.1.0", None, &|| {})
            .await
            .unwrap();
        let (text, _) = connection
            .call("echo", arguments("one"), &cancel)
            .await
            .unwrap();
        assert!(text.contains("echo: one"));

        // The server drops the stream: the connection is closed, a call
        // says so, and Settings shows it.
        server.drop_stream.notify_one();
        wait_until("the connection closing", || connection.is_closed()).await;
        let error = connection
            .call("echo", arguments("two"), &cancel)
            .await
            .unwrap_err();
        assert!(
            error.contains("connection to the server was lost"),
            "{error}"
        );
        assert!(context.live(config.id).is_none());
        let status = context.status(config.id, true);
        assert_eq!(status.state, crate::models::mcp::McpState::Error);
        assert!(status.message.unwrap().contains("stopped"));

        // The next use opens a new session; the first try finds the server
        // restarting (503) and the second gets through. The failed call is
        // not repeated.
        server.refuse_gets.store(1, Ordering::SeqCst);
        let posts_before = server.count("POST /messages");
        let again = context
            .connect(&config, "0.1.0", None, &|| {})
            .await
            .unwrap();
        assert!(!Arc::ptr_eq(&again, &connection));
        let (text, _) = again
            .call("echo", arguments("three"), &cancel)
            .await
            .unwrap();
        assert!(text.contains("echo: three"));
        assert_eq!(server.count("GET /sse"), 3, "{:?}", server.seen());
        let posts: Vec<String> = server.seen()[..]
            .iter()
            .filter(|l| l.starts_with("POST /messages"))
            .skip(posts_before)
            .cloned()
            .collect();
        assert!(
            posts.iter().any(|l| l.contains("tools/call three"))
                && !posts.iter().any(|l| l.contains("two")),
            "{posts:?}"
        );

        // A server that stays away: three attempts, then the error (the
        // Streamable HTTP answer, as no event stream came either).
        server.drop_stream.notify_one();
        wait_until("the connection closing", || again.is_closed()).await;
        server.refuse_gets.store(10, Ordering::SeqCst);
        let gets_before = server.count("GET /sse");
        let error = context
            .connect(&config, "0.1.0", None, &|| {})
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("HTTP 405"), "{error}");
        assert_eq!(server.count("GET /sse") - gets_before, 3);
        assert_eq!(context.status(config.id, true).message.unwrap(), error);
    }

    #[tokio::test]
    async fn disconnecting_stops_a_reconnection_in_progress() {
        let server = legacy_server(Behaviour::default()).await;
        let config = config(format!("{}/sse", server.base));
        let context = McpContext::with_backoff([
            Duration::from_millis(100),
            Duration::from_secs(30),
            Duration::from_secs(30),
        ]);
        let connection = context
            .connect(&config, "0.1.0", None, &|| {})
            .await
            .unwrap();
        server.drop_stream.notify_one();
        wait_until("the connection closing", || connection.is_closed()).await;
        server.refuse_gets.store(10, Ordering::SeqCst);
        let waiting = {
            let context = context.clone();
            let config = config.clone();
            tokio::spawn(async move { context.connect(&config, "0.1.0", None, &|| {}).await })
        };
        wait_until("the first attempt", || server.count("GET /sse") == 2).await;
        context.disconnect(config.id);
        let error = tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .expect("stopped without waiting out the backoff")
            .unwrap()
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("stopped"), "{error}");
        assert_eq!(server.count("GET /sse"), 2);
    }
}
