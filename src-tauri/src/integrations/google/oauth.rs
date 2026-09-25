//! Google OAuth 2.0 for installed (desktop) applications.
//!
//! Flow: authorization code with PKCE (S256) and a loopback redirect to
//! `http://127.0.0.1:<random port>`, as Google recommends for desktop apps.
//! The system browser shows Google's consent screen; ReMa never sees the
//! user's password. Tokens are returned to Rust only.

use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

use super::GoogleEndpoints;
use crate::{
    error::{AppError, AppResult},
    llm::http::scrub,
};

/// The OAuth client registered in Google Cloud ("Desktop app" type).
/// Google does not treat a desktop client secret as confidential, but ReMa
/// still keeps it in the OS credential store.
#[derive(Clone)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: Option<String>,
}

/// Cryptographically random URL-safe string.
pub fn random_token(bytes: usize) -> AppResult<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf)
        .map_err(|e| AppError::internal(format!("no secure randomness: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Proof Key for Code Exchange (RFC 7636, S256).
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> AppResult<Self> {
        // 48 random bytes → 64 base64url characters (allowed: 43..=128).
        Ok(Self::from_verifier(random_token(48)?))
    }

    pub fn from_verifier(verifier: String) -> Self {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

pub fn authorization_url(
    endpoints: &GoogleEndpoints,
    client: &OAuthClient,
    redirect_uri: &str,
    scopes: &[&str],
    challenge: &str,
    state: &str,
) -> AppResult<String> {
    let mut url = Url::parse(&endpoints.auth).map_err(|e| AppError::internal(e.to_string()))?;
    url.query_pairs_mut()
        .append_pair("client_id", &client.client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &scopes.join(" "))
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        // A refresh token, so the user authenticates once.
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("include_granted_scopes", "true");
    Ok(url.to_string())
}

/// What the browser brought back to the loopback redirect.
#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    Code(String),
    Denied(String),
}

/// Parses the request target of a loopback redirect, e.g.
/// `/?state=…&code=…`. `None` for unrelated requests (favicon etc.).
pub fn parse_callback(target: &str, expected_state: &str) -> Option<AppResult<Callback>> {
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    let (code, error) = (param("code"), param("error"));
    if code.is_none() && error.is_none() {
        return None;
    }
    if param("state").as_deref() != Some(expected_state) {
        return Some(Err(AppError::authentication(
            "The Google sign-in response did not match this request. Try again.",
        )));
    }
    Some(Ok(match (code, error) {
        (_, Some(error)) => Callback::Denied(error),
        (Some(code), None) => Callback::Code(code),
        (None, None) => unreachable!(),
    }))
}

const SUCCESS_PAGE: &str = "<!doctype html><meta charset=utf-8><title>ReMa</title>\
<body style=\"font-family:-apple-system,Segoe UI,sans-serif;text-align:center;padding:64px\">\
<h2>ReMa is connected to Google</h2><p>You can close this tab and return to ReMa.</p>";
const FAILURE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>ReMa</title>\
<body style=\"font-family:-apple-system,Segoe UI,sans-serif;text-align:center;padding:64px\">\
<h2>Google sign-in was not completed</h2><p>You can close this tab and try again in ReMa.</p>";

/// Waits for the browser to hit the loopback redirect and returns the code.
pub async fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
    cancel: &CancellationToken,
    timeout: Duration,
) -> AppResult<String> {
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    loop {
        let (mut socket, _) = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::validation("Google sign-in was cancelled.")),
            _ = &mut deadline => return Err(AppError::validation("Google sign-in timed out. Try again.")),
            accepted = listener.accept() => accepted?,
        };
        let mut buf = vec![0u8; 8192];
        let mut len = 0;
        // Read until the end of the request headers (the query carries all we need).
        while len < buf.len() {
            let n = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut buf[len..]))
                .await
                .unwrap_or(Ok(0))?;
            if n == 0 {
                break;
            }
            len += n;
            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8_lossy(&buf[..len]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/");

        let outcome = parse_callback(target, expected_state);
        let (status, body) = match &outcome {
            Some(Ok(Callback::Code(_))) => ("200 OK", SUCCESS_PAGE),
            Some(_) => ("200 OK", FAILURE_PAGE),
            None => ("404 Not Found", ""),
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.shutdown().await;

        match outcome {
            None => continue,
            Some(Ok(Callback::Code(code))) => return Ok(code),
            Some(Ok(Callback::Denied(error))) => {
                return Err(AppError::authentication(if error == "access_denied" {
                    "Google access was not granted.".to_string()
                } else {
                    format!("Google sign-in failed ({error}).")
                }))
            }
            Some(Err(error)) => return Err(error),
        }
    }
}

/// Google's token endpoint response.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: Option<i64>,
    pub refresh_token: Option<String>,
    /// Space-separated scopes actually granted.
    pub scope: Option<String>,
    pub id_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

/// Why a token request failed.
#[derive(Debug)]
pub enum TokenFailure {
    /// The grant is no longer valid (revoked, expired): sign in again.
    InvalidGrant,
    Other(AppError),
}

impl From<TokenFailure> for AppError {
    fn from(failure: TokenFailure) -> Self {
        match failure {
            TokenFailure::InvalidGrant => AppError::authentication(
                "Google access was revoked or has expired. Reconnect Google in Settings.",
            ),
            TokenFailure::Other(error) => error,
        }
    }
}

async fn token_request(
    http: &reqwest::Client,
    endpoints: &GoogleEndpoints,
    client: &OAuthClient,
    mut form: Vec<(&str, String)>,
) -> Result<TokenResponse, TokenFailure> {
    form.push(("client_id", client.client_id.clone()));
    if let Some(secret) = &client.client_secret {
        form.push(("client_secret", secret.clone()));
    }
    let secrets: Vec<String> = form
        .iter()
        .filter(|(k, _)| {
            matches!(
                *k,
                "code" | "refresh_token" | "code_verifier" | "client_secret"
            )
        })
        .map(|(_, v)| v.clone())
        .collect();
    let response = http
        .post(&endpoints.token)
        .form(&form)
        .send()
        .await
        .map_err(|e| TokenFailure::Other(e.into()))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if status.is_success() {
        return serde_json::from_str(&body).map_err(|_| {
            TokenFailure::Other(AppError::provider(
                "Google sent an unreadable token response.",
            ))
        });
    }
    let error = serde_json::from_str::<TokenError>(&body).ok();
    match error {
        Some(e) if e.error == "invalid_grant" => Err(TokenFailure::InvalidGrant),
        Some(e) if e.error == "invalid_client" || e.error == "unauthorized_client" => {
            Err(TokenFailure::Other(AppError::configuration(
                "Google rejected the OAuth client. Check the client ID and secret in Settings.",
            )))
        }
        Some(e) => {
            let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
            let detail = scrub(&e.error_description.unwrap_or(e.error), &refs);
            Err(TokenFailure::Other(AppError::authentication(format!(
                "Google sign-in failed: {detail}"
            ))))
        }
        None => Err(TokenFailure::Other(AppError::provider(format!(
            "Google sign-in failed ({}).",
            status.as_u16()
        )))),
    }
}

pub async fn exchange_code(
    http: &reqwest::Client,
    endpoints: &GoogleEndpoints,
    client: &OAuthClient,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> AppResult<TokenResponse> {
    token_request(
        http,
        endpoints,
        client,
        vec![
            ("grant_type", "authorization_code".into()),
            ("code", code.into()),
            ("code_verifier", verifier.into()),
            ("redirect_uri", redirect_uri.into()),
        ],
    )
    .await
    .map_err(AppError::from)
}

pub async fn refresh(
    http: &reqwest::Client,
    endpoints: &GoogleEndpoints,
    client: &OAuthClient,
    refresh_token: &str,
) -> Result<TokenResponse, TokenFailure> {
    token_request(
        http,
        endpoints,
        client,
        vec![
            ("grant_type", "refresh_token".into()),
            ("refresh_token", refresh_token.into()),
        ],
    )
    .await
}

/// Revokes a token at Google (best effort; the local copy is deleted anyway).
pub async fn revoke(
    http: &reqwest::Client,
    endpoints: &GoogleEndpoints,
    token: &str,
) -> AppResult<()> {
    let response = http
        .post(&endpoints.revoke)
        .form(&[("token", token)])
        .send()
        .await?;
    // 400 means the token was already invalid: nothing left to revoke.
    if response.status().is_success() || response.status().as_u16() == 400 {
        Ok(())
    } else {
        Err(AppError::provider(format!(
            "Google could not revoke access ({}).",
            response.status().as_u16()
        )))
    }
}

/// The account email from an ID token received directly from Google's
/// token endpoint over TLS (no signature check needed in that case).
pub fn email_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims.get("email")?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoints() -> GoogleEndpoints {
        GoogleEndpoints::default()
    }

    fn client() -> OAuthClient {
        OAuthClient {
            client_id: "client-123.apps.googleusercontent.com".into(),
            client_secret: Some("secret".into()),
        }
    }

    #[test]
    fn pkce_matches_the_rfc_7636_example() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );

        let random = Pkce::new().unwrap();
        assert_eq!(random.verifier.len(), 64);
        assert!(random
            .verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(Pkce::new().unwrap().verifier, random.verifier);
    }

    #[test]
    fn builds_a_pkce_authorization_url_with_offline_access() {
        let url = authorization_url(
            &endpoints(),
            &client(),
            "http://127.0.0.1:5555",
            &[
                "openid",
                "email",
                "https://www.googleapis.com/auth/gmail.readonly",
            ],
            "challenge",
            "state-1",
        )
        .unwrap();
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.host_str(), Some("accounts.google.com"));
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], "challenge");
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5555");
        assert_eq!(
            q["scope"],
            "openid email https://www.googleapis.com/auth/gmail.readonly"
        );
        assert!(
            !url.contains("secret"),
            "the client secret never goes to the browser"
        );
    }

    #[test]
    fn parses_loopback_callbacks_and_checks_state() {
        assert_eq!(
            parse_callback("/?state=s1&code=4%2Fabc&scope=x", "s1")
                .unwrap()
                .unwrap(),
            Callback::Code("4/abc".into())
        );
        assert_eq!(
            parse_callback("/?error=access_denied&state=s1", "s1")
                .unwrap()
                .unwrap(),
            Callback::Denied("access_denied".into())
        );
        assert!(parse_callback("/?state=other&code=x", "s1")
            .unwrap()
            .is_err());
        assert!(parse_callback("/favicon.ico", "s1").is_none());
    }

    #[test]
    fn reads_the_email_from_an_id_token() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"email":"me@example.com","sub":"1"}"#);
        assert_eq!(
            email_from_id_token(&format!("header.{payload}.signature")).as_deref(),
            Some("me@example.com")
        );
        assert_eq!(email_from_id_token("garbage"), None);
    }

    #[tokio::test]
    async fn receives_the_code_on_the_loopback_redirect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = tokio::spawn(async move {
            let http = reqwest::Client::new();
            // Unrelated request first, then the real redirect.
            let _ = http
                .get(format!("http://127.0.0.1:{port}/favicon.ico"))
                .send()
                .await;
            http.get(format!("http://127.0.0.1:{port}/?state=st&code=the-code"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        });
        let code = wait_for_callback(
            listener,
            "st",
            &CancellationToken::new(),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(code, "the-code");
        assert!(browser.await.unwrap().contains("connected to Google"));
    }

    #[tokio::test]
    async fn sign_in_can_be_cancelled() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = wait_for_callback(listener, "st", &cancel, Duration::from_secs(5)).await;
        assert!(matches!(result, Err(AppError::Validation(_))));
    }
}
