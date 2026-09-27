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

/// How often a request that met a temporary failure is sent again.
const RETRIES: u32 = 2;
/// Longest pause a provider's `retry-after` may ask for.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(20);

/// Statuses that mean "try again shortly": rate limits and overloaded or
/// failing servers (Anthropic's 529 "overloaded" included).
fn temporary(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

/// The pause before the next attempt: the provider's `retry-after` when it
/// sends one (capped), else 1 s, then 3 s.
fn retry_wait(response: &Response, attempt: u32) -> Duration {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map_or(backoff(attempt), |s| {
            Duration::from_secs_f64(s).min(MAX_RETRY_WAIT)
        })
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(1 + 2 * u64::from(attempt))
}

/// Like [`send`], but a temporary failure is sent again, at most twice
/// with a short pause, as the providers' own SDKs do: a rate limit, an
/// overloaded or failing server, or a connection that could not be made.
/// Nothing has reached the model's output yet, so a retry cannot repeat
/// anything. An account without credits is not retried.
pub async fn send_retrying(
    request: RequestBuilder,
    cancel: &CancellationToken,
    provider: &str,
    secrets: &[&str],
) -> AppResult<Option<Response>> {
    let mut attempt = 0;
    loop {
        // A request whose body cannot be copied is sent once.
        let Some(this) = request.try_clone() else {
            return send(request, cancel, provider, secrets).await;
        };
        let sent = tokio::select! {
            _ = cancel.cancelled() => return Ok(None),
            sent = this.send() => sent,
        };
        let wait = match sent {
            Ok(response) if response.status().is_success() => return Ok(Some(response)),
            Ok(response) => {
                let status = response.status();
                let wait = retry_wait(&response, attempt);
                let error = error_from_response(response, provider, secrets).await;
                if attempt == RETRIES || !temporary(status) || matches!(error, AppError::Billing(_))
                {
                    return Err(error);
                }
                wait
            }
            Err(error) if attempt < RETRIES && error.is_connect() => backoff(attempt),
            Err(error) => return Err(error.into()),
        };
        attempt += 1;
        tokio::select! {
            _ = cancel.cancelled() => return Ok(None),
            _ = tokio::time::sleep(wait) => {}
        }
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

/// Where each provider's API credits are managed.
pub const ANTHROPIC_BILLING_URL: &str = "https://console.anthropic.com/settings/billing";
pub const OPENAI_BILLING_URL: &str = "https://platform.openai.com/settings/organization/billing";

/// An error that means the account has no credits or quota left.
fn billing_error(detail: &str) -> Option<AppError> {
    let lower = detail.to_lowercase();
    if lower.contains("credit balance is too low") {
        return Some(AppError::Billing(format!(
            "Your Anthropic account has no API credits left, so Claude can't answer. A Claude \
             Console sign-in or an API key uses prepaid API credits, which are separate from a \
             Claude Pro or Max plan. Add credits at {ANTHROPIC_BILLING_URL} or choose another model."
        )));
    }
    if lower.contains("insufficient_quota") || lower.contains("exceeded your current quota") {
        return Some(AppError::Billing(format!(
            "Your OpenAI API account has no quota left. API keys are billed separately from \
             ChatGPT plans. Add credits at {OPENAI_BILLING_URL}, or connect OpenAI with your \
             ChatGPT account in Settings."
        )));
    }
    None
}

/// Google answers a key it does not (or no longer) accept with a 400
/// (`INVALID_ARGUMENT`, reason `API_KEY_INVALID`), not a 401.
fn rejected_key(status: StatusCode, detail: &str) -> bool {
    let lower = detail.to_lowercase();
    status == StatusCode::BAD_REQUEST
        && (lower.contains("api key not valid") || lower.contains("api key expired"))
}

pub fn status_error(status: StatusCode, provider: &str, detail: &str) -> AppError {
    if let Some(error) = billing_error(detail) {
        return error;
    }
    if rejected_key(status, detail) {
        return AppError::authentication(format!("{provider} rejected the API key: {detail}"));
    }
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => AppError::authentication(format!(
            "{provider} rejected the credentials. Reconnect {provider} in Settings."
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

        // Gemini's answer to a key it does not accept.
        for detail in [
            "API key not valid. Please pass a valid API key.",
            "API key expired. Please renew the API key.",
        ] {
            let gemini = status_error(StatusCode::BAD_REQUEST, "Gemini", detail);
            assert!(matches!(gemini, AppError::Authentication(_)), "{detail}");
            assert_eq!(
                gemini.to_string(),
                format!("Gemini rejected the API key: {detail}")
            );
        }
        let malformed = status_error(StatusCode::BAD_REQUEST, "Gemini", "Invalid JSON payload");
        assert!(matches!(malformed, AppError::Provider(_)));

        assert_eq!(
            scrub("key=abcd1234 end", &["abcd1234"]),
            "key=[redacted] end"
        );
    }

    #[test]
    fn explains_empty_credit_balances() {
        let anthropic = status_error(
            StatusCode::BAD_REQUEST,
            "Anthropic",
            "Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits.",
        );
        assert!(matches!(anthropic, AppError::Billing(_)));
        assert!(anthropic.to_string().contains(ANTHROPIC_BILLING_URL));
        assert!(anthropic
            .to_string()
            .contains("separate from a Claude Pro or Max plan"));

        let openai = status_error(
            StatusCode::TOO_MANY_REQUESTS,
            "OpenAI",
            "You exceeded your current quota, please check your plan and billing details.",
        );
        assert!(matches!(openai, AppError::Billing(_)));
        assert!(openai.to_string().contains(OPENAI_BILLING_URL));

        let other = status_error(StatusCode::BAD_REQUEST, "Anthropic", "max_tokens too large");
        assert!(matches!(other, AppError::Provider(_)));
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
