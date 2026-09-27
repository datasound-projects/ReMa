//! Authenticated requests to provider APIs (Gmail, Google Calendar,
//! Microsoft Graph).
//!
//! The access token is fetched from the token manager for every request
//! (cached in memory) and sent only to the provider's own API origin. A 401
//! triggers one silent refresh and one retry; a rate limit (429/503) one
//! short, bounded wait and one retry. Error text never contains a token.

use std::{sync::Arc, time::Duration};

use reqwest::{Method, RequestBuilder, Url};
use serde_json::Value;

use super::tokens;
use crate::{
    error::{AppError, AppResult},
    llm::{
        http::{error_message, scrub},
        BoxFuture,
    },
    models::connectors::ProviderId,
    state::AppState,
};

/// Where an API client gets its access token.
pub trait TokenSource: Send + Sync {
    /// A valid token; `refresh` forces a new one (after a 401).
    fn token(&self, refresh: bool) -> BoxFuture<'_, AppResult<String>>;
}

/// Tokens from the connector token manager.
pub struct ConnectorTokens {
    pub state: AppState,
    pub provider: ProviderId,
}

impl TokenSource for ConnectorTokens {
    fn token(&self, refresh: bool) -> BoxFuture<'_, AppResult<String>> {
        Box::pin(async move {
            if refresh {
                tokens::refresh_access_token(&self.state, self.provider).await
            } else {
                tokens::get_valid_access_token(&self.state, self.provider).await
            }
        })
    }
}

/// A fixed token (tests).
pub struct StaticToken(pub String);

impl TokenSource for StaticToken {
    fn token(&self, _refresh: bool) -> BoxFuture<'_, AppResult<String>> {
        let token = self.0.clone();
        Box::pin(async move { Ok(token) })
    }
}

/// A provider API response.
#[derive(Debug)]
pub struct ApiResponse {
    pub status: u16,
    pub body: String,
}

impl ApiResponse {
    pub fn json(&self, service: &str) -> AppResult<Value> {
        if self.body.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&self.body)
            .map_err(|_| AppError::provider(format!("{service} sent an unreadable response.")))
    }

    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// An authenticated client for one provider API.
#[derive(Clone)]
pub struct ApiClient {
    pub http: reqwest::Client,
    pub tokens: Arc<dyn TokenSource>,
    /// "Gmail", "Google Calendar", "Outlook Mail", …
    pub service: &'static str,
    /// The connector name shown in "Reconnect … in Settings".
    pub connector: &'static str,
    /// The API base URL; the token is never sent anywhere else.
    pub base: String,
}

/// Longest wait honoured for `Retry-After` before the single retry.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

impl ApiClient {
    pub fn new(
        http: reqwest::Client,
        tokens: Arc<dyn TokenSource>,
        service: &'static str,
        connector: &'static str,
        base: &str,
    ) -> Self {
        Self {
            http,
            tokens,
            service,
            connector,
            base: base.trim_end_matches('/').to_string(),
        }
    }

    /// `path` below the base, or a full URL the provider returned (Graph
    /// `@odata.nextLink` / `deltaLink`), which must stay on the API origin.
    pub fn url(&self, path_or_url: &str) -> AppResult<Url> {
        let url = if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
            Url::parse(path_or_url)
        } else {
            Url::parse(&format!(
                "{}/{}",
                self.base,
                path_or_url.trim_start_matches('/')
            ))
        }
        .map_err(|_| AppError::provider(format!("{} returned an invalid link.", self.service)))?;
        let base = Url::parse(&self.base).map_err(|e| AppError::internal(e.to_string()))?;
        if url.origin() != base.origin() || !url.path().starts_with(base.path()) {
            return Err(AppError::provider(format!(
                "{} returned a link to another site; it was not followed.",
                self.service
            )));
        }
        Ok(url)
    }

    /// Sends a request built by `build` (called again for a retry).
    pub async fn send(
        &self,
        method: Method,
        path_or_url: &str,
        build: impl Fn(RequestBuilder) -> RequestBuilder,
    ) -> AppResult<ApiResponse> {
        let url = self.url(path_or_url)?;
        let mut refreshed = false;
        let mut waited = false;
        loop {
            let token = self.tokens.token(refreshed).await?;
            let request = build(
                self.http
                    .request(method.clone(), url.clone())
                    .bearer_auth(&token)
                    .timeout(REQUEST_TIMEOUT),
            );
            let response = request.send().await.map_err(|e| {
                AppError::provider(format!(
                    "{} could not be reached ({}).",
                    self.service,
                    if e.is_timeout() {
                        "timed out"
                    } else {
                        "network error"
                    }
                ))
            })?;
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            let body = response.text().await.unwrap_or_default();
            match status {
                401 if !refreshed => {
                    refreshed = true;
                    continue;
                }
                429 | 503 if !waited => {
                    waited = true;
                    let wait = Duration::from_secs(retry_after.unwrap_or(1)).min(MAX_RETRY_WAIT);
                    tokio::time::sleep(wait).await;
                    continue;
                }
                _ => {}
            }
            let body = scrub(&body, &[&token]);
            return Ok(ApiResponse { status, body });
        }
    }

    pub async fn get(&self, path_or_url: &str, query: &[(&str, String)]) -> AppResult<Value> {
        let response = self
            .send(Method::GET, path_or_url, |r| r.query(query))
            .await?;
        self.expect_ok(response)
    }

    /// Fails with a readable error unless the response succeeded.
    pub fn expect_ok(&self, response: ApiResponse) -> AppResult<Value> {
        if response.ok() {
            return response.json(self.service);
        }
        Err(self.error(&response))
    }

    /// A readable error for a failed response.
    pub fn error(&self, response: &ApiResponse) -> AppError {
        let detail = serde_json::from_str::<Value>(&response.body)
            .ok()
            .and_then(|v| error_message(&v))
            .unwrap_or_default();
        api_error(self.service, self.connector, response.status, &detail)
    }
}

