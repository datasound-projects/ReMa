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
//! within 20 seconds, and the usual connect and call timeouts apply.

use std::{borrow::Cow, fmt, future::Future, time::Duration};

use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, ACCEPT, CONTENT_TYPE},
    Url,
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
        Err(_) => return Probe::NotLegacy,
    };
    let is_stream = response.status().is_success()
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
    let (tx, incoming) = mpsc::channel(64);
    let named = tokio::time::timeout(ENDPOINT_WAIT, async {
        let mut named = None;
        while named.is_none() {
            let Some(Ok(bytes)) = body.next().await else {
                return None;
            };
            for event in parser.push(&bytes) {
                if named.is_none() && event.event.as_deref() == Some("endpoint") {
                    named = Some(event.data.clone());
                } else if let Some(message) = message_of(&event) {
                    let _ = tx.try_send(message);
                }
            }
        }
        named
    })
    .await;
    let Ok(Some(named)) = named else {
        return Probe::NotLegacy;
    };
    let endpoint = match message_address(&stream_url, &named) {
        Ok(endpoint) => endpoint,
        Err(why) => return Probe::Refused(why),
    };
    let reader = tokio::spawn(async move {
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
    use std::sync::{Arc, Mutex};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;
    use crate::{
        mcp::client::{connect, ServerConfig},
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

    /// A server that speaks only HTTP+SSE: GET /sse opens the stream and
    /// names `endpoint`; POSTs there are answered on the stream. A POST to
    /// /sse (Streamable HTTP's first try) is refused with 405. It records
    /// each request line with its Authorization header.
    async fn legacy_server(endpoint: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (to_stream, from_posts) = mpsc::unbounded_channel::<String>();
        let from_posts = Arc::new(tokio::sync::Mutex::new(from_posts));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let to_stream = to_stream.clone();
                let from_posts = from_posts.clone();
                let log = log.clone();
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
                    log.lock().unwrap().push(format!("{line} | {auth}"));
                    if line.starts_with("GET /sse") {
                        let open = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\nevent: endpoint\ndata: {endpoint}\n\n"
                        );
                        socket.write_all(open.as_bytes()).await.unwrap();
                        let mut posts = from_posts.lock().await;
                        while let Some(message) = posts.recv().await {
                            let event = format!("event: message\ndata: {message}\n\n");
                            if socket.write_all(event.as_bytes()).await.is_err() {
                                return;
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
                            .write_all(
                                b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n",
                            )
                            .await;
                    }
                });
            }
        });
        (base, seen)
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

    #[tokio::test]
    async fn a_server_on_the_older_http_sse_transport_connects_lists_and_calls() {
        let (base, seen) = legacy_server("/messages?sessionId=s1").await;
        let connection = connect(&config(format!("{base}/sse")), "0.1.0", None)
            .await
            .unwrap();
        assert_eq!(connection.protocol_version.as_deref(), Some("2024-11-05"));
        assert_eq!(
            connection.server_info.as_deref(),
            Some("legacy-fixture 1.0.0")
        );
        assert_eq!(connection.tools.len(), 1);
        let mut arguments = serde_json::Map::new();
        arguments.insert("text".into(), "hello".into());
        let (text, is_error) = connection
            .call(
                "echo",
                arguments,
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!is_error);
        assert!(text.contains("echo: hello"), "{text}");
        let seen = seen.lock().unwrap().clone();
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
                .ends_with("authorization: bearer legacy-token")));
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
        let mut arguments = serde_json::Map::new();
        arguments.insert("text".into(), "from ReMa".into());
        let (text, is_error) = connection
            .call(
                "echo",
                arguments,
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
        let (base, seen) = legacy_server("https://evil.example/collect").await;
        let error = match connect(&config(format!("{base}/sse")), "0.1.0", None).await {
            Err(crate::mcp::client::ConnectError::Failed(why)) => why,
            other => panic!("{:?}", other.map(|c| c.tools.len())),
        };
        assert!(error.contains("another site (evil.example)"), "{error}");
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .all(|l| !l.starts_with("POST /collect")));
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
}
