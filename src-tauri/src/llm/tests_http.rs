//! End-to-end adapter tests against a local mock HTTP server.

use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};
use tokio_util::sync::CancellationToken;

use serde_json::json;

use super::*;
use crate::error::AppError;

/// Serves one request: responds with `status` and streams `chunks`, then
/// (if `hold_open`) keeps the connection open. Returns the raw request.
async fn serve_once(
    status: u16,
    chunks: Vec<&'static str>,
    hold_open: bool,
) -> (String, oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        // Read headers, then the body by Content-Length.
        loop {
            let n = socket.read(&mut buf).await.unwrap();
            request.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&request).to_string();
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        let _ = tx.send(String::from_utf8_lossy(&request).to_string());
        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        for chunk in chunks {
            socket.write_all(chunk.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if hold_open {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    (base_url, rx)
}

fn endpoint(kind: ProviderKind, base_url: String, key: &str) -> Endpoint {
    Endpoint {
        kind,
        name: kind.display_name().into(),
        connection: ConnectionMethod::ApiKey,
        base_url,
        credential: Some(Credential::ApiKey { key: key.into() }),
    }
}

fn request() -> ChatRequest {
    ChatRequest {
        system: None,
        turns: vec![Turn {
            role: MessageRole::User,
            content: "Hi".into(),
        }],
        max_output_tokens: None,
        ..ChatRequest::default()
    }
}

async fn collect(endpoint: &Endpoint, cancel: CancellationToken) -> (AppResult<Finish>, String) {
    let llm = ProviderLanguageModel::new(None);
    let mut text = String::new();
    let mut sink = |delta: &str| text.push_str(delta);
    let result = llm
        .stream_chat(endpoint, "test-model", &request(), cancel, &mut sink)
        .await;
    (result, text)
}

#[tokio::test]
async fn streams_openai_compatible_responses() {
    let (base_url, received) = serve_once(
        200,
        vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        ],
        false,
    )
    .await;
    let endpoint = endpoint(ProviderKind::OpenaiCompatible, base_url, "local-key");
    let (result, text) = collect(&endpoint, CancellationToken::new()).await;

    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Hello world");
    let request = received.await.unwrap();
    assert!(request.starts_with("POST /chat/completions"));
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer local-key"));
    assert!(request.contains("\"stream\":true"));
}

#[tokio::test]
async fn streams_anthropic_responses_with_its_headers() {
    let (base_url, received) = serve_once(
        200,
        vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi!\"}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ],
        false,
    )
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let (result, text) = collect(&endpoint, CancellationToken::new()).await;

    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Hi!");
    let request = received.await.unwrap().to_ascii_lowercase();
    assert!(request.starts_with("post /messages"));
    assert!(request.contains("x-api-key: sk-ant-test"));
    assert!(request.contains("anthropic-version: 2023-06-01"));
}

#[tokio::test]
async fn sends_claude_console_tokens_as_oauth_bearer() {
    let (base_url, received) = serve_once(
        200,
        vec![
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Ok\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ],
        false,
    )
    .await;
    let endpoint = Endpoint {
        connection: ConnectionMethod::ClaudeConsole,
        credential: Some(Credential::OAuth {
            access_token: "console-access-token".into(),
            refresh_token: None,
            expires_at: None,
        }),
        ..endpoint(ProviderKind::Anthropic, base_url, "unused")
    };
    let (result, text) = collect(&endpoint, CancellationToken::new()).await;

    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Ok");
    let request = received.await.unwrap().to_ascii_lowercase();
    assert!(request.contains("authorization: bearer console-access-token"));
    assert!(request.contains("anthropic-beta: oauth-2025-04-20"));
    assert!(!request.contains("x-api-key"));
}

#[tokio::test]
async fn a_chatgpt_connection_needs_the_codex_runtime() {
    let endpoint = Endpoint {
        connection: ConnectionMethod::ChatgptAccount,
        credential: None,
        ..endpoint(ProviderKind::Openai, "http://unused".into(), "unused")
    };
    let (result, _) = collect(&endpoint, CancellationToken::new()).await;
    assert!(matches!(result.unwrap_err(), AppError::Configuration(_)));
}

#[tokio::test]
async fn maps_rejected_keys_to_authentication_errors_without_the_key() {
    let (base_url, _) = serve_once(
        401,
        vec!["{\"error\":{\"message\":\"Incorrect API key provided: sk-secret-value\"}}"],
        false,
    )
    .await;
    let endpoint = endpoint(ProviderKind::Openai, base_url, "sk-secret-value");
    let (result, _) = collect(&endpoint, CancellationToken::new()).await;

    let error = result.unwrap_err();
    assert!(matches!(error, AppError::Authentication(_)));
    assert!(!error.to_string().contains("sk-secret-value"));
}

#[tokio::test]
async fn stops_a_stalled_stream_when_cancelled() {
    let (base_url, _) = serve_once(
        200,
        vec!["data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n"],
        true,
    )
    .await;
    let endpoint = endpoint(ProviderKind::OpenaiCompatible, base_url, "k");
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        trigger.cancel();
    });

    let started = std::time::Instant::now();
    let (result, text) = collect(&endpoint, cancel).await;
    assert_eq!(result.unwrap(), Finish::Cancelled);
    assert_eq!(text, "partial");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn reports_unreachable_servers_as_network_errors() {
    // Bind then drop a listener to get a closed local port.
    let port = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let endpoint = endpoint(
        ProviderKind::OpenaiCompatible,
        format!("http://127.0.0.1:{port}"),
        "k",
    );
    let (result, _) = collect(&endpoint, CancellationToken::new()).await;
    assert!(matches!(result.unwrap_err(), AppError::Network(_)));
}

/// Serves several requests in order, one response each. Returns the base
/// URL and the requests received (bodies included).
async fn serve_sequence(
    responses: Vec<(u16, Vec<&'static str>)>,
) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        for (status, chunks) in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&request).to_string());
            let kind = if status == 200 {
                "text/event-stream"
            } else {
                "application/json"
            };
            let head =
                format!("HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nConnection: close\r\n\r\n");
            socket.write_all(head.as_bytes()).await.unwrap();
            for chunk in chunks {
                socket.write_all(chunk.as_bytes()).await.unwrap();
                socket.flush().await.unwrap();
            }
            let _ = socket.shutdown().await;
        }
    });
    (base_url, rx)
}

