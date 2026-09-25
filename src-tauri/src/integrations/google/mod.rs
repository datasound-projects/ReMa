//! Google Workspace connection: OAuth, credential storage, token refresh.
//!
//! Tokens live only in the OS credential store and in Rust memory. The UI
//! sees a [`GoogleStatus`] (connected, account email, which services are
//! enabled and granted) and never a token.

pub mod calendar;
pub mod gmail;
pub mod oauth;

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{
    db::providers as settings_repo,
    error::{AppError, AppResult},
    models::google::{GoogleClientSource, GoogleService, GoogleServiceStatus, GoogleStatus},
    secrets::Credential,
    state::AppState,
    time::now_ms,
};
use oauth::OAuthClient;

/// Minimal scopes. `openid email` identifies the account in Settings.
pub const SCOPE_OPENID: &str = "openid";
pub const SCOPE_EMAIL: &str = "email";
/// Read messages and search the mailbox (no send, modify or delete).
pub const SCOPE_GMAIL_READONLY: &str = "https://www.googleapis.com/auth/gmail.readonly";
/// Read events and create/update events ReMa manages (no calendar settings or sharing).
pub const SCOPE_CALENDAR_EVENTS: &str = "https://www.googleapis.com/auth/calendar.events";

/// Keychain accounts.
const TOKEN_ACCOUNT: &str = "google";
const CLIENT_SECRET_ACCOUNT: &str = "google-oauth-client";

/// Non-secret settings.
const KEY_CLIENT_ID: &str = "google.client_id";
const KEY_EMAIL: &str = "google.email";
const KEY_SCOPES: &str = "google.scopes";
const KEY_NEEDS_RECONNECT: &str = "google.needs_reconnect";
const KEY_GMAIL_ENABLED: &str = "google.gmail_enabled";
const KEY_CALENDAR_ENABLED: &str = "google.calendar_enabled";

/// How long a sign-in waits for the browser.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Refresh access tokens this long before they expire.
/// Refresh this long before expiry: longer than a task run may take (15
/// minutes), so a token fetched when a run starts stays valid until it ends.
const EXPIRY_MARGIN_MS: i64 = 20 * 60_000;

/// Google endpoints. Overridable in debug builds for local testing.
#[derive(Debug, Clone)]
pub struct GoogleEndpoints {
    pub auth: String,
    pub token: String,
    pub revoke: String,
    pub gmail: String,
    pub calendar: String,
}

impl Default for GoogleEndpoints {
    fn default() -> Self {
        Self {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
            gmail: "https://gmail.googleapis.com/gmail/v1".into(),
            calendar: "https://www.googleapis.com/calendar/v3".into(),
        }
    }
}

impl GoogleEndpoints {
    /// Official endpoints, or (debug builds only) a local mock server set
    /// with `REMA_GOOGLE_BASE_URL` for end-to-end tests.
    pub fn from_env() -> Self {
        #[cfg(debug_assertions)]
        if let Ok(base) = std::env::var("REMA_GOOGLE_BASE_URL") {
            let base = base.trim_end_matches('/');
            return Self {
                auth: format!("{base}/o/oauth2/v2/auth"),
                token: format!("{base}/token"),
                revoke: format!("{base}/revoke"),
                gmail: format!("{base}/gmail/v1"),
                calendar: format!("{base}/calendar/v3"),
            };
        }
        Self::default()
    }
}

/// Shared Google state: endpoints, HTTP client, in-flight sign-in.
#[derive(Clone)]
pub struct GoogleContext {
    pub endpoints: Arc<GoogleEndpoints>,
    pub http: reqwest::Client,
    /// The sign-in waiting for the browser, tagged with a sequence number.
    sign_in: Arc<Mutex<Option<(u64, CancellationToken)>>>,
    sign_in_seq: Arc<AtomicU64>,
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
}

impl GoogleContext {
    pub fn new(endpoints: GoogleEndpoints) -> Self {
        Self {
            endpoints: Arc::new(endpoints),
            http: crate::llm::http::client(),
            sign_in: Arc::default(),
            sign_in_seq: Arc::default(),
            refresh_lock: Arc::default(),
        }
    }

    fn signing_in(&self) -> bool {
        self.sign_in.lock().map(|s| s.is_some()).unwrap_or(false)
    }
}

fn setting(state: &AppState, key: &str) -> AppResult<Option<String>> {
    state.db.call(|c| settings_repo::get_setting(c, key))
}

