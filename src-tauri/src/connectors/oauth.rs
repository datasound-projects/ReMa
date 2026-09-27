//! OAuth 2.0 for native apps (RFC 8252), shared by Google and Microsoft:
//! authorization code with PKCE (S256), a cryptographically random `state`,
//! a loopback redirect, and the user's default browser. ReMa never sees a
//! password; codes and tokens stay in Rust.

use std::{fmt, time::Duration};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::failure::{self, Failure, TokenPhase};
use crate::{
    error::{AppError, AppResult},
    llm::http::scrub,
    oauth_loopback::{self, Loopback, Reply, WaitError},
};

/// One `[oauth]` diagnostic line: the provider and the phase of a sign-in,
/// plus facts that are never secret (a port, an error category). Codes,
/// tokens, verifiers, `state` and client secrets are never logged.
pub fn log(provider: &str, phase: &str, facts: &str) {
    let provider = provider.to_ascii_lowercase();
    super::diag(if facts.is_empty() {
        format!("[oauth] provider={provider} phase={phase}")
    } else {
        format!("[oauth] provider={provider} phase={phase} {facts}")
    });
}

/// ReMa's own app registration with a provider. Configured when ReMa is
/// built, never entered by users. A Google "Desktop app" client comes with a
/// secret Google does not treat as confidential; Microsoft public clients
/// have none.
#[derive(Clone)]
pub struct OAuthApp {
    pub client_id: String,
    pub client_secret: Option<String>,
}

impl fmt::Debug for OAuthApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthApp")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
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

impl fmt::Debug for Pkce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pkce(<redacted>)")
    }
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

/// The browser half of a sign-in.
pub struct AuthorizationRequest<'a> {
    pub endpoint: &'a str,
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub scopes: &'a [String],
    pub challenge: &'a str,
    pub state: &'a str,
    /// Provider-specific parameters (offline access, account chooser).
    pub extra: &'a [(&'a str, &'a str)],
}

pub fn authorization_url(request: &AuthorizationRequest<'_>) -> AppResult<String> {
    let mut url = Url::parse(request.endpoint).map_err(|e| AppError::internal(e.to_string()))?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", request.client_id)
            .append_pair("redirect_uri", request.redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", &request.scopes.join(" "))
            .append_pair("code_challenge", request.challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", request.state);
        for (key, value) in request.extra {
            query.append_pair(key, value);
        }
    }
    Ok(url.to_string())
}

/// What the browser brought back to the loopback redirect.
#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    Code(String),
    Denied {
        error: String,
        description: Option<String>,
    },
}