fn body_of(request: &str) -> serde_json::Value {
    let body = &request[request.find("\r\n\r\n").unwrap() + 4..];
    serde_json::from_str(body).unwrap()
}

#[derive(Default)]
struct RecordingWeb(std::sync::Mutex<Vec<WebEvent>>);

impl WebObserver for RecordingWeb {
    fn observe(&self, event: WebEvent) {
        self.0.lock().unwrap().push(event);
    }
}

async fn collect_with_web(
    endpoint: &Endpoint,
    model: &str,
    web: Arc<RecordingWeb>,
) -> (AppResult<Finish>, String) {
    let llm = ProviderLanguageModel::new(None);
    let mut request = request();
    request.web = Some(WebSearch {
        observer: Some(web),
    });
    let mut text = String::new();
    let mut sink = |delta: &str| text.push_str(delta);
    let result = llm
        .stream_chat(
            endpoint,
            model,
            &request,
            CancellationToken::new(),
            &mut sink,
        )
        .await;
    (result, text)
}

#[tokio::test]
async fn anthropic_searches_the_web_and_resumes_a_paused_turn() {
    let (base_url, mut received) = serve_sequence(vec![
        (
            200,
            vec![
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Searching.\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"server_tool_use\",\"id\":\"srvtoolu_1\",\"name\":\"web_search\",\"input\":{}}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\": \\\"AI jobs\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\" Vienna\\\"}\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"web_search_tool_result\",\"tool_use_id\":\"srvtoolu_1\",\"content\":[{\"type\":\"web_search_result\",\"title\":\"Senior AI Engineer\",\"url\":\"https://jobs.example.com/1\",\"encrypted_content\":\"e1\"}]}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"pause_turn\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        ),
        (
            200,
            vec![
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Found one.\"}}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        ),
    ])
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let web = Arc::new(RecordingWeb::default());
    let (result, text) = collect_with_web(&endpoint, "claude-opus-5", web.clone()).await;

    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Searching.\n\nFound one.");
    let first = body_of(&received.recv().await.unwrap());
    let tools: Vec<&str> = first["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["type"].as_str().unwrap())
        .collect();
    assert_eq!(tools, ["web_search_20260209", "web_fetch_20260209"]);

    // The paused step goes back verbatim, with no extra user message.
    let second = body_of(&received.recv().await.unwrap());
    let messages = second["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    let content = messages[1]["content"].as_array().unwrap();
    assert_eq!(content[0], json!({ "type": "text", "text": "Searching." }));
    assert_eq!(content[1]["type"], "server_tool_use");
    assert_eq!(content[1]["input"], json!({ "query": "AI jobs Vienna" }));
    assert_eq!(content[2]["type"], "web_search_tool_result");
    assert_eq!(content[2]["content"][0]["encrypted_content"], "e1");

    let events = web.0.lock().unwrap().clone();
    assert_eq!(
        events[0],
        WebEvent::Started {
            id: "srvtoolu_1".into(),
            kind: WebKind::Search,
            target: "AI jobs Vienna".into()
        }
    );
    assert!(matches!(
        &events[1],
        WebEvent::Finished { sources, .. } if sources[0].url == "https://jobs.example.com/1"
    ));
}

#[tokio::test]
async fn answers_without_web_search_when_the_provider_refuses_it() {
    let (base_url, mut received) = serve_sequence(vec![
        (
            400,
            vec!["{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"tools.0: web search is not enabled for this organization\"}}"],
        ),
        (
            200,
            vec![
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"From memory.\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        ),
    ])
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let web = Arc::new(RecordingWeb::default());
    let (result, text) = collect_with_web(&endpoint, "claude-haiku-4-5", web.clone()).await;

    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "From memory.");
    let first = body_of(&received.recv().await.unwrap());
    assert_eq!(first["tools"][0]["type"], "web_search_20250305");
    let second = body_of(&received.recv().await.unwrap());
    assert!(second.get("tools").is_none());
    assert!(matches!(
        &web.0.lock().unwrap()[0],
        WebEvent::Unavailable { reason } if reason.contains("not enabled")
    ));
}

#[tokio::test]
async fn other_errors_are_not_retried_without_web_search() {
    let (base_url, _) = serve_sequence(vec![(
        400,
        vec!["{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"Your credit balance is too low to access the Anthropic API.\"}}"],
    )])
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let web = Arc::new(RecordingWeb::default());
    let (result, _) = collect_with_web(&endpoint, "claude-opus-5", web.clone()).await;
    assert!(result.unwrap_err().to_string().contains("credit"));
    assert!(web.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn openai_uses_the_responses_api_with_web_search() {
    let (base_url, mut received) = serve_sequence(vec![(
        200,
        vec![
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{}}\n\n",
            "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"id\":\"ws_1\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"id\":\"ws_1\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"AI jobs Vienna\"}}}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"delta\":\"Two roles.\"}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
        ],
    )])
    .await;
    let endpoint = endpoint(ProviderKind::Openai, base_url, "sk-test");
    let web = Arc::new(RecordingWeb::default());
    let (result, text) = collect_with_web(&endpoint, "gpt-6", web.clone()).await;

    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Two roles.");
    let request = received.recv().await.unwrap();
    assert!(request.starts_with("POST /responses"));
    let body = body_of(&request);
    assert_eq!(body["tools"][0]["type"], "web_search");
    assert_eq!(body["store"], false);
    assert_eq!(web.0.lock().unwrap().len(), 2);
}