fn flag(state: &AppState, key: &str, default: bool) -> AppResult<bool> {
    Ok(setting(state, key)?.map_or(default, |v| v == "1"))
}

fn granted_scopes(state: &AppState) -> AppResult<Vec<String>> {
    Ok(setting(state, KEY_SCOPES)?
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default())
}

/// The OAuth client: one entered in Settings, else one built into ReMa.
async fn oauth_client(state: &AppState) -> AppResult<Option<(OAuthClient, GoogleClientSource)>> {
    if let Some(client_id) = setting(state, KEY_CLIENT_ID)? {
        let secret = match state.vault.get(CLIENT_SECRET_ACCOUNT).await? {
            Some(Credential::ApiKey { key }) => Some(key),
            _ => None,
        };
        return Ok(Some((
            OAuthClient {
                client_id,
                client_secret: secret,
            },
            GoogleClientSource::Custom,
        )));
    }
    Ok(option_env!("REMA_GOOGLE_CLIENT_ID").map(|client_id| {
        (
            OAuthClient {
                client_id: client_id.to_string(),
                client_secret: option_env!("REMA_GOOGLE_CLIENT_SECRET").map(str::to_string),
            },
            GoogleClientSource::Builtin,
        )
    }))
}

pub async fn status(state: &AppState) -> AppResult<GoogleStatus> {
    let client = oauth_client(state).await?.map(|(_, source)| source);
    let connected = state.vault.get(TOKEN_ACCOUNT).await?.is_some();
    let scopes = granted_scopes(state)?;
    let granted = |scope: &str| connected && scopes.iter().any(|s| s == scope);
    Ok(GoogleStatus {
        client,
        connected,
        needs_reconnect: flag(state, KEY_NEEDS_RECONNECT, false)?,
        email: setting(state, KEY_EMAIL)?,
        gmail: GoogleServiceStatus {
            enabled: flag(state, KEY_GMAIL_ENABLED, true)?,
            granted: granted(SCOPE_GMAIL_READONLY),
        },
        calendar: GoogleServiceStatus {
            enabled: flag(state, KEY_CALENDAR_ENABLED, true)?,
            granted: granted(SCOPE_CALENDAR_EVENTS),
        },
        connecting: state.google.signing_in(),
    })
}

/// Stores a user-provided OAuth client (Settings).
pub async fn save_client(state: &AppState, client_id: &str, client_secret: &str) -> AppResult<()> {
    let client_id = client_id.trim();
    if client_id.is_empty() || client_id.len() > 300 || client_id.contains(char::is_whitespace) {
        return Err(AppError::validation(
            "Enter the OAuth client ID from Google Cloud.",
        ));
    }
    let secret = client_secret.trim();
    if secret.is_empty() {
        state.vault.delete(CLIENT_SECRET_ACCOUNT).await?;
    } else {
        state
            .vault
            .set(
                CLIENT_SECRET_ACCOUNT,
                Credential::ApiKey { key: secret.into() },
            )
            .await?;
    }
    state
        .db
        .call(|c| settings_repo::set_setting(c, KEY_CLIENT_ID, client_id))?;
    state.events.google_changed();
    Ok(())
}

/// Scopes for the enabled services.
pub fn scopes_for(gmail: bool, calendar: bool) -> Vec<&'static str> {
    let mut scopes = vec![SCOPE_OPENID, SCOPE_EMAIL];
    if gmail {
        scopes.push(SCOPE_GMAIL_READONLY);
    }
    if calendar {
        scopes.push(SCOPE_CALENDAR_EVENTS);
    }
    scopes
}