/// Parses the request target of a loopback redirect (`/?state=…&code=…`).
/// `None` for unrelated requests (a favicon, any other path, a request
/// without `code` or `error`). A wrong or missing `state` is rejected (CSRF
/// protection).
pub fn parse_callback(target: &str, expected_state: &str) -> Option<AppResult<Callback>> {
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    // The redirect URI is the bare loopback origin, so the provider comes
    // back to "/" and nowhere else.
    if url.path() != "/" {
        return None;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    let (code, error) = (param("code"), param("error"));
    if code.is_none() && error.is_none() {
        return None;
    }
    let state_ok = param("state").is_some_and(|state| {
        // Constant-time comparison: the state is a secret for this sign-in.
        state.len() == expected_state.len()
            && state
                .bytes()
                .zip(expected_state.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    });
    if !state_ok {
        return Some(Err(AppError::authentication(
            "The sign-in response did not match this request (invalid state). Try again.",
        )));
    }
    Some(Ok(match (code, error) {
        (_, Some(error)) => Callback::Denied {
            error,
            description: param("error_description"),
        },
        (Some(code), None) => Callback::Code(code),
        (None, None) => unreachable!(),
    }))
}

/// Waits for the browser to come back to the loopback redirect. Returns the
/// code with the browser's request, whose page the caller answers once the
/// sign-in has finished. A refusal or a forged `state` is answered with the
/// failure page here. The listener is closed on return, so a repeated or
/// late callback finds nothing.
pub async fn wait_for_callback(
    loopback: Loopback,
    expected_state: &str,
    provider: &str,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<(String, Reply), Failure> {
    let (target, reply) = oauth_loopback::wait_on(
        loopback,
        |target| parse_callback(target, expected_state).is_some(),
        cancel,
        timeout,
    )
    .await
    .map_err(|error| match error {
        WaitError::Cancelled => failure::cancelled(provider),
        WaitError::TimedOut => failure::timed_out(provider),
        WaitError::Failed(error) => failure::network(provider).with_detail(error.to_string()),
    })?;
    log(provider, "callback_received", "");
    let failure = match parse_callback(&target, expected_state) {
        Some(Ok(Callback::Code(code))) => return Ok((code, reply)),
        Some(Ok(Callback::Denied { error, description })) => {
            failure::from_redirect(provider, &error, description.as_deref())
        }
        Some(Err(_)) => failure::invalid_state(provider),
        None => failure::invalid_state(provider),
    };
    reply.send(oauth_loopback::NOT_CONNECTED).await;
    Err(failure)
}

/// A token endpoint response. Never printed.
#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: Option<i64>,
    pub refresh_token: Option<String>,
    /// Space-separated scopes actually granted.
    pub scope: Option<String>,
    pub id_token: Option<String>,
}

impl fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

/// POSTs a token request (code exchange or refresh). Every value sent is
/// scrubbed from the provider's error text before it goes anywhere.
pub async fn token_request(
    http: &reqwest::Client,
    token_url: &str,
    app: &OAuthApp,
    provider: &str,
    phase: TokenPhase,
    mut form: Vec<(&str, String)>,
) -> Result<TokenResponse, Failure> {
    form.push(("client_id", app.client_id.clone()));
    if let Some(secret) = &app.client_secret {
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
        .post(token_url)
        .form(&form)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| {
            failure::network(provider).with_detail(if e.is_timeout() {
                "timed out"
            } else {
                "network error"
            })
        })?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if status.is_success() {
        return serde_json::from_str(&body).map_err(|_| {
            Failure::new(
                crate::models::connectors::ConnectorErrorCode::TokenExchangeFailed,
                format!("{provider} sent an unreadable answer. Click Retry."),
            )
        });
    }
    let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
    let error = serde_json::from_str::<TokenError>(&body).ok();
    let description = error
        .as_ref()
        .and_then(|e| e.error_description.as_deref())
        .map(|d| scrub(d, &refs));
    Err(failure::from_token_error(
        provider,
        phase,
        status.as_u16(),
        error.as_ref().map(|e| e.error.as_str()),
        description.as_deref(),
    ))
}

/// Claims of an ID token received directly from the provider's token
/// endpoint over TLS (OpenID Connect allows skipping the signature check
/// in that case). Used only to label the account.
pub fn id_token_claims(id_token: &str) -> Option<Value> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!format!("{random:?}").contains(&random.verifier));
    }

    #[test]
    fn states_are_random_and_long() {
        let a = random_token(24).unwrap();
        assert_eq!(a.len(), 32);
        assert_ne!(a, random_token(24).unwrap());
    }

    #[test]
    fn builds_a_pkce_authorization_url_without_secrets() {
        let scopes = vec!["openid".to_string(), "Mail.Read".to_string()];
        let url = authorization_url(&AuthorizationRequest {
            endpoint: "https://login.example.com/authorize",
            client_id: "client-1",
            redirect_uri: "http://localhost:5555",
            scopes: &scopes,
            challenge: "challenge",
            state: "state-1",
            extra: &[("prompt", "select_account")],
        })
        .unwrap();
        let parsed = Url::parse(&url).unwrap();
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["scope"], "openid Mail.Read");
        assert_eq!(q["prompt"], "select_account");
        assert_eq!(q["state"], "state-1");
        assert!(!q.contains_key("client_secret"));
    }

    #[test]
    fn validates_state_and_reads_denials() {
        assert_eq!(
            parse_callback("/?state=s1&code=4%2Fabc", "s1")
                .unwrap()
                .unwrap(),
            Callback::Code("4/abc".into())
        );
        assert_eq!(
            parse_callback("/?error=access_denied&state=s1", "s1")
                .unwrap()
                .unwrap(),
            Callback::Denied {
                error: "access_denied".into(),
                description: None
            }
        );
        // A forged or missing state never yields a code.
        assert!(parse_callback("/?state=other&code=x", "s1")
            .unwrap()
            .is_err());
        assert!(parse_callback("/?code=x", "s1").unwrap().is_err());
        assert!(parse_callback("/?state=s&code=x", "s1").unwrap().is_err());
        assert!(parse_callback("/favicon.ico", "s1").is_none());
    }

    #[test]
    fn token_responses_never_print_tokens() {
        let token: TokenResponse = serde_json::from_str(
            r#"{"access_token":"at-secret","refresh_token":"rt-secret","expires_in":3600,"scope":"a b"}"#,
        )
        .unwrap();
        let debug = format!("{token:?}");
        assert!(!debug.contains("at-secret") && !debug.contains("rt-secret"));
        let app = OAuthApp {
            client_id: "id".into(),
            client_secret: Some("shh".into()),
        };
        assert!(!format!("{app:?}").contains("shh"));
    }

    #[test]
    fn reads_id_token_claims() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"email":"me@example.com","sub":"1"}"#);
        let claims = id_token_claims(&format!("header.{payload}.signature")).unwrap();
        assert_eq!(claims["email"], "me@example.com");
        assert!(id_token_claims("garbage").is_none());
    }
}
