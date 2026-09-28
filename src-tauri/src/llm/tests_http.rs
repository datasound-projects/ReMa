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
        server_web_search: false,
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
        server_web_search: false,
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
        required: false,
        ..WebSearch::default()
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
    assert_eq!(tools, ["web_search_20260318", "web_fetch_20260318"]);

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
async fn a_search_the_organization_turned_off_continues_with_remas_tools() {
    let (base_url, mut received) = serve_sequence(vec![
        (
            400,
            vec!["{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"web search is not enabled for this organization\"}}"],
        ),
        (
            200,
            vec![
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"From ReMa's sources.\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        ),
    ])
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let web = Arc::new(RecordingWeb::default());
    let llm = ProviderLanguageModel::new(None);
    let mut request = request();
    request.web = Some(WebSearch {
        observer: Some(web.clone()),
        fallback: Some(ToolBox {
            specs: vec![ToolSpec {
                name: "rema_career_search".into(),
                description: "Search current career information.".into(),
                input_schema: json!({ "type": "object" }),
            }],
            executor: Arc::new(fake::NoTools),
        }),
        ..WebSearch::default()
    });
    let mut text = String::new();
    let mut sink = |delta: &str| text.push_str(delta);
    let result = llm
        .stream_chat(
            &endpoint,
            "claude-opus-5",
            &request,
            CancellationToken::new(),
            &mut sink,
        )
        .await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "From ReMa's sources.");
    let first = body_of(&received.recv().await.unwrap());
    assert_eq!(first["tools"][0]["type"], "web_search_20260318");
    // The answer goes on with ReMa's career tools instead, and says why.
    let second = body_of(&received.recv().await.unwrap());
    let tools = second["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "rema_career_search");
    assert!(matches!(
        &web.0.lock().unwrap()[0],
        WebEvent::Unavailable { reason } if reason.contains("not enabled")
    ));
}

#[tokio::test]
async fn a_model_told_to_search_directly_is_asked_again_directly() {
    let (base_url, mut received) = serve_sequence(vec![
        (
            400,
            vec!["{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"tools.0: this model does not support programmatic tool calling; set allowed_callers to [\\\"direct\\\"]\"}}"],
        ),
        (
            200,
            vec![
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Searched.\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        ),
    ])
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let web = Arc::new(RecordingWeb::default());
    let (result, text) =
        collect_with_web(&endpoint, "claude-sonnet-9-directtest", web.clone()).await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Searched.");
    let first = body_of(&received.recv().await.unwrap());
    assert!(first["tools"][0].get("allowed_callers").is_none());
    let second = body_of(&received.recv().await.unwrap());
    assert_eq!(second["tools"][0]["allowed_callers"], json!(["direct"]));
    assert_eq!(second["tools"][0]["type"], "web_search_20260318");
    // Search stayed on: nothing was reported unavailable.
    assert!(web.0.lock().unwrap().is_empty());
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

#[tokio::test]
async fn a_required_search_fails_rather_than_answering_without_it() {
    let (base_url, _) = serve_sequence(vec![(
        400,
        vec!["{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"tools.0: web search is not enabled for this organization\"}}"],
    )])
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let llm = ProviderLanguageModel::new(None);
    let mut request = request();
    request.web = Some(WebSearch {
        observer: None,
        required: true,
        ..WebSearch::default()
    });
    let mut sink = |_: &str| {};
    let error = llm
        .stream_chat(
            &endpoint,
            "claude-opus-5",
            &request,
            CancellationToken::new(),
            &mut sink,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("web search is not enabled"));
}

#[tokio::test]
async fn reports_search_errors_that_arrive_with_a_200() {
    let (base_url, _) = serve_sequence(vec![(
        200,
        vec![
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"server_tool_use\",\"id\":\"srvtoolu_1\",\"name\":\"web_search\",\"input\":{}}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\": \\\"AI jobs\\\"}\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"web_search_tool_result\",\"tool_use_id\":\"srvtoolu_1\",\"content\":{\"type\":\"web_search_tool_result_error\",\"error_code\":\"too_many_requests\"}}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"text_delta\",\"text\":\"{\\\"postings\\\":[]}\"}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ],
    )])
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let web = Arc::new(RecordingWeb::default());
    let (result, _) = collect_with_web(&endpoint, "claude-opus-5", web.clone()).await;
    assert_eq!(result.unwrap(), Finish::Complete);
    let events = web.0.lock().unwrap().clone();
    assert!(matches!(
        &events[1],
        WebEvent::Finished { error: Some(e), target, .. }
            if e.contains("rate limiting") && target == "AI jobs"
    ));
}

/// Records the calls it runs and answers each with its own id.
#[derive(Default)]
struct RecordingTools(std::sync::Mutex<Vec<ToolCall>>);

impl ToolExecutor for RecordingTools {
    fn execute<'a>(&'a self, call: &'a ToolCall) -> BoxFuture<'a, ToolOutput> {
        self.0.lock().unwrap().push(call.clone());
        Box::pin(async move {
            ToolOutput {
                content: format!("result of {}", call.id),
                is_error: false,
            }
        })
    }
}

fn tool_box(name: &str, executor: Arc<dyn ToolExecutor>) -> ToolBox {
    ToolBox {
        specs: vec![ToolSpec {
            name: name.into(),
            description: "A tool.".into(),
            input_schema: json!({ "type": "object" }),
        }],
        executor,
    }
}

async fn run(
    endpoint: &Endpoint,
    model: &str,
    request: &ChatRequest,
    llm: &ProviderLanguageModel,
) -> (AppResult<Finish>, String) {
    let mut text = String::new();
    let mut sink = |delta: &str| text.push_str(delta);
    let result = llm
        .stream_chat(
            endpoint,
            model,
            request,
            CancellationToken::new(),
            &mut sink,
        )
        .await;
    (result, text)
}

const GEMINI_SERVER_PARTS: &str = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"toolCall\":{\"id\":\"s1\",\"toolType\":\"GOOGLE_SEARCH\",\"args\":{\"queries\":[\"AI jobs Vienna\"]}},\"thoughtSignature\":\"sig-a\"},{\"toolResponse\":{\"id\":\"s1\",\"response\":{\"results\":[]}}},{\"functionCall\":{\"id\":\"fc1\",\"name\":\"mcp_rema_search_jobs\",\"args\":{\"q\":\"AI\"}},\"thoughtSignature\":\"sig-b\"}]},\"finishReason\":\"STOP\"}]}\n\n";

#[tokio::test]
async fn gemini_3_searches_next_to_functions_and_sends_its_parts_back() {
    let (base_url, mut received) = serve_sequence(vec![
        (200, vec![GEMINI_SERVER_PARTS]),
        (
            200,
            vec!["data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Done.\"}]},\"finishReason\":\"STOP\"}]}\n\n"],
        ),
    ])
    .await;
    let endpoint = endpoint(ProviderKind::Gemini, base_url, "gemini-key");
    let tools = Arc::new(RecordingTools::default());
    let mut request = request();
    request.tools = Some(tool_box("mcp_rema_search_jobs", tools.clone()));
    request.web = Some(WebSearch::default());
    let (result, text) = run(
        &endpoint,
        "gemini-3-pro",
        &request,
        &ProviderLanguageModel::new(None),
    )
    .await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Done.");

    // Built-in search next to functions needs server-side invocations on
    // (without it Gemini 3 answers 400).
    let first = body_of(&received.recv().await.unwrap());
    let kinds: Vec<String> = first["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_object().unwrap().keys().next().unwrap().clone())
        .collect();
    assert_eq!(kinds, ["functionDeclarations", "google_search"]);
    assert_eq!(
        first["toolConfig"]["includeServerSideToolInvocations"],
        json!(true)
    );

    // The model turn goes back exactly as Gemini sent it (search call and
    // result, signatures), and the response names the call's own id.
    assert_eq!(tools.0.lock().unwrap()[0].id, "fc1");
    let second = body_of(&received.recv().await.unwrap());
    let contents = second["contents"].as_array().unwrap();
    let parts = contents[1]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0]["toolCall"]["id"], "s1");
    assert_eq!(parts[0]["thoughtSignature"], "sig-a");
    assert_eq!(parts[1]["toolResponse"]["id"], "s1");
    assert_eq!(parts[2]["thoughtSignature"], "sig-b");
    assert_eq!(contents[2]["parts"][0]["functionResponse"]["id"], "fc1");
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["response"]["output"],
        "result of fc1"
    );
}

#[tokio::test]
async fn gemini_2_gets_remas_search_tools_instead_of_search_next_to_functions() {
    let (base_url, mut received) = serve_sequence(vec![(
        200,
        vec!["data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Ok.\"}]},\"finishReason\":\"STOP\"}]}\n\n"],
    )])
    .await;
    let endpoint = endpoint(ProviderKind::Gemini, base_url, "gemini-key");
    let web = Arc::new(RecordingWeb::default());
    let mut request = request();
    request.tools = Some(tool_box("mcp_rema_search_jobs", Arc::new(fake::NoTools)));
    request.web = Some(WebSearch {
        observer: Some(web.clone()),
        fallback: Some(ToolBox {
            specs: vec![
                ToolSpec {
                    name: "rema_career_search".into(),
                    description: "Search.".into(),
                    input_schema: json!({ "type": "object" }),
                },
                ToolSpec {
                    name: "mcp_rema_search_jobs".into(),
                    description: "A tool.".into(),
                    input_schema: json!({ "type": "object" }),
                },
            ],
            executor: Arc::new(fake::NoTools),
        }),
        ..WebSearch::default()
    });
    let (result, text) = run(
        &endpoint,
        "gemini-2.5-flash",
        &request,
        &ProviderLanguageModel::new(None),
    )
    .await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Ok.");
    let body = body_of(&received.recv().await.unwrap());
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1, "no google_search next to functions");
    let names: Vec<&str> = tools[0]["functionDeclarations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["rema_career_search", "mcp_rema_search_jobs"]);
    assert!(body.get("toolConfig").is_none());
    // Not a refusal of Gemini's search: nothing is reported unavailable.
    assert!(web.0.lock().unwrap().is_empty());
}

fn compat_tool_call(name: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"function\":{{\"name\":\"{name}\",\"arguments\":\"{{}}\"}}}}]}}}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n"
    )
}

#[tokio::test]
async fn calls_without_provider_ids_get_ids_no_other_call_has() {
    let call: &'static str = Box::leak(compat_tool_call("lookup").into_boxed_str());
    let (base_url, mut received) = serve_sequence(vec![
        (200, vec![call]),
        (200, vec![call]),
        (
            200,
            vec!["data: {\"choices\":[{\"delta\":{\"content\":\"Answer.\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"],
        ),
    ])
    .await;
    let endpoint = endpoint(ProviderKind::OpenaiCompatible, base_url, "k");
    let tools = Arc::new(RecordingTools::default());
    let mut request = request();
    request.tools = Some(tool_box("lookup", tools.clone()));
    let (result, text) = run(
        &endpoint,
        "qwen3",
        &request,
        &ProviderLanguageModel::new(None),
    )
    .await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Answer.");
    let ids: Vec<String> = tools
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.id.clone())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(
        ids[0], ids[1],
        "each call keeps its own activity and approval"
    );
    assert!(ids.iter().all(|id| is_synthetic_call_id(id)));
    let _ = received.recv().await;
    let _ = received.recv().await;
    let last = body_of(&received.recv().await.unwrap());
    let results: Vec<&str> = last["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["tool_call_id"].as_str().unwrap())
        .collect();
    assert_eq!(results, [ids[0].as_str(), ids[1].as_str()]);
}

#[tokio::test]
async fn a_long_tool_loop_ends_with_an_answer_instead_of_an_error() {
    let call: &'static str = Box::leak(compat_tool_call("lookup").into_boxed_str());
    let mut responses: Vec<(u16, Vec<&'static str>)> =
        (0..MAX_TOOL_ROUNDS).map(|_| (200, vec![call])).collect();
    responses.push((
        200,
        vec!["data: {\"choices\":[{\"delta\":{\"content\":\"What I found so far.\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"],
    ));
    let (base_url, mut received) = serve_sequence(responses).await;
    let endpoint = endpoint(ProviderKind::OpenaiCompatible, base_url, "k");
    let tools = Arc::new(RecordingTools::default());
    let mut request = request();
    request.tools = Some(tool_box("lookup", tools.clone()));
    let (result, text) = run(
        &endpoint,
        "qwen3",
        &request,
        &ProviderLanguageModel::new(None),
    )
    .await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "What I found so far.");
    assert_eq!(tools.0.lock().unwrap().len(), MAX_TOOL_ROUNDS);
    let mut bodies = Vec::new();
    while let Ok(request) = received.try_recv() {
        bodies.push(body_of(&request));
    }
    assert_eq!(bodies.len(), MAX_TOOL_ROUNDS + 1);
    assert!(bodies[..MAX_TOOL_ROUNDS]
        .iter()
        .all(|b| b.get("tool_choice").is_none()));
    assert_eq!(bodies[MAX_TOOL_ROUNDS]["tool_choice"], "none");
}

#[tokio::test]
async fn an_overloaded_provider_is_asked_again_before_anything_streamed() {
    let (base_url, mut received) = serve_sequence(vec![
        (
            529,
            vec!["{\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}"],
        ),
        (
            200,
            vec![
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Ok\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        ),
    ])
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let (result, text) = collect(&endpoint, CancellationToken::new()).await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Ok");
    assert!(received.recv().await.is_some());
    assert!(received.recv().await.is_some(), "sent twice");
}

#[tokio::test]
async fn an_empty_quota_is_not_asked_again() {
    // One response only: a retry would find nothing listening.
    let (base_url, _) = serve_sequence(vec![(
        429,
        vec!["{\"error\":{\"message\":\"You exceeded your current quota, please check your plan and billing details.\",\"type\":\"insufficient_quota\"}}"],
    )])
    .await;
    let endpoint = endpoint(ProviderKind::Openai, base_url, "sk-test");
    let (result, _) = collect(&endpoint, CancellationToken::new()).await;
    assert!(matches!(result.unwrap_err(), AppError::Billing(_)));
}

#[tokio::test]
async fn openai_reasoning_goes_back_with_the_calls_it_led_to() {
    let (base_url, mut received) = serve_sequence(vec![
        (
            200,
            vec![
                "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"status\":\"completed\",\"summary\":[],\"encrypted_content\":\"enc-1\"}}\n\n",
                "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_2\",\"summary\":[]}}\n\n",
                "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_a\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
                "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"delta\":\"{}\"}\n\n",
                "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
            ],
        ),
        (
            200,
            vec![
                "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"Done.\"}\n\n",
                "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
            ],
        ),
    ])
    .await;
    let endpoint = endpoint(ProviderKind::Openai, base_url, "sk-test");
    let mut request = request();
    request.tools = Some(tool_box("lookup", Arc::new(RecordingTools::default())));
    let (result, text) = run(
        &endpoint,
        "gpt-6",
        &request,
        &ProviderLanguageModel::new(None),
    )
    .await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Done.");
    let _ = received.recv().await;
    let second = body_of(&received.recv().await.unwrap());
    let input = second["input"].as_array().unwrap();
    // The encrypted reasoning, then the call it led to; a reasoning item
    // without encrypted content (nothing to send back) is left out.
    assert_eq!(
        input[1],
        json!({ "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "enc-1" })
    );
    assert_eq!(input[2]["type"], "function_call");
    assert_eq!(input[3]["type"], "function_call_output");
    assert_eq!(input.len(), 4);
}

#[tokio::test]
async fn each_claude_console_request_takes_the_current_token() {
    let (base_url, received) = serve_once(
        200,
        vec![
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Ok\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ],
        false,
    )
    .await;
    let mut console = crate::accounts::fake::FakeAccountRuntime::signed_out();
    console.credential = Some(Credential::OAuth {
        access_token: "fresh-console-token".into(),
        refresh_token: None,
        expires_at: None,
    });
    let llm = ProviderLanguageModel::new(None).with_console(Arc::new(console));
    let endpoint = Endpoint {
        connection: ConnectionMethod::ClaudeConsole,
        credential: Some(Credential::OAuth {
            access_token: "expired-console-token".into(),
            refresh_token: None,
            expires_at: None,
        }),
        ..endpoint(ProviderKind::Anthropic, base_url, "unused")
    };
    let (result, text) = run(&endpoint, "claude-opus-5", &request(), &llm).await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Ok");
    let request = received.await.unwrap().to_ascii_lowercase();
    assert!(request.contains("authorization: bearer fresh-console-token"));
    assert!(!request.contains("expired-console-token"));
}

/// A mail tool: running it marks the answer as having read private data.
struct PrivateTools(Arc<std::sync::atomic::AtomicBool>);

impl ToolExecutor for PrivateTools {
    fn execute<'a>(&'a self, _call: &'a ToolCall) -> BoxFuture<'a, ToolOutput> {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async {
            ToolOutput {
                content: "One email from Acme.".into(),
                is_error: false,
            }
        })
    }
}

#[tokio::test]
async fn web_search_stops_once_a_tool_returned_private_data() {
    let (base_url, mut received) = serve_sequence(vec![
        (
            200,
            vec![
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"mail_search\",\"input\":{}}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        ),
        (
            200,
            vec![
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Acme wrote.\"}}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        ),
    ])
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let private = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let web = Arc::new(RecordingWeb::default());
    let mut request = request();
    request.tools = Some(tool_box(
        "mail_search",
        Arc::new(PrivateTools(private.clone())),
    ));
    request.web = Some(WebSearch {
        observer: Some(web.clone()),
        ..WebSearch::default()
    });
    request.private = Some(private);
    let (result, text) = run(
        &endpoint,
        "claude-opus-5",
        &request,
        &ProviderLanguageModel::new(None),
    )
    .await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Acme wrote.");

    let types = |body: &Value| -> Vec<String> {
        body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["type"].as_str().unwrap_or("custom").to_string())
            .collect()
    };
    // Before any private data: the web next to the mail tool.
    let first = body_of(&received.recv().await.unwrap());
    assert!(types(&first).iter().any(|t| t.starts_with("web_search")));
    // After the mail tool returned: no web tools, and the reason is shown.
    let second = body_of(&received.recv().await.unwrap());
    assert!(
        types(&second).iter().all(|t| !t.starts_with("web_")),
        "{:?}",
        types(&second)
    );
    assert!(web
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, WebEvent::Unavailable { reason } if reason.contains("mail"))));
}

/// Claude asks for a search and ReMa's mail tool in the same step (the API
/// holds the search until the next request). Once the mail tool returns,
/// the next request has no web tools, so the held search is left out and
/// shown as not run; the thinking written with the web tools declared is
/// dropped by the API instead of the request being refused.
#[tokio::test]
async fn a_search_held_next_to_a_private_tool_is_dropped_with_the_web() {
    let (base_url, mut received) = serve_sequence(vec![
        (
            200,
            vec![
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Mail and web.\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"server_tool_use\",\"id\":\"srvtoolu_1\",\"name\":\"web_search\",\"input\":{}}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\\\"Acme GmbH\\\"}\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"mail_search\",\"input\":{}}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        ),
        (
            200,
            vec![
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Acme wrote.\"}}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ],
        ),
    ])
    .await;
    let endpoint = endpoint(ProviderKind::Anthropic, base_url, "sk-ant-test");
    let private = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let web = Arc::new(RecordingWeb::default());
    let mut request = request();
    request.tools = Some(tool_box(
        "mail_search",
        Arc::new(PrivateTools(private.clone())),
    ));
    request.web = Some(WebSearch {
        observer: Some(web.clone()),
        ..WebSearch::default()
    });
    request.private = Some(private);
    let (result, text) = run(
        &endpoint,
        "claude-opus-5-5",
        &request,
        &ProviderLanguageModel::new(None),
    )
    .await;
    assert_eq!(result.unwrap(), Finish::Complete);
    assert_eq!(text, "Acme wrote.");

    let first = received.recv().await.unwrap();
    assert!(!first.to_ascii_lowercase().contains("anthropic-beta"));
    assert!(body_of(&first).get("thinking").is_none());

    let second = received.recv().await.unwrap();
    assert!(second
        .to_ascii_lowercase()
        .contains("anthropic-beta: thinking-binding-controls-2026-08-01"));
    let body = body_of(&second);
    assert_eq!(
        body["thinking"]["block_binding"]["prefix_mismatch_behavior"],
        "drop_block"
    );
    let tools = body["tools"].as_array().unwrap();
    assert!(tools.iter().all(|t| t.get("type").is_none()), "{tools:?}");
    // The step goes back without the held search; the mail call and its
    // result stay.
    let step: Vec<&str> = body["messages"][1]["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["type"].as_str().unwrap())
        .collect();
    assert_eq!(step, ["thinking", "tool_use"]);
    assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "toolu_1");

    let events = web.0.lock().unwrap();
    assert!(events.iter().any(|e| matches!(e,
        WebEvent::Finished { id, error: Some(_), target, .. }
            if id == "srvtoolu_1" && target == "Acme GmbH")));
    assert!(events
        .iter()
        .any(|e| matches!(e, WebEvent::Unavailable { .. })));
}