/// Runs the sign-in: opens Google's consent page in the browser via
/// `open_browser` and waits for the loopback redirect.
pub async fn connect(
    state: &AppState,
    open_browser: impl FnOnce(&str) -> AppResult<()>,
) -> AppResult<GoogleStatus> {
    let (client, _) = oauth_client(state)
        .await?
        .ok_or_else(|| AppError::configuration("Add a Google OAuth client in Settings first."))?;
    let gmail = flag(state, KEY_GMAIL_ENABLED, true)?;
    let calendar = flag(state, KEY_CALENDAR_ENABLED, true)?;
    if !gmail && !calendar {
        return Err(AppError::validation("Turn on Gmail or Calendar first."));
    }
    let scopes = scopes_for(gmail, calendar);

    let pkce = oauth::Pkce::new()?;
    let csrf_state = oauth::random_token(24)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let redirect_uri = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let url = oauth::authorization_url(
        &state.google.endpoints,
        &client,
        &redirect_uri,
        &scopes,
        &pkce.challenge,
        &csrf_state,
    )?;

    // Only one sign-in at a time; a new one replaces an abandoned one.
    let cancel = CancellationToken::new();
    let seq = state.google.sign_in_seq.fetch_add(1, Ordering::Relaxed);
    if let Some((_, previous)) = state
        .google
        .sign_in
        .lock()
        .unwrap()
        .replace((seq, cancel.clone()))
    {
        previous.cancel();
    }
    state.events.google_changed();

    let result = async {
        open_browser(&url)?;
        let code =
            oauth::wait_for_callback(listener, &csrf_state, &cancel, SIGN_IN_TIMEOUT).await?;
        let tokens = oauth::exchange_code(
            &state.google.http,
            &state.google.endpoints,
            &client,
            &code,
            &pkce.verifier,
            &redirect_uri,
        )
        .await?;
        store_tokens(state, tokens, None).await
    }
    .await;

    {
        let mut slot = state.google.sign_in.lock().unwrap();
        if slot.as_ref().is_some_and(|(current, _)| *current == seq) {
            *slot = None;
        }
    }
    state.events.google_changed();
    result?;
    status(state).await
}

async fn store_tokens(
    state: &AppState,
    tokens: oauth::TokenResponse,
    previous_refresh: Option<String>,
) -> AppResult<()> {
    let refresh_token = tokens.refresh_token.or(previous_refresh);
    if refresh_token.is_none() {
        return Err(AppError::authentication(
            "Google did not grant offline access. Try connecting again.",
        ));
    }
    let expires_at = tokens.expires_in.map(|s| now_ms() + s * 1000);
    state
        .vault
        .set(
            TOKEN_ACCOUNT,
            Credential::OAuth {
                access_token: tokens.access_token,
                refresh_token,
                expires_at,
            },
        )
        .await?;
    let email = tokens
        .id_token
        .as_deref()
        .and_then(oauth::email_from_id_token);
    state.db.call(|c| {
        if let Some(scope) = &tokens.scope {
            settings_repo::set_setting(c, KEY_SCOPES, scope)?;
        }
        if let Some(email) = &email {
            settings_repo::set_setting(c, KEY_EMAIL, email)?;
        }
        settings_repo::delete_setting(c, KEY_NEEDS_RECONNECT)
    })
}

/// Cancels a sign-in that is waiting for the browser.
pub fn cancel_connect(state: &AppState) {
    if let Some((_, cancel)) = state.google.sign_in.lock().unwrap().take() {
        cancel.cancel();
    }
    state.events.google_changed();
}

/// Revokes ReMa's access at Google and deletes the local authorization.
pub async fn disconnect(state: &AppState) -> AppResult<()> {
    if let Some(Credential::OAuth {
        access_token,
        refresh_token,
        ..
    }) = state.vault.get(TOKEN_ACCOUNT).await?
    {
        let token = refresh_token.unwrap_or(access_token);
        // Best effort: the local copy is removed even if Google is unreachable.
        let _ = oauth::revoke(&state.google.http, &state.google.endpoints, &token).await;
    }
    state.vault.delete(TOKEN_ACCOUNT).await?;
    state.db.call(|c| {
        for key in [KEY_EMAIL, KEY_SCOPES, KEY_NEEDS_RECONNECT] {
            settings_repo::delete_setting(c, key)?;
        }
        Ok(())
    })?;
    state.events.google_changed();
    Ok(())
}

pub fn set_service_enabled(
    state: &AppState,
    service: GoogleService,
    enabled: bool,
) -> AppResult<()> {
    let key = match service {
        GoogleService::Gmail => KEY_GMAIL_ENABLED,
        GoogleService::Calendar => KEY_CALENDAR_ENABLED,
    };
    state
        .db
        .call(|c| settings_repo::set_setting(c, key, if enabled { "1" } else { "0" }))?;
    state.events.google_changed();
    Ok(())
}