/// Maps API errors to ReMa errors without exposing tokens or content.
pub fn api_error(service: &str, connector: &str, status: u16, detail: &str) -> AppError {
    let detail: String = detail.chars().take(200).collect();
    let lower = detail.to_lowercase();
    match status {
        401 => AppError::authentication(format!(
            "{service} access expired or was revoked. Reconnect {connector} in Settings → Connectors."
        )),
        403 if lower.contains("insufficient")
            || lower.contains("scope")
            || lower.contains("access is denied")
            || lower.contains("accessdenied") =>
        {
            AppError::authentication(format!(
                "ReMa does not have permission for {service}. Reconnect {connector} in Settings → \
                 Connectors and allow access."
            ))
        }
        403 => AppError::provider(format!("{service} refused the request: {detail}")),
        404 => AppError::not_found(format!("{service}: item not found")),
        429 => AppError::provider(format!("{service} rate limit reached. Try again later.")),
        s if s >= 500 => AppError::provider(format!("{service} is unavailable right now ({s}).")),
        s => AppError::provider(format!("{service} returned an error ({s}): {detail}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockServer;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingTokens(AtomicUsize);

    impl TokenSource for CountingTokens {
        fn token(&self, refresh: bool) -> BoxFuture<'_, AppResult<String>> {
            if refresh {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
            let token = if refresh { "fresh" } else { "stale" }.to_string();
            Box::pin(async move { Ok(token) })
        }
    }

    #[tokio::test]
    async fn refreshes_once_on_401_and_retries() {
        let server = MockServer::start(|req| {
            if req.headers.contains("authorization: bearer fresh") {
                Some((200, r#"{"ok":true}"#.into()))
            } else {
                Some((401, r#"{"error":{"message":"expired"}}"#.into()))
            }
        })
        .await;
        let tokens = Arc::new(CountingTokens(AtomicUsize::new(0)));
        let api = ApiClient::new(
            reqwest::Client::new(),
            tokens.clone(),
            "Gmail",
            "Gmail",
            &server.base_url,
        );
        assert_eq!(api.get("x", &[]).await.unwrap()["ok"], true);
        assert_eq!(tokens.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn never_sends_the_token_to_another_host() {
        let api = ApiClient::new(
            reqwest::Client::new(),
            Arc::new(StaticToken("tok".into())),
            "Outlook Mail",
            "Outlook Mail",
            "https://graph.microsoft.com/v1.0",
        );
        assert!(api
            .url("https://graph.microsoft.com/v1.0/me/messages/delta?$deltatoken=x")
            .is_ok());
        assert!(api.url("https://evil.example.com/v1.0/me").is_err());
        assert!(api.url("https://graph.microsoft.com/beta/me").is_err());
    }

    #[tokio::test]
    async fn waits_briefly_on_rate_limits_then_reports_them() {
        let server = MockServer::start(|_| Some((429, "{}".into()))).await;
        let api = ApiClient::new(
            reqwest::Client::new(),
            Arc::new(StaticToken("tok".into())),
            "Gmail",
            "Gmail",
            &server.base_url,
        );
        let error = api.get("x", &[]).await.unwrap_err();
        assert!(error.to_string().contains("rate limit"), "{error}");
        assert_eq!(server.requests().len(), 2, "one retry");
    }

    #[test]
    fn maps_permission_errors_to_reconnect() {
        let e = api_error(
            "Gmail",
            "Gmail",
            403,
            "Request had insufficient authentication scopes.",
        );
        assert!(matches!(e, AppError::Authentication(_)));
        assert!(e.to_string().contains("Reconnect Gmail"));
    }
}
