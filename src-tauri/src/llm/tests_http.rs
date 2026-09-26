//! End-to-end adapter tests against a local mock HTTP server.

use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};
use tokio_util::sync::CancellationToken;

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