/// Checks that a service is enabled and granted; explains what to do if not.
pub async fn require(state: &AppState, service: GoogleService) -> AppResult<()> {
    let status = status(state).await?;
    let (name, service_status) = match service {
        GoogleService::Gmail => ("Gmail", status.gmail),
        GoogleService::Calendar => ("Google Calendar", status.calendar),
    };
    if !status.connected {
        return Err(AppError::authentication(
            "Connect Google in Settings first.",
        ));
    }
    if !service_status.enabled {
        return Err(AppError::configuration(format!(
            "{name} is turned off in Settings."
        )));
    }
    if !service_status.granted {
        return Err(AppError::authentication(format!(
            "ReMa does not have {name} access yet. Reconnect Google in Settings."
        )));
    }
    Ok(())
}

/// A valid access token, refreshed automatically when it is about to expire.
pub async fn access_token(state: &AppState) -> AppResult<String> {
    let _guard = state.google.refresh_lock.lock().await;
    let Some(Credential::OAuth {
        access_token,
        refresh_token,
        expires_at,
    }) = state.vault.get(TOKEN_ACCOUNT).await?
    else {
        return Err(AppError::authentication(
            "Connect Google in Settings first.",
        ));
    };
    if expires_at.is_some_and(|at| at > now_ms() + EXPIRY_MARGIN_MS) {
        return Ok(access_token);
    }
    let Some(refresh_token) = refresh_token else {
        return Err(AppError::authentication("Reconnect Google in Settings."));
    };
    let (client, _) = oauth_client(state).await?.ok_or_else(|| {
        AppError::configuration("The Google OAuth client is missing in Settings.")
    })?;
    match oauth::refresh(
        &state.google.http,
        &state.google.endpoints,
        &client,
        &refresh_token,
    )
    .await
    {
        Ok(tokens) => {
            let access = tokens.access_token.clone();
            store_tokens(state, tokens, Some(refresh_token)).await?;
            Ok(access)
        }
        Err(oauth::TokenFailure::InvalidGrant) => {
            // Keep the account visible but ask the user to sign in again.
            state.vault.delete(TOKEN_ACCOUNT).await?;
            state
                .db
                .call(|c| settings_repo::set_setting(c, KEY_NEEDS_RECONNECT, "1"))?;
            state.events.google_changed();
            Err(oauth::TokenFailure::InvalidGrant.into())
        }
        Err(oauth::TokenFailure::Other(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing, test_support::MockServer};

    fn id_token(email: &str) -> String {
        let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"email":"{email}"}}"#));
        format!("h.{payload}.s")
    }

    /// A mock Google that issues tokens and accepts refresh/revoke.
    async fn mock_google(refresh_status: u16) -> MockServer {
        let token = id_token("me@example.com");
        MockServer::start(move |req| match req.target.as_str() {
            "/token" if req.body.contains("grant_type=authorization_code") => Some((
                200,
                format!(
                    r#"{{"access_token":"at-1","expires_in":3600,"refresh_token":"rt-1",
                    "scope":"openid email {SCOPE_GMAIL_READONLY} {SCOPE_CALENDAR_EVENTS}","id_token":"{token}"}}"#
                ),
            )),
            "/token" if refresh_status == 200 => {
                Some((200, r#"{"access_token":"at-2","expires_in":3600}"#.into()))
            }
            "/token" => Some((400, r#"{"error":"invalid_grant"}"#.into())),
            "/revoke" => Some((200, "{}".into())),
            _ => None,
        })
        .await
    }

    async fn state_with(server: &MockServer) -> AppState {
        let (mut state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let base = &server.base_url;
        state.google = GoogleContext::new(GoogleEndpoints {
            auth: format!("{base}/auth"),
            token: format!("{base}/token"),
            revoke: format!("{base}/revoke"),
            gmail: format!("{base}/gmail/v1"),
            calendar: format!("{base}/calendar/v3"),
        });
        save_client(&state, "client-1", "client-secret")
            .await
            .unwrap();
        state
    }

    /// Plays the browser: follows the consent URL straight to the redirect.
    fn fake_browser(url: &str) -> AppResult<()> {
        let url = reqwest::Url::parse(url).unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        let redirect = format!("{}/?state={}&code=auth-code", q["redirect_uri"], q["state"]);
        tokio::spawn(async move {
            reqwest::get(redirect).await.unwrap();
        });
        Ok(())
    }

    async fn connected_state(server: &MockServer) -> AppState {
        let state = state_with(server).await;
        let status = connect(&state, fake_browser).await.unwrap();
        assert!(status.connected);
        state
    }

    #[tokio::test]
    async fn connects_with_pkce_and_stores_tokens_only_in_the_keychain() {
        let server = mock_google(200).await;
        let state = connected_state(&server).await;

        let status = status(&state).await.unwrap();
        assert_eq!(status.email.as_deref(), Some("me@example.com"));
        assert!(status.gmail.granted && status.calendar.granted);
        assert!(!status.connecting);

        // The token exchange carried the PKCE verifier and the client credentials.
        let exchange = server
            .requests()
            .into_iter()
            .find(|r| r.target == "/token")
            .unwrap();
        assert!(exchange.body.contains("code_verifier="));
        assert!(exchange.body.contains("code=auth-code"));
        assert!(exchange.body.contains("client_secret=client-secret"));

        // Tokens are in the credential store, never in SQLite.
        assert!(matches!(
            state.vault.get(TOKEN_ACCOUNT).await.unwrap(),
            Some(Credential::OAuth { .. })
        ));
        let dump = state
            .db
            .call(|c| {
                let mut stmt = c.prepare("SELECT key || '=' || value FROM settings")?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?.join(";"))
            })
            .unwrap();
        assert!(!dump.contains("at-1") && !dump.contains("rt-1"));

        // A valid token is reused without calling Google again.
        let calls = server.requests().len();
        assert_eq!(access_token(&state).await.unwrap(), "at-1");
        assert_eq!(server.requests().len(), calls);
    }

    #[tokio::test]
    async fn refreshes_expired_tokens_and_keeps_the_refresh_token() {
        let server = mock_google(200).await;
        let state = connected_state(&server).await;
        state
            .vault
            .set(
                TOKEN_ACCOUNT,
                Credential::OAuth {
                    access_token: "old".into(),
                    refresh_token: Some("rt-1".into()),
                    expires_at: Some(now_ms() - 1),
                },
            )
            .await
            .unwrap();

        assert_eq!(access_token(&state).await.unwrap(), "at-2");
        match state.vault.get(TOKEN_ACCOUNT).await.unwrap() {
            Some(Credential::OAuth { refresh_token, .. }) => {
                assert_eq!(refresh_token.as_deref(), Some("rt-1"))
            }
            other => panic!("unexpected credential {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_revoked_grant_asks_the_user_to_reconnect() {
        let server = mock_google(400).await;
        let state = connected_state(&server).await;
        state
            .vault
            .set(
                TOKEN_ACCOUNT,
                Credential::OAuth {
                    access_token: "old".into(),
                    refresh_token: Some("rt-1".into()),
                    expires_at: Some(now_ms() - 1),
                },
            )
            .await
            .unwrap();

        let error = access_token(&state).await.unwrap_err();
        assert!(matches!(error, AppError::Authentication(_)));
        let status = status(&state).await.unwrap();
        assert!(!status.connected && status.needs_reconnect);
        assert_eq!(status.email.as_deref(), Some("me@example.com"));
    }

    #[tokio::test]
    async fn disconnect_revokes_and_forgets_the_account() {
        let server = mock_google(200).await;
        let state = connected_state(&server).await;
        disconnect(&state).await.unwrap();

        let revoke = server
            .requests()
            .into_iter()
            .find(|r| r.target == "/revoke")
            .unwrap();
        assert!(revoke.body.contains("token=rt-1"));
        let status = status(&state).await.unwrap();
        assert!(!status.connected && status.email.is_none() && !status.gmail.granted);
        assert!(require(&state, GoogleService::Gmail).await.is_err());
    }

    #[tokio::test]
    async fn requests_only_the_scopes_of_enabled_services() {
        assert_eq!(
            scopes_for(true, false),
            ["openid", "email", SCOPE_GMAIL_READONLY]
        );
        assert_eq!(
            scopes_for(false, true),
            ["openid", "email", SCOPE_CALENDAR_EVENTS]
        );

        let server = mock_google(200).await;
        let state = state_with(&server).await;
        set_service_enabled(&state, GoogleService::Gmail, false).unwrap();
        set_service_enabled(&state, GoogleService::Calendar, false).unwrap();
        assert!(matches!(
            connect(&state, fake_browser).await.unwrap_err(),
            AppError::Validation(_)
        ));
    }

    #[tokio::test]
    async fn needs_an_oauth_client() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        if option_env!("REMA_GOOGLE_CLIENT_ID").is_none() {
            let error = connect(&state, |_| Ok(())).await.unwrap_err();
            assert!(matches!(error, AppError::Configuration(_)));
            assert!(status(&state).await.unwrap().client.is_none());
        }
    }
}
