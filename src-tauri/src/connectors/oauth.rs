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

use crate::{
    error::{AppError, AppResult},
    llm::http::scrub,
    oauth_loopback::{self, Loopback, Pages},
};

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
/// `None` for unrelated requests (favicon etc.). A wrong or missing `state`
/// is rejected (CSRF protection).
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

/// Waits for the browser to hit the loopback redirect and returns the code.
pub async fn wait_for_code(
    loopback: Loopback,
    expected_state: &str,
    provider: &str,
    cancel: &CancellationToken,
    timeout: Duration,
) -> AppResult<String> {
    let target = oauth_loopback::receive_on(
        loopback,
        |target| parse_callback(target, expected_state).is_some(),
        |target| {
            matches!(
                parse_callback(target, expected_state),
                Some(Ok(Callback::Code(_)))
            )
        },
        Pages {
            service: provider,
            success_title: Some("ReMa connected successfully."),
        },
        cancel,
        timeout,
    )
    .await?;
    match parse_callback(&target, expected_state) {
        Some(Ok(Callback::Code(code))) => Ok(code),
        Some(Ok(Callback::Denied { error, description })) => Err(match error.as_str() {
            "access_denied" | "consent_required" => {
                AppError::validation(format!("{provider} access was not granted."))
            }
            _ => AppError::authentication(format!(
                "{provider} sign-in failed ({error}{}).",
                description
                    .map(|d| format!(": {}", d.chars().take(160).collect::<String>()))
                    .unwrap_or_default()
            )),
        }),
        Some(Err(error)) => Err(error),
        None => Err(AppError::authentication(format!(
            "{provider} sign-in did not complete."
        ))),
    }
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

/// Why a token request failed.
#[derive(Debug)]
pub enum TokenFailure {
    /// The grant is no longer valid (revoked, expired, password changed,
    /// consent withdrawn): the user must sign in again.
    Reauthenticate(String),
    Other(AppError),
}

impl TokenFailure {
    pub fn into_error(self, provider: &str) -> AppError {
        match self {
            Self::Reauthenticate(_) => AppError::authentication(format!(
                "{provider} access was revoked or has expired. Reconnect in Settings → Connectors."
            )),
            Self::Other(error) => error,
        }
    }
}

/// POSTs a token request. Secret form values are scrubbed from any error.
pub async fn token_request(
    http: &reqwest::Client,
    token_url: &str,
    app: &OAuthApp,
    provider: &str,
    mut form: Vec<(&str, String)>,
) -> Result<TokenResponse, TokenFailure> {
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
            TokenFailure::Other(AppError::provider(format!(
                "{provider} could not be reached ({}).",
                if e.is_timeout() {
                    "timed out"
                } else {
                    "network error"
                }
            )))
        })?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if status.is_success() {
        return serde_json::from_str(&body).map_err(|_| {
            TokenFailure::Other(AppError::provider(format!(
                "{provider} sent an unreadable token response."
            )))
        });
    }
    let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
    match serde_json::from_str::<TokenError>(&body).ok() {
        Some(e)
            if matches!(
                e.error.as_str(),
                "invalid_grant" | "interaction_required" | "consent_required" | "login_required"
            ) =>
        {
            Err(TokenFailure::Reauthenticate(scrub(
                &e.error_description.unwrap_or(e.error),
                &refs,
            )))
        }
        Some(e) if e.error == "invalid_client" || e.error == "unauthorized_client" => {
            Err(TokenFailure::Other(AppError::configuration(format!(
                "{provider} rejected ReMa's app registration ({}). This build of ReMa needs an \
                 updated {provider} sign-in configuration.",
                e.error
            ))))
        }
        Some(e) => {
            let detail = scrub(&e.error_description.unwrap_or(e.error), &refs);
            Err(TokenFailure::Other(AppError::authentication(format!(
                "{provider} sign-in failed: {}",
                detail.chars().take(300).collect::<String>()
            ))))
        }
        None if status.as_u16() == 429 => Err(TokenFailure::Other(AppError::provider(format!(
            "{provider} is rate limiting sign-ins. Try again in a minute."
        )))),
        None => Err(TokenFailure::Other(AppError::provider(format!(
            "{provider} sign-in failed ({}).",
            status.as_u16()
        )))),
    }
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
