//! HTTP plumbing shared by the provider adapters: sending with cancellation,
//! mapping error responses, and reading SSE streams.

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::sse::{SseEvent, SseParser};
use crate::error::{AppError, AppResult};

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        // A stream that sends nothing for this long is considered dead.
        .read_timeout(Duration::from_secs(180))
        .user_agent(concat!("ReMa/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("HTTP client configuration is valid")
}

/// Sends a request. Returns `None` if cancelled before a response arrived.
pub async fn send(
    request: RequestBuilder,
    cancel: &CancellationToken,
    provider: &str,
    secrets: &[&str],
) -> AppResult<Option<Response>> {
    let response = tokio::select! {
        _ = cancel.cancelled() => return Ok(None),
        response = request.send() => response?,
    };
    if response.status().is_success() {
        Ok(Some(response))
    } else {
        Err(error_from_response(response, provider, secrets).await)
    }
}

/// Sends a request that must complete (no cancellation), e.g. model listing.
pub async fn send_json(
    request: RequestBuilder,
    provider: &str,
    secrets: &[&str],
) -> AppResult<Value> {
    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(error_from_response(response, provider, secrets).await);
    }
    Ok(response.json::<Value>().await?)
}

/// Whether the stream ran to completion or was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEnd {
    Completed,
    Cancelled,
}

/// What the event handler wants next.
pub enum Flow {
    Continue,
    Stop,
}

/// Reads an SSE response, calling `on_event` for each event.
pub async fn read_sse(
    response: Response,
    cancel: &CancellationToken,
    mut on_event: impl FnMut(SseEvent) -> AppResult<Flow>,
) -> AppResult<StreamEnd> {
    let mut parser = SseParser::default();
    let mut body = response.bytes_stream();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Ok(StreamEnd::Cancelled),
            chunk = body.next() => chunk,
        };
        match chunk {
            Some(bytes) => {
                for event in parser.push(&bytes?) {
                    if let Flow::Stop = on_event(event)? {
                        return Ok(StreamEnd::Completed);
                    }
                }
            }
            None => {
                if let Some(event) = parser.finish() {
                    on_event(event)?;
                }
                return Ok(StreamEnd::Completed);
            }
        }
    }
}

/// Extracts a human-readable message from a provider error body.
///
/// OpenAI, Anthropic and Gemini all use `{"error": {"message": ...}}`; other
/// OpenAI-compatible servers use `message`, `detail` or a string `error`.
pub fn error_message(body: &Value) -> Option<String> {
    let candidates = [
        body.pointer("/error/message"),
        body.get("message"),
        body.get("detail"),
        body.get("error"),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Removes secret values from text that will be shown to the user.
pub fn scrub(text: &str, secrets: &[&str]) -> String {
    secrets
        .iter()
        .filter(|s| s.len() >= 4)
        .fold(text.to_string(), |acc, secret| {
            acc.replace(secret, "[redacted]")
        })
}

async fn error_from_response(response: Response, provider: &str, secrets: &[&str]) -> AppError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|json| error_message(&json))
        .unwrap_or_else(|| {
            body.chars()
                .take(300)
                .collect::<String>()
                .trim()
                .to_string()
        });
    status_error(status, provider, &scrub(&detail, secrets))
}

pub fn status_error(status: StatusCode, provider: &str, detail: &str) -> AppError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => AppError::authentication(format!(
            "{provider} rejected the credentials. Check the API key in Settings."
        )),
        StatusCode::TOO_MANY_REQUESTS => AppError::provider(format!(
            "{provider} rate limit or quota reached{}",
            suffix(detail)
        )),
        StatusCode::NOT_FOUND => {
            AppError::provider(format!("{provider}: not found{}", suffix(detail)))
        }
        _ if status.is_server_error() => AppError::provider(format!(
            "{provider} is unavailable right now ({}){}",
            status.as_u16(),
            suffix(detail)
        )),
        _ => AppError::provider(format!(
            "{provider} returned an error ({}){}",
            status.as_u16(),
            suffix(detail)
        )),
    }
}

fn suffix(detail: &str) -> String {
    if detail.is_empty() {
        String::new()
    } else {
        format!(": {detail}")
    }
}

/// Joins a base URL and a path without doubling slashes.
pub fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_error_messages_from_common_shapes() {
        assert_eq!(
            error_message(&json!({"error": {"message": "bad model"}})).as_deref(),
            Some("bad model")
        );
        assert_eq!(
            error_message(&json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}))
                .as_deref(),
            Some("Overloaded")
        );
        assert_eq!(
            error_message(&json!({"detail": "nope"})).as_deref(),
            Some("nope")
        );
        assert_eq!(
            error_message(&json!({"error": "plain"})).as_deref(),
            Some("plain")
        );
        assert_eq!(error_message(&json!({"ok": true})), None);
    }

    #[test]
    fn maps_statuses_without_leaking_secrets() {
        let auth = status_error(
            StatusCode::UNAUTHORIZED,
            "OpenAI",
            "Incorrect API key sk-live-123",
        );
        assert!(matches!(auth, AppError::Authentication(_)));
        assert!(!auth.to_string().contains("sk-live-123"));

        let limited = status_error(StatusCode::TOO_MANY_REQUESTS, "Anthropic", "slow down");
        assert!(matches!(limited, AppError::Provider(_)));
        assert!(limited.to_string().contains("slow down"));

        assert_eq!(
            scrub("key=abcd1234 end", &["abcd1234"]),
            "key=[redacted] end"
        );
    }

    #[test]
    fn joins_urls() {
        assert_eq!(
            join_url("http://localhost:11434/v1/", "/models"),
            "http://localhost:11434/v1/models"
        );
        assert_eq!(
            join_url("https://api.openai.com/v1", "chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
    }
}
