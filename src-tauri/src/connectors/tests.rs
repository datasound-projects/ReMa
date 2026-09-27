//! Connector sign-in, tokens and lifecycle against a mock Google and
//! Microsoft: the real authorization code + PKCE flow through the loopback
//! redirect (a fake browser follows it), token exchange, refresh and
//! rotation, reauthentication, permissions, disconnect and revocation, the
//! old Google settings, and that tokens never leave the credential store.

use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Url;
use sha2::{Digest, Sha256};

use super::*;
use crate::{
    db::{jobs as jobs_repo, providers as settings_repo},
    events::RecordingEvents,
    llm::fake::FakeLanguageModel,
    models::jobs::ApplicationStatus,
    secrets::Credential,
    state::testing,
    test_support::{MockServer, Recorded},
};

const GOOGLE_CLIENT: &str = "google-client.apps.googleusercontent.com";
const GOOGLE_SECRET: &str = "google-installed-app-secret";
const MICROSOFT_CLIENT: &str = "00000000-1111-2222-3333-444444444444";
const LINKEDIN_CLIENT: &str = "86linkedin-test-client";

fn id_token(claims: serde_json::Value) -> String {
    format!("h.{}.s", URL_SAFE_NO_PAD.encode(claims.to_string()))
}

/// What the mock providers grant, whether refreshes work, and how the
/// connection-check endpoints answer.
struct Grants {
    google_scope: Mutex<String>,
    microsoft_scope: Mutex<String>,
    /// LinkedIn reports granted scopes comma-separated.
    linkedin_scope: Mutex<String>,
    refresh_ok: Mutex<bool>,
    /// The token endpoints cannot be reached (offline).
    token_down: Mutex<bool>,
    /// Overrides the Google code exchange (status, body).
    google_exchange: Mutex<Option<(u16, String)>>,
    gmail_profile: Mutex<(u16, String)>,
    outlook_messages: Mutex<(u16, String)>,
}

struct Providers {
    server: MockServer,
    grants: Arc<Grants>,
}

impl Providers {
    fn requests(&self, path: &str) -> Vec<Recorded> {
        self.server
            .requests()
            .into_iter()
            .filter(|r| r.target.starts_with(path))
            .collect()
    }

    fn grant_google(&self, scopes: &[&str]) {
        *self.grants.google_scope.lock().unwrap() = scopes.join(" ");
    }
}

async fn providers() -> Providers {
    let grants = Arc::new(Grants {
        google_scope: Mutex::new(google::scopes(&[ConnectorId::Gmail]).join(" ")),
        microsoft_scope: Mutex::new("Mail.Read User.Read openid profile email".into()),
        linkedin_scope: Mutex::new("email,openid,profile".into()),
        refresh_ok: Mutex::new(true),
        token_down: Mutex::new(false),
        google_exchange: Mutex::new(None),
        gmail_profile: Mutex::new((
            200,
            r#"{"emailAddress":"ana@gmail.com","messagesTotal":12,"threadsTotal":9,"historyId":"77"}"#
                .into(),
        )),
        outlook_messages: Mutex::new((200, r#"{"value":[{"id":"m1"}]}"#.into())),
    });
    let g = grants.clone();
    let server = MockServer::start(move |req| {
        let refresh_ok = *g.refresh_ok.lock().unwrap();
        let exchange = req.body.contains("grant_type=authorization_code");
        // Connection checks, by path.
        match req.target.split('?').next().unwrap_or_default() {
            "/gmail/v1/users/me/profile" => return Some(g.gmail_profile.lock().unwrap().clone()),
            "/calendar/v3/calendars/primary/events" => {
                return Some((200, r#"{"kind":"calendar#events"}"#.into()))
            }
            "/graph/v1.0/me" => {
                return Some((
                    200,
                    r#"{"id":"m-graph-id","displayName":"Ana Example","mail":null,"userPrincipalName":"ana@outlook.com"}"#
                        .into(),
                ))
            }
            "/graph/v1.0/me/messages" => return Some(g.outlook_messages.lock().unwrap().clone()),
            "/graph/v1.0/me/calendar" => return Some((200, r#"{"id":"cal-1"}"#.into())),
            _ => {}
        }
        if *g.token_down.lock().unwrap() && req.target.contains("token") {
            return Some((503, "Service Unavailable".into()));
        }
        if exchange && req.target == "/token" {
            if let Some(answer) = g.google_exchange.lock().unwrap().clone() {
                return Some(answer);
            }
        }
        match req.target.as_str() {
            "/token" if exchange => Some((
                200,
                serde_json::json!({
                    "access_token": "g-at-1", "expires_in": 3599, "refresh_token": "g-rt-1",
                    "scope": *g.google_scope.lock().unwrap(), "token_type": "Bearer",
                    "id_token": id_token(serde_json::json!({
                        "sub": "g-123", "email": "ana@gmail.com", "name": "Ana Example"
                    })),
                })
                .to_string(),
            )),
            "/token" if refresh_ok => Some((
                200,
                r#"{"access_token":"g-at-2","expires_in":3599,"token_type":"Bearer"}"#.into(),
            )),
            "/token" => Some((
                400,
                r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#
                    .into(),
            )),
            "/revoke" => Some((200, "{}".into())),
            "/common/oauth2/v2.0/token" if exchange => Some((
                200,
                serde_json::json!({
                    "access_token": "m-at-1", "expires_in": 3599, "refresh_token": "m-rt-1",
                    "scope": *g.microsoft_scope.lock().unwrap(), "token_type": "Bearer",
                    "id_token": id_token(serde_json::json!({
                        "oid": "m-oid", "preferred_username": "ana@outlook.com", "name": "Ana Example"
                    })),
                })
                .to_string(),
            )),
            // Microsoft rotates refresh tokens.
            "/common/oauth2/v2.0/token" if refresh_ok => Some((
                200,
                r#"{"access_token":"m-at-2","refresh_token":"m-rt-2","expires_in":3599}"#.into(),
            )),
            "/common/oauth2/v2.0/token" => Some((
                400,
                r#"{"error":"invalid_grant","error_description":"AADSTS70008: expired"}"#.into(),
            )),
            // LinkedIn: no refresh token for this kind of app; identity
            // through OpenID Connect.
            "/linkedin/oauth/v2/accessToken" if exchange => Some((
                200,
                serde_json::json!({
                    "access_token": "li-at-1", "expires_in": 5_183_999,
                    "scope": *g.linkedin_scope.lock().unwrap(), "token_type": "Bearer",
                    "id_token": id_token(serde_json::json!({
                        "sub": "782bbtaQ", "email": "ana@example.com"
                    })),
                })
                .to_string(),
            )),
            "/linkedin/v2/userinfo" => Some((
                200,
                serde_json::json!({
                    "sub": "782bbtaQ", "name": "Ana Example", "given_name": "Ana",
                    "family_name": "Example", "email": "ana@example.com", "email_verified": true
                })
                .to_string(),
            )),
            _ => None,
        }
    })
    .await;
    Providers { server, grants }
}

fn state_for(providers: &Providers, apps: Apps) -> (AppState, Arc<RecordingEvents>) {
    let (mut state, events) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
    let base = &providers.server.base_url;
    state.connectors = ConnectorsContext::new(
        GoogleEndpoints::at(base),
        MicrosoftEndpoints::at(base, &format!("{base}/graph/v1.0")),
        apps,
    )
    .with_linkedin(linkedin::LinkedinEndpoints::at(base));
    (state, events)
}

fn apps() -> Apps {
    Apps {
        google: Some(OAuthApp {
            client_id: GOOGLE_CLIENT.into(),
            client_secret: Some(GOOGLE_SECRET.into()),
        }),
        microsoft: Some(OAuthApp {
            client_id: MICROSOFT_CLIENT.into(),
            client_secret: None,
        }),
        linkedin: Some(OAuthApp {
            client_id: LINKEDIN_CLIENT.into(),
            client_secret: None,
        }),
    }
}

/// Plays the system browser: the user signs in and consents, and the
/// provider redirects to ReMa's loopback address.
#[derive(Clone, Default)]
struct Browser {
    opened: Arc<Mutex<Vec<String>>>,
    page: Arc<Mutex<Option<String>>>,
}

enum Consent {
    Allow,
    Deny,
    WrongState,
    /// The provider returns this `error` and `error_description`.
    Error(&'static str, &'static str),
}

impl Browser {
    fn open(&self, consent: Consent) -> impl FnOnce(&str) -> AppResult<()> + '_ {
        move |url: &str| {
            self.opened.lock().unwrap().push(url.to_string());
            let query = params(url);
            let redirect = query["redirect_uri"].clone();
            let state = query["state"].clone();
            let target = match consent {
                Consent::Allow => format!("{redirect}/?state={state}&code=auth-code-123"),
                Consent::Deny => format!("{redirect}/?state={state}&error=access_denied"),
                Consent::WrongState => format!("{redirect}/?state=forged&code=auth-code-123"),
                Consent::Error(error, description) => format!(
                    "{redirect}/?state={state}&error={error}&error_description={}",
                    description.replace(' ', "+")
                ),
            };
            let page = self.page.clone();
            tokio::spawn(async move {
                if let Ok(response) = reqwest::get(target).await {
                    *page.lock().unwrap() = response.text().await.ok();
                }
            });
            Ok(())
        }
    }

    fn last_url(&self) -> String {
        self.opened.lock().unwrap().last().cloned().unwrap()
    }
}

fn params(url: &str) -> std::collections::HashMap<String, String> {
    Url::parse(url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

fn form(body: &str) -> std::collections::HashMap<String, String> {
    Url::parse(&format!("http://form.invalid/?{body}"))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

async fn card(state: &AppState, id: ConnectorId) -> ConnectorStatus {
    overview(state)
        .await
        .unwrap()
        .connectors
        .into_iter()
        .find(|c| c.id == id)
        .unwrap()
}

/// Every value in every table, for "no secret is stored in SQLite".
fn database_text(state: &AppState) -> String {
    state
        .db
        .call(|c| {
            let tables: Vec<String> = c
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            let mut out = String::new();
            for table in tables {
                let mut statement = c.prepare(&format!("SELECT * FROM \"{table}\""))?;
                let columns = statement.column_count();
                let mut rows = statement.query([])?;
                while let Some(row) = rows.next()? {
                    for i in 0..columns {
                        let value: rusqlite::types::Value = row.get(i)?;
                        out.push_str(&format!("{value:?}\n"));
                    }
                }
            }
            Ok(out)
        })
        .unwrap()
}

async fn stored(state: &AppState, provider: ProviderId) -> Option<Credential> {
    tokens::grant(state, provider).await
}

/// Lets the account's access token expire: the one in memory, and for
/// LinkedIn (whose access token is its grant) the stored one.
async fn expire(state: &AppState, provider: ProviderId) {
    let grant = stored(state, provider).await.expect("connected");
    state.connectors.forget_provider_access(provider);
    if let Credential::OAuth {
        access_token,
        refresh_token,
        ..
    } = grant
    {
        let account = state
            .db
            .call(|c| repo::account(c, provider))
            .unwrap()
            .and_then(|a| a.account_id)
            .unwrap();
        state
            .vault
            .set(
                &tokens::vault_key(provider, &account),
                Credential::OAuth {
                    access_token,
                    refresh_token,
                    expires_at: Some(now_ms() - 1_000),
                },
            )
            .await
            .unwrap();
    }
}

/// Waits for the fake browser's tab to show a page.
async fn page_shown(browser: &Browser) -> String {
    for _ in 0..200 {
        if let Some(page) = browser.page.lock().unwrap().clone() {
            return page;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the browser tab never got a page");
}

// ── Sign-in ─────────────────────────────────────────────────────────

#[tokio::test]
async fn gmail_signs_in_with_pkce_through_the_loopback_redirect() {
    let providers = providers().await;
    let (state, events) = state_for(&providers, apps());
    let browser = Browser::default();
    connect(&state, ConnectorId::Gmail, browser.open(Consent::Allow))
        .await
        .unwrap();

    // The authorization request.
    let url = browser.last_url();
    assert!(url.starts_with(&format!("{}/o/oauth2/v2/auth?", providers.server.base_url)));
    let query = params(&url);
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["client_id"], GOOGLE_CLIENT);
    assert!(query["redirect_uri"].starts_with("http://127.0.0.1:"));
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["access_type"], "offline");
    assert!(query["prompt"].contains("select_account"));
    assert!(query["state"].len() >= 32, "random state");
    assert_eq!(
        query["scope"], "openid email profile https://www.googleapis.com/auth/gmail.readonly",
        "Gmail only: no calendar, no send or modify scope"
    );

    // The code exchange proves possession of the PKCE verifier.
    let exchange = form(&providers.requests("/token")[0].body);
    assert_eq!(exchange["grant_type"], "authorization_code");
    assert_eq!(exchange["code"], "auth-code-123");
    assert_eq!(exchange["redirect_uri"], query["redirect_uri"]);
    let verifier = &exchange["code_verifier"];
    assert!((43..=128).contains(&verifier.len()));
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        query["code_challenge"]
    );

    // The browser shows the success page, once ReMa is really connected.
    let page = page_shown(&browser).await;
    assert!(page.contains("ReMa connected successfully."));
    assert!(page.contains("You can close this browser tab and return to ReMa."));
    for secret in ["auth-code-123", "g-at-1", "g-rt-1", query["state"].as_str()] {
        assert!(!page.contains(secret), "{secret} shown in the browser");
    }
    // Checked with one small request: the Gmail profile, no message.
    let checks = providers.requests("/gmail/");
    assert_eq!(checks.len(), 1);
    assert!(checks[0].target.starts_with("/gmail/v1/users/me/profile"));

    let gmail = card(&state, ConnectorId::Gmail).await;
    assert_eq!(gmail.state, ConnectorState::Connected);
    assert_eq!(gmail.account_email.as_deref(), Some("ana@gmail.com"));
    assert_eq!(gmail.publisher, "Google");
    assert_eq!(
        gmail.description,
        "Read job-related emails and track application updates."
    );
    assert!(gmail.permissions.iter().all(|p| p.granted));
    // Connecting reads no mail: only "Job Mail & Interview Sync" does.
    assert_eq!(gmail.last_sync_started_at, None);
    assert!(!state.connectors.is_syncing(ConnectorId::Gmail));
    assert_eq!(
        card(&state, ConnectorId::GoogleCalendar).await.state,
        ConnectorState::Disconnected
    );
    assert!(*events.connectors.lock().unwrap() > 0);
    assert_eq!(
        require(&state, ConnectorId::Gmail)
            .await
            .unwrap()
            .email
            .as_deref(),
        Some("ana@gmail.com")
    );
}

#[tokio::test]
async fn tokens_live_only_in_the_credential_store() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let browser = Browser::default();
    connect(&state, ConnectorId::Gmail, browser.open(Consent::Allow))
        .await
        .unwrap();
    let verifier = form(&providers.requests("/token")[0].body)["code_verifier"].clone();

    // Only the refresh token is kept, under the account's own key; the
    // access token stays in memory.
    assert!(matches!(
        stored(&state, ProviderId::Google).await,
        Some(Credential::RefreshToken { ref refresh_token }) if refresh_token == "g-rt-1"
    ));
    let key = tokens::vault_key(ProviderId::Google, "g-123");
    let raw = state.vault.get_text(&key).await.unwrap().unwrap();
    assert!(raw.contains("g-rt-1") && !raw.contains("g-at-1"), "{raw}");
    assert!(state
        .vault
        .get_text(&tokens::legacy_key(ProviderId::Google))
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        tokens::get_valid_access_token(&state, ProviderId::Google)
            .await
            .unwrap(),
        "g-at-1"
    );
    let secrets = [
        "g-at-1",
        "g-rt-1",
        "auth-code-123",
        verifier.as_str(),
        GOOGLE_SECRET,
    ];
    // A refresh, so a refresh request is logged too.
    state.connectors.forget_provider_access(ProviderId::Google);
    tokens::get_valid_access_token(&state, ProviderId::Google)
        .await
        .unwrap();
    let database = database_text(&state);
    let overview = serde_json::to_string(&overview(&state).await.unwrap()).unwrap();
    let debug = format!(
        "{:?} {:?} {:?}",
        stored(&state, ProviderId::Google).await,
        state.connectors.app(ProviderId::Google),
        Pkce::from_verifier(verifier.clone())
    );
    let logs = diag_lines().lock().unwrap().join("\n");
    assert!(logs.contains("[oauth] provider=google phase=token_exchange_success"));
    assert!(logs.contains("[oauth] provider=google phase=token_refreshed"));
    for secret in secrets.into_iter().chain(["g-at-2"]) {
        assert!(!database.contains(secret), "{secret} in SQLite");
        assert!(!overview.contains(secret), "{secret} sent to the interface");
        assert!(!debug.contains(secret), "{secret} in debug output");
        assert!(!logs.contains(secret), "{secret} logged");
    }
    // Diagnostics name no account either.
    assert!(!logs.contains("ana@gmail.com") && !logs.contains("g-123"));
}

#[tokio::test]
async fn adding_a_second_google_connector_asks_for_the_union_of_scopes() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let browser = Browser::default();
    connect(&state, ConnectorId::Gmail, browser.open(Consent::Allow))
        .await
        .unwrap();
    providers.grant_google(
        &google::scopes(&[ConnectorId::Gmail, ConnectorId::GoogleCalendar])
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    connect(
        &state,
        ConnectorId::GoogleCalendar,
        browser.open(Consent::Allow),
    )
    .await
    .unwrap();
    let scope = params(&browser.last_url())["scope"].clone();
    for wanted in [
        google::SCOPE_GMAIL_READONLY,
        google::SCOPE_CALENDAR_EVENTS,
        google::SCOPE_CALENDAR_FREEBUSY,
    ] {
        assert!(scope.split(' ').any(|s| s == wanted), "{wanted} in {scope}");
    }
    for id in [ConnectorId::Gmail, ConnectorId::GoogleCalendar] {
        let connector = card(&state, id).await;
        assert_eq!(connector.state, ConnectorState::Connected);
        // Connecting (or adding a connector to the account) reads nothing
        // and claims no sync.
        assert_eq!(connector.last_sync_at, None, "{id:?}");
    }
    // Checks only: no message and no event was read.
    assert!(providers.requests("/gmail/v1/users/me/messages").is_empty());
    assert!(providers.requests("/gmail/v1/users/me/history").is_empty());
    let events = providers.requests("/calendar/v3/calendars/primary/events");
    assert_eq!(events.len(), 1);
    assert!(events[0].target.contains("maxResults=1") && events[0].target.contains("fields=kind"));
}

#[tokio::test]
async fn a_permission_left_unchecked_is_reported() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let browser = Browser::default();
    // The user unticks the calendar permission on Google's consent screen.
    providers.grant_google(&["openid", "email", "profile", google::SCOPE_GMAIL_READONLY]);
    let error = connect(
        &state,
        ConnectorId::GoogleCalendar,
        browser.open(Consent::Allow),
    )
    .await
    .unwrap_err();
    // Not a generic OAuth failure: the sign-in worked, a permission is
    // missing (Spec B §47).
    assert!(matches!(error, AppError::Permission(_)), "{error}");
    assert!(page_shown(&browser)
        .await
        .contains("ReMa could not complete authorization."));
    let calendar = card(&state, ConnectorId::GoogleCalendar).await;
    assert_eq!(calendar.state, ConnectorState::PermissionMissing);
    assert_eq!(
        calendar.error_code,
        Some(ConnectorErrorCode::ScopeNotGranted)
    );
    assert!(calendar.message.unwrap().contains("Calendar — Read events"));
    // No request was needed to know it.
    assert!(providers.requests("/calendar/").is_empty());
    let error = require(&state, ConnectorId::GoogleCalendar)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not have permission"));
    assert!(ready(&state, ConnectorKind::Calendar).await.is_empty());
}

#[tokio::test]
async fn denied_consent_and_a_forged_state_connect_nothing() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let browser = Browser::default();

    let error = connect(&state, ConnectorId::Gmail, browser.open(Consent::Deny))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not granted"), "{error}");

    // Declining is the user's choice: the card simply offers `+` again.
    assert_eq!(
        card(&state, ConnectorId::Gmail).await.state,
        ConnectorState::Disconnected
    );

    let error = connect(
        &state,
        ConnectorId::Gmail,
        browser.open(Consent::WrongState),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("did not start"), "{error}");

    assert!(
        providers.requests("/token").is_empty(),
        "no code was exchanged"
    );
    assert!(stored(&state, ProviderId::Google).await.is_none());
    let gmail = card(&state, ConnectorId::Gmail).await;
    assert_eq!(gmail.state, ConnectorState::Error, "Retry is offered");
    assert_eq!(gmail.error_code, Some(ConnectorErrorCode::InvalidState));
    assert!(!gmail.enabled);
    assert!(page_shown(&browser)
        .await
        .contains("ReMa could not complete authorization."));
}

#[tokio::test]
async fn a_sign_in_shows_connecting_and_can_be_cancelled() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let observer = state.clone();
    let seen = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let during = card(&observer, ConnectorId::Gmail).await.state;
        observer.connectors.cancel_sign_in(ProviderId::Google);
        during
    });
    // The user never finishes signing in.
    let error = connect(&state, ConnectorId::Gmail, |_| Ok(()))
        .await
        .unwrap_err();
    assert_eq!(seen.await.unwrap(), ConnectorState::Connecting);
    assert!(!error.to_string().is_empty());
    assert_eq!(
        card(&state, ConnectorId::Gmail).await.state,
        ConnectorState::Disconnected
    );
}

#[tokio::test]
async fn an_unanswered_sign_in_times_out_and_a_late_callback_finds_nothing() {
    let loopback = Loopback::ipv4().await.unwrap();
    let port = loopback.port();
    let failure = match oauth::wait_for_callback(
        loopback,
        "state",
        "Google",
        &CancellationToken::new(),
        Duration::from_millis(100),
    )
    .await
    {
        Err(failure) => failure,
        Ok(_) => panic!("nothing called back"),
    };
    assert_eq!(failure.code, ConnectorErrorCode::SignInTimedOut);
    assert!(failure.message.contains("Retry"));
    // The listener is gone: the browser's late redirect is refused.
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err());
}

#[tokio::test]
async fn builds_without_an_app_registration_offer_no_sign_in() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, Apps::default());
    let opened = Arc::new(Mutex::new(false));
    let flag = opened.clone();
    let error = connect(&state, ConnectorId::OutlookMail, move |_| {
        *flag.lock().unwrap() = true;
        Ok(())
    })
    .await
    .unwrap_err();
    assert!(matches!(error, AppError::Configuration(_)));
    assert!(!*opened.lock().unwrap(), "no browser was opened");
    for connector in overview(&state).await.unwrap().connectors {
        assert_eq!(connector.state, ConnectorState::Unavailable);
    }
}

#[tokio::test]
async fn outlook_signs_in_as_a_public_client_on_localhost() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let browser = Browser::default();
    connect(
        &state,
        ConnectorId::OutlookMail,
        browser.open(Consent::Allow),
    )
    .await
    .unwrap();

    let url = browser.last_url();
    assert!(url.starts_with(&format!(
        "{}/common/oauth2/v2.0/authorize?",
        providers.server.base_url
    )));
    let query = params(&url);
    assert!(query["redirect_uri"].starts_with("http://localhost:"));
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["prompt"], "select_account");
    let scopes: Vec<&str> = query["scope"].split(' ').collect();
    for wanted in [
        "openid",
        "profile",
        "email",
        "offline_access",
        "User.Read",
        "Mail.Read",
    ] {
        assert!(scopes.contains(&wanted), "{wanted}");
    }
    assert!(!scopes.contains(&"Calendars.ReadWrite"));
    assert!(!scopes
        .iter()
        .any(|s| s.contains("Send") || s.contains("ReadWrite")));

    let exchange = form(&providers.requests("/common/oauth2/v2.0/token")[0].body);
    assert_eq!(exchange["client_id"], MICROSOFT_CLIENT);
    assert!(!exchange.contains_key("client_secret"), "public client");
    assert!(exchange["scope"].contains("Mail.Read"));

    let outlook = card(&state, ConnectorId::OutlookMail).await;
    assert_eq!(outlook.state, ConnectorState::Connected);
    assert_eq!(outlook.account_email.as_deref(), Some("ana@outlook.com"));
    assert_eq!(outlook.publisher, "Microsoft");
    // Identity from Graph /me, then one small mail check.
    assert_eq!(providers.requests("/graph/v1.0/me?").len(), 1);
    let mail = providers.requests("/graph/v1.0/me/messages");
    assert_eq!(mail.len(), 1);
    assert!(mail[0].target.contains("%24top=1") || mail[0].target.contains("$top=1"));
    // The grant is keyed by the account's Microsoft id.
    assert!(state
        .vault
        .get_text(&tokens::vault_key(ProviderId::Microsoft, "m-oid"))
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn linkedin_signs_in_with_openid_connect_and_grants_identity_only() {
    use crate::network::capabilities::{self, NetworkCapability, ProviderAccess};
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let browser = Browser::default();
    connect(&state, ConnectorId::Linkedin, browser.open(Consent::Allow))
        .await
        .unwrap();

    // The official authorization endpoint, identity scopes, PKCE, the
    // loopback address: nothing asks for a password or cookies.
    let url = browser.last_url();
    assert!(url.starts_with(&format!(
        "{}/linkedin/oauth/v2/authorization?",
        providers.server.base_url
    )));
    let query = params(&url);
    assert_eq!(query["client_id"], LINKEDIN_CLIENT);
    assert_eq!(query["scope"], "openid profile email");
    assert_eq!(query["code_challenge_method"], "S256");
    assert!(query["redirect_uri"].starts_with("http://127.0.0.1:"));
    assert!(query["state"].len() >= 32);
    for asked in ["password", "cookie", "li_at", "username"] {
        assert!(!url.to_lowercase().contains(asked), "{asked}");
    }
    // A native client: the verifier proves the sign-in, no secret ships.
    let exchange = form(&providers.requests("/linkedin/oauth/v2/accessToken")[0].body);
    assert!(!exchange.contains_key("client_secret"));
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(exchange["code_verifier"].as_bytes())),
        query["code_challenge"]
    );

    // Connected as the member (name from the userinfo endpoint).
    let linkedin = card(&state, ConnectorId::Linkedin).await;
    assert_eq!(linkedin.state, ConnectorState::Connected);
    assert_eq!(linkedin.account_name.as_deref(), Some("Ana Example"));
    assert_eq!(linkedin.account_email.as_deref(), Some("ana@example.com"));
    let granted: Vec<(Capability, bool)> = linkedin
        .permissions
        .iter()
        .map(|p| (p.capability, p.granted))
        .collect();
    assert_eq!(
        granted,
        [
            (Capability::NetworkIdentity, true),
            (Capability::NetworkProfile, true),
            (Capability::NetworkConnections, false),
        ]
    );
    // Identity is not search or network access.
    let caps = capabilities::all(&state).await.unwrap();
    let li = caps
        .iter()
        .find(|c| c.provider == ProviderId::Linkedin)
        .unwrap();
    assert_eq!(li.access, ProviderAccess::Connected);
    assert!(li.has(NetworkCapability::AuthenticateIdentity));
    assert!(!li.has(NetworkCapability::ReadFirstDegreeConnections));
    assert!(!li.has(NetworkCapability::SearchPeople));
    assert!(li
        .summary
        .contains("does not currently have permission to read your connection list"));

    // The token lives only in the credential store.
    assert!(matches!(
        stored(&state, ProviderId::Linkedin).await,
        Some(Credential::OAuth { ref access_token, refresh_token: None, .. })
            if access_token == "li-at-1"
    ));
    let database = database_text(&state);
    let interface = format!(
        "{} {}",
        serde_json::to_string(&overview(&state).await.unwrap()).unwrap(),
        serde_json::to_string(&caps).unwrap()
    );
    for secret in [
        "li-at-1",
        "auth-code-123",
        exchange["code_verifier"].as_str(),
    ] {
        assert!(!database.contains(secret), "{secret} in SQLite");
        assert!(
            !interface.contains(secret),
            "{secret} sent to the interface"
        );
    }
}

#[tokio::test]
async fn linkedin_scopes_approved_for_the_app_are_used_only_when_granted() {
    use crate::network::capabilities::{self, NetworkCapability};
    let providers = providers().await;
    *providers.grants.linkedin_scope.lock().unwrap() =
        "email,openid,profile,r_1st_connections".into();
    let (state, _) = state_for(&providers, apps());
    connect(
        &state,
        ConnectorId::Linkedin,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    let caps = capabilities::all(&state).await.unwrap();
    let li = caps
        .iter()
        .find(|c| c.provider == ProviderId::Linkedin)
        .unwrap();
    assert!(li.has(NetworkCapability::ReadFirstDegreeConnections));
    assert!(!li.has(NetworkCapability::ReadSecondDegreeConnections));
    assert!(card(&state, ConnectorId::Linkedin)
        .await
        .permissions
        .iter()
        .all(|p| p.granted));
}

#[tokio::test]
async fn an_expired_linkedin_sign_in_asks_to_reconnect_and_never_opens_one() {
    use crate::network::capabilities::{self, ProviderAccess};
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    connect(
        &state,
        ConnectorId::Linkedin,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    expire(&state, ProviderId::Linkedin).await;
    // Seen as expired before any request.
    assert_eq!(
        card(&state, ConnectorId::Linkedin).await.state,
        ConnectorState::ReauthRequired
    );
    let error = tokens::get_valid_access_token(&state, ProviderId::Linkedin)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Reconnect"), "{error}");
    // Nothing to refresh with: no token request, the token is forgotten.
    assert_eq!(
        providers.requests("/linkedin/oauth/v2/accessToken").len(),
        1
    );
    assert!(stored(&state, ProviderId::Linkedin).await.is_none());
    let caps = capabilities::all(&state).await.unwrap();
    let li = caps
        .iter()
        .find(|c| c.provider == ProviderId::Linkedin)
        .unwrap();
    assert_eq!(li.access, ProviderAccess::ReconnectNeeded);
    assert!(li.available.is_empty());

    // Disconnecting removes the account and the tokens.
    disconnect(&state, ConnectorId::Linkedin).await.unwrap();
    assert_eq!(
        card(&state, ConnectorId::Linkedin).await.state,
        ConnectorState::Disconnected
    );
    assert!(state
        .db
        .call(|c| repo::account(c, ProviderId::Linkedin))
        .unwrap()
        .is_none());
    let caps = capabilities::all(&state).await.unwrap();
    assert_eq!(
        caps.iter()
            .find(|c| c.provider == ProviderId::Linkedin)
            .unwrap()
            .access,
        ProviderAccess::NotConnected
    );
}

#[tokio::test]
async fn linkedin_cancel_and_forged_state_connect_nothing() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let browser = Browser::default();
    assert!(
        connect(&state, ConnectorId::Linkedin, browser.open(Consent::Deny))
            .await
            .is_err()
    );
    assert!(connect(
        &state,
        ConnectorId::Linkedin,
        browser.open(Consent::WrongState)
    )
    .await
    .is_err());
    assert!(providers
        .requests("/linkedin/oauth/v2/accessToken")
        .is_empty());
    // The forged answer is reported on the card, with Retry.
    let linkedin = card(&state, ConnectorId::Linkedin).await;
    assert_eq!(linkedin.state, ConnectorState::Error);
    assert_eq!(linkedin.error_code, Some(ConnectorErrorCode::InvalidState));
    assert!(!linkedin.enabled);
    assert!(stored(&state, ProviderId::Linkedin).await.is_none());
}

#[tokio::test]
async fn xing_is_shown_as_not_available_and_opens_no_sign_in() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let opened = Arc::new(Mutex::new(false));
    let flag = opened.clone();
    let error = connect(&state, ConnectorId::Xing, move |_| {
        *flag.lock().unwrap() = true;
        Ok(())
    })
    .await
    .unwrap_err();
    assert!(!*opened.lock().unwrap(), "no browser was opened");
    assert!(error.to_string().contains("XING"), "{error}");
    let xing = card(&state, ConnectorId::Xing).await;
    assert_eq!(xing.state, ConnectorState::Unavailable);
    assert!(xing
        .message
        .unwrap()
        .contains("no sign-in for desktop apps"));
}

// ── Token manager ───────────────────────────────────────────────────

#[tokio::test]
async fn access_tokens_are_refreshed_silently_before_they_expire() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    connect(
        &state,
        ConnectorId::Gmail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    assert_eq!(
        tokens::get_valid_access_token(&state, ProviderId::Google)
            .await
            .unwrap(),
        "g-at-1",
        "still valid: no refresh"
    );
    assert_eq!(providers.requests("/token").len(), 1);

    expire(&state, ProviderId::Google).await;
    assert_eq!(
        tokens::get_valid_access_token(&state, ProviderId::Google)
            .await
            .unwrap(),
        "g-at-2"
    );
    let refresh = form(&providers.requests("/token")[1].body);
    assert_eq!(refresh["grant_type"], "refresh_token");
    assert_eq!(refresh["refresh_token"], "g-rt-1");
    // Google kept the refresh token (none was returned).
    assert!(matches!(
        stored(&state, ProviderId::Google).await,
        Some(Credential::RefreshToken { ref refresh_token }) if refresh_token == "g-rt-1"
    ));
    // Forced refresh (after a 401).
    tokens::refresh_access_token(&state, ProviderId::Google)
        .await
        .unwrap();
    assert_eq!(providers.requests("/token").len(), 3);
}

#[tokio::test]
async fn microsoft_refresh_tokens_rotate() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    connect(
        &state,
        ConnectorId::OutlookMail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    expire(&state, ProviderId::Microsoft).await;
    assert_eq!(
        tokens::get_valid_access_token(&state, ProviderId::Microsoft)
            .await
            .unwrap(),
        "m-at-2"
    );
    let refresh = form(&providers.requests("/common/oauth2/v2.0/token")[1].body);
    assert_eq!(refresh["refresh_token"], "m-rt-1");
    assert!(refresh["scope"].split(' ').any(|s| s == "offline_access"));
    assert!(!refresh.contains_key("client_secret"));
    assert!(matches!(
        stored(&state, ProviderId::Microsoft).await,
        Some(Credential::RefreshToken { ref refresh_token }) if refresh_token == "m-rt-2"
    ));
}

#[tokio::test]
async fn a_revoked_grant_requires_reconnecting_and_never_retries_on_its_own() {
    let providers = providers().await;
    let (state, events) = state_for(&providers, apps());
    connect(
        &state,
        ConnectorId::Gmail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    *providers.grants.refresh_ok.lock().unwrap() = false;
    expire(&state, ProviderId::Google).await;

    let error = tokens::get_valid_access_token(&state, ProviderId::Google)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Authentication(_)));
    assert!(
        stored(&state, ProviderId::Google).await.is_none(),
        "tokens deleted"
    );
    assert!(
        tokens::requires_reauthentication(&state, ProviderId::Google)
            .await
            .unwrap()
    );
    let gmail = card(&state, ConnectorId::Gmail).await;
    assert_eq!(gmail.state, ConnectorState::ReauthRequired);
    assert_eq!(gmail.error_code, Some(ConnectorErrorCode::ReauthRequired));
    assert!(gmail.message.unwrap().starts_with("Reconnect required"));
    assert_eq!(
        gmail.account_email.as_deref(),
        Some("ana@gmail.com"),
        "the account stays visible"
    );
    assert!(require(&state, ConnectorId::Gmail).await.is_err());

    // Later calls fail fast: no token request, no browser, one notification.
    let requests = providers.requests("/token").len();
    for _ in 0..3 {
        assert!(tokens::get_valid_access_token(&state, ProviderId::Google)
            .await
            .is_err());
    }
    assert_eq!(providers.requests("/token").len(), requests);
    let shown = events.shown.lock().unwrap().clone();
    assert_eq!(
        shown
            .iter()
            .filter(|(t, _)| t == "Reconnect Google")
            .count(),
        1
    );

    // Reconnect fixes it.
    *providers.grants.refresh_ok.lock().unwrap() = true;
    connect(
        &state,
        ConnectorId::Gmail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    assert_eq!(
        card(&state, ConnectorId::Gmail).await.state,
        ConnectorState::Connected
    );
}

// ── Disconnect ──────────────────────────────────────────────────────

#[tokio::test]
async fn disconnecting_revokes_the_grant_once_the_last_connector_is_removed() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let browser = Browser::default();
    providers.grant_google(
        &google::scopes(&[ConnectorId::Gmail, ConnectorId::GoogleCalendar])
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    connect(&state, ConnectorId::Gmail, browser.open(Consent::Allow))
        .await
        .unwrap();
    connect(
        &state,
        ConnectorId::GoogleCalendar,
        browser.open(Consent::Allow),
    )
    .await
    .unwrap();
    let now = now_ms();
    let application = state
        .db
        .call(|c| {
            repo::save_cursor(c, ProviderId::Google, "g-123", "mail:inbox", "42", now)?;
            jobs_repo::insert_application(
                c,
                &jobs_repo::ApplicationRecord::new("Acme", ApplicationStatus::Confirmed, now),
            )
        })
        .unwrap();

    // Gmail goes; Google Calendar still needs the account.
    disconnect(&state, ConnectorId::Gmail).await.unwrap();
    assert!(providers.requests("/revoke").is_empty());
    assert!(stored(&state, ProviderId::Google).await.is_some());
    assert_eq!(
        card(&state, ConnectorId::Gmail).await.state,
        ConnectorState::Disconnected
    );
    assert_eq!(
        card(&state, ConnectorId::GoogleCalendar).await.state,
        ConnectorState::Connected
    );
    assert_eq!(
        state
            .db
            .call(|c| repo::cursor(c, ProviderId::Google, "g-123", "mail:inbox"))
            .unwrap(),
        None,
        "mail sync state removed"
    );

    // The last Google connector: revoke at Google and forget the account.
    disconnect(&state, ConnectorId::GoogleCalendar)
        .await
        .unwrap();
    let revoke = providers.requests("/revoke");
    assert_eq!(revoke.len(), 1);
    assert_eq!(form(&revoke[0].body)["token"], "g-rt-1");
    assert!(stored(&state, ProviderId::Google).await.is_none());
    assert!(state
        .db
        .call(|c| repo::account(c, ProviderId::Google))
        .unwrap()
        .is_none());
    // Application history is kept.
    assert_eq!(
        state
            .db
            .call(|c| jobs_repo::get_application(c, application))
            .unwrap()
            .company,
        "Acme"
    );
}

#[tokio::test]
async fn microsoft_disconnect_deletes_local_tokens() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    connect(
        &state,
        ConnectorId::OutlookMail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    disconnect(&state, ConnectorId::OutlookMail).await.unwrap();
    assert!(stored(&state, ProviderId::Microsoft).await.is_none());
    assert_eq!(
        card(&state, ConnectorId::OutlookMail).await.state,
        ConnectorState::Disconnected
    );
}

// ── Sync slots ──────────────────────────────────────────────────────

#[test]
fn at_most_one_sync_runs_per_connector() {
    let ctx = ConnectorsContext::new(
        GoogleEndpoints::default(),
        MicrosoftEndpoints::default(),
        Apps::default(),
    );
    let first = sync::begin(&ctx, ConnectorId::Gmail).unwrap();
    assert!(sync::begin(&ctx, ConnectorId::Gmail).is_none());
    assert!(sync::begin(&ctx, ConnectorId::OutlookMail).is_some());
    assert!(ctx.is_syncing(ConnectorId::Gmail));
    ctx.shutdown();
    assert!(first.cancel.is_cancelled(), "app exit stops running syncs");
    drop(first);
    assert!(!ctx.is_syncing(ConnectorId::Gmail));
}

// ── The old Google Workspace settings ───────────────────────────────

async fn old_google(state: &AppState, custom_client: bool) {
    state
        .db
        .call(|c| {
            if custom_client {
                settings_repo::set_setting(c, "google.client_id", "user-client")?;
            }
            settings_repo::set_setting(c, "google.email", "ana@gmail.com")?;
            settings_repo::set_setting(
                c,
                "google.scopes",
                &format!(
                    "openid email {} {}",
                    google::SCOPE_GMAIL_READONLY,
                    google::SCOPE_CALENDAR_EVENTS
                ),
            )
        })
        .unwrap();
    state
        .vault
        .set(
            "google",
            Credential::OAuth {
                access_token: "old-at".into(),
                refresh_token: Some("old-rt".into()),
                expires_at: Some(now_ms() + 3_600_000),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn old_google_tokens_from_the_built_in_client_keep_working() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    old_google(&state, false).await;
    legacy::migrate(&state).await.unwrap();

    // The old grant keeps working: its access token is used until it
    // expires, with no refresh.
    assert!(stored(&state, ProviderId::Google).await.is_some());
    assert_eq!(
        tokens::get_valid_access_token(&state, ProviderId::Google)
            .await
            .unwrap(),
        "old-at"
    );
    assert!(providers.requests("/token").is_empty());
    assert!(state.vault.get("google").await.unwrap().is_none());
    for id in [ConnectorId::Gmail, ConnectorId::GoogleCalendar] {
        let status = card(&state, id).await;
        assert_eq!(status.state, ConnectorState::Connected, "{id:?}");
        assert_eq!(status.account_email.as_deref(), Some("ana@gmail.com"));
    }
    assert!(providers.requests("/revoke").is_empty());
    // Once only.
    legacy::migrate(&state).await.unwrap();
    assert!(stored(&state, ProviderId::Google).await.is_some());
}

#[tokio::test]
async fn old_google_tokens_from_a_user_client_are_revoked_and_reconnect_is_asked() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    old_google(&state, true).await;
    legacy::migrate(&state).await.unwrap();

    let revoke = providers.requests("/revoke");
    assert_eq!(revoke.len(), 1);
    assert_eq!(form(&revoke[0].body)["token"], "old-rt");
    assert!(stored(&state, ProviderId::Google).await.is_none());
    assert!(state.vault.get("google").await.unwrap().is_none());
    assert!(state
        .db
        .call(|c| settings_repo::get_setting(c, "google.client_id"))
        .unwrap()
        .is_none());
    let gmail = card(&state, ConnectorId::Gmail).await;
    assert_eq!(gmail.state, ConnectorState::ReauthRequired);
    assert!(gmail.detail.unwrap().contains("its own Google sign-in"));
}

// ── Spec B: one sign-in at a time, pages, checks, errors ─────────────

#[tokio::test]
async fn a_second_sign_in_is_refused_while_one_is_open() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let first = state.clone();
    let waiting =
        tokio::spawn(async move { connect(&first, ConnectorId::Gmail, |_| Ok(())).await });
    for _ in 0..100 {
        if card(&state, ConnectorId::Gmail).await.state == ConnectorState::Connecting {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // A double click, or Calendar while Gmail's sign-in is open.
    let opened = Arc::new(Mutex::new(0));
    for id in [ConnectorId::Gmail, ConnectorId::GoogleCalendar] {
        let count = opened.clone();
        let error = connect(&state, id, move |_| {
            *count.lock().unwrap() += 1;
            Ok(())
        })
        .await
        .unwrap_err();
        assert!(matches!(error, AppError::Conflict(_)), "{error}");
    }
    assert_eq!(*opened.lock().unwrap(), 0, "no second browser window");
    // Microsoft is a different account: its sign-in may run meanwhile.
    connect(
        &state,
        ConnectorId::OutlookMail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    state.connectors.cancel_sign_in(ProviderId::Google);
    assert!(waiting.await.unwrap().is_err());
    assert_eq!(
        card(&state, ConnectorId::Gmail).await.state,
        ConnectorState::Disconnected
    );
}

#[tokio::test]
async fn a_failed_code_exchange_says_so_in_the_browser_and_offers_retry() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    *providers.grants.google_exchange.lock().unwrap() = Some((
        401,
        r#"{"error":"invalid_client","error_description":"The OAuth client was not found."}"#
            .into(),
    ));
    let browser = Browser::default();
    let error = connect(&state, ConnectorId::Gmail, browser.open(Consent::Allow))
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Configuration(_)), "{error}");
    let page = page_shown(&browser).await;
    assert!(page.contains("ReMa could not complete authorization."));
    assert!(page.contains("Return to ReMa for details."));
    assert!(!page.contains("connected successfully"));
    let gmail = card(&state, ConnectorId::Gmail).await;
    assert_eq!(gmail.state, ConnectorState::Error);
    assert_eq!(
        gmail.error_code,
        Some(ConnectorErrorCode::ProviderConfigurationError)
    );
    assert!(gmail
        .message
        .unwrap()
        .contains("not a problem with your account"));
    assert!(gmail.detail.unwrap().contains("invalid_client"));
    assert!(stored(&state, ProviderId::Google).await.is_none());

    // Retry works once the registration is fixed, and clears the failure.
    *providers.grants.google_exchange.lock().unwrap() = None;
    connect(
        &state,
        ConnectorId::Gmail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    let gmail = card(&state, ConnectorId::Gmail).await;
    assert_eq!(gmail.state, ConnectorState::Connected);
    assert_eq!(gmail.error_code, None);
}

#[tokio::test]
async fn the_redirect_is_taken_once_and_only_on_its_own_path() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let redirect = Arc::new(Mutex::new(String::new()));
    let seen = redirect.clone();
    let browser = Browser::default();
    let open = browser.open(Consent::Allow);
    connect(&state, ConnectorId::Gmail, move |url| {
        *seen.lock().unwrap() = params(url)["redirect_uri"].clone();
        open(url)
    })
    .await
    .unwrap();
    // After the sign-in nothing listens there: a repeated callback (a
    // reloaded tab, a replayed code) is refused.
    let port: u16 = redirect
        .lock()
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err());
    assert_eq!(providers.requests("/token").len(), 1, "one code exchange");

    // Another path on the loopback port is not the redirect.
    assert!(oauth::parse_callback("/other?state=s1&code=x", "s1").is_none());
    assert!(oauth::parse_callback("/favicon.ico?code=x&state=s1", "s1").is_none());
    assert!(oauth::parse_callback("/?state=s1&code=x", "s1").is_some());
}

#[tokio::test]
async fn concurrent_requests_share_one_refresh() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    connect(
        &state,
        ConnectorId::Gmail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    expire(&state, ProviderId::Google).await;
    let callers: Vec<_> = (0..8)
        .map(|_| {
            let state = state.clone();
            tokio::spawn(async move {
                tokens::get_valid_access_token(&state, ProviderId::Google)
                    .await
                    .unwrap()
            })
        })
        .collect();
    for caller in callers {
        assert_eq!(caller.await.unwrap(), "g-at-2");
    }
    let refreshes = providers
        .requests("/token")
        .into_iter()
        .filter(|r| r.body.contains("grant_type=refresh_token"))
        .count();
    assert_eq!(refreshes, 1, "a single refresh for every caller");
}

#[tokio::test]
async fn grants_move_from_the_provider_key_to_the_account_key() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    // Stored by an earlier version: one key per provider, access token too.
    state
        .vault
        .set(
            &tokens::legacy_key(ProviderId::Google),
            Credential::OAuth {
                access_token: "at-from-before".into(),
                refresh_token: Some("rt-from-before".into()),
                expires_at: Some(now_ms() + 3_600_000),
            },
        )
        .await
        .unwrap();
    let now = now_ms();
    state
        .db
        .call(|c| {
            repo::save_account(
                c,
                &AccountRecord {
                    provider: ProviderId::Google,
                    account_id: Some("g-123".into()),
                    email: Some("ana@gmail.com".into()),
                    display_name: None,
                    granted_scopes: google::scopes(&[ConnectorId::Gmail]),
                    status: AccountStatus::Connected,
                    status_reason: None,
                    connected_at: now,
                    updated_at: now,
                },
            )?;
            repo::set_enabled(c, ConnectorId::Gmail, true, now)
        })
        .unwrap();
    // The access token still works (no refresh) …
    assert_eq!(
        tokens::get_valid_access_token(&state, ProviderId::Google)
            .await
            .unwrap(),
        "at-from-before"
    );
    assert!(providers.requests("/token").is_empty());
    // … and only the refresh token remains, under the account's key.
    assert!(state
        .vault
        .get_text(&tokens::legacy_key(ProviderId::Google))
        .await
        .unwrap()
        .is_none());
    let raw = state
        .vault
        .get_text(&tokens::vault_key(ProviderId::Google, "g-123"))
        .await
        .unwrap()
        .unwrap();
    assert!(raw.contains("rt-from-before") && !raw.contains("at-from-before"));
    assert_eq!(
        card(&state, ConnectorId::Gmail).await.state,
        ConnectorState::Connected
    );
}

#[tokio::test]
async fn a_connector_is_connected_only_when_its_api_answers() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    *providers.grants.gmail_profile.lock().unwrap() = (
        403,
        r#"{"error":{"code":403,"message":"Gmail API has not been used in project 1234 before or it is disabled.","status":"PERMISSION_DENIED","details":[{"reason":"SERVICE_DISABLED"}]}}"#
            .into(),
    );
    let browser = Browser::default();
    let error = connect(&state, ConnectorId::Gmail, browser.open(Consent::Allow))
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Configuration(_)), "{error}");
    assert!(page_shown(&browser)
        .await
        .contains("ReMa could not complete authorization."));
    let gmail = card(&state, ConnectorId::Gmail).await;
    assert_eq!(gmail.state, ConnectorState::Error);
    assert_eq!(gmail.error_code, Some(ConnectorErrorCode::ApiNotEnabled));
    assert!(gmail.message.unwrap().contains("Gmail API is not enabled"));
    // The sign-in itself is kept: once the API answers, nothing else is needed.
    assert!(stored(&state, ProviderId::Google).await.is_some());

    // After a restart the card still says why (it is stored).
    let restarted = card(&state, ConnectorId::Gmail).await;
    assert_eq!(
        restarted.error_code,
        Some(ConnectorErrorCode::ApiNotEnabled)
    );

    *providers.grants.gmail_profile.lock().unwrap() =
        (200, r#"{"emailAddress":"ana@gmail.com"}"#.into());
    connect(
        &state,
        ConnectorId::Gmail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    let gmail = card(&state, ConnectorId::Gmail).await;
    assert_eq!(gmail.state, ConnectorState::Connected);
    assert_eq!(gmail.error_code, None);
}

#[tokio::test]
async fn a_check_the_provider_cannot_answer_leaves_the_connector_connected() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    *providers.grants.gmail_profile.lock().unwrap() = (503, "Service Unavailable".into());
    connect(
        &state,
        ConnectorId::Gmail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    let gmail = card(&state, ConnectorId::Gmail).await;
    assert_eq!(gmail.state, ConnectorState::Connected);
    assert_eq!(gmail.error_code, None);
}

#[tokio::test]
async fn an_outlook_account_without_a_mailbox_is_explained() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    *providers.grants.outlook_messages.lock().unwrap() = (
        404,
        r#"{"error":{"code":"MailboxNotEnabledForRESTAPI","message":"The mailbox is either inactive, soft-deleted, or is hosted on-premise."}}"#
            .into(),
    );
    assert!(connect(
        &state,
        ConnectorId::OutlookMail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .is_err());
    let outlook = card(&state, ConnectorId::OutlookMail).await;
    assert_eq!(outlook.state, ConnectorState::Error);
    assert_eq!(
        outlook.error_code,
        Some(ConnectorErrorCode::AccountNotSupported)
    );
}

#[tokio::test]
async fn an_organization_that_requires_admin_approval_is_named_as_the_reason() {
    let providers = providers().await;
    *providers.grants.microsoft_scope.lock().unwrap() =
        "Calendars.ReadWrite User.Read openid profile email".into();
    let (state, _) = state_for(&providers, apps());
    let browser = Browser::default();
    let error = connect(
        &state,
        ConnectorId::OutlookCalendar,
        browser.open(Consent::Error(
            "access_denied",
            "AADSTS90094: An administrator of Contoso has set a policy that prevents you from granting ReMa the permissions it is requesting.",
        )),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, AppError::Permission(_)), "{error}");
    let calendar = card(&state, ConnectorId::OutlookCalendar).await;
    assert_eq!(calendar.state, ConnectorState::Error);
    assert_eq!(
        calendar.error_code,
        Some(ConnectorErrorCode::ProviderAdminPolicy)
    );
    assert!(calendar.message.unwrap().starts_with(
        "Your organization requires administrator approval before ReMa can access this Microsoft account."
    ));
    assert!(providers.requests("/common/oauth2/v2.0/token").is_empty());
    // A personal account still connects.
    connect(
        &state,
        ConnectorId::OutlookCalendar,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    assert_eq!(
        card(&state, ConnectorId::OutlookCalendar).await.state,
        ConnectorState::Connected
    );
}

#[tokio::test]
async fn offline_keeps_the_connection_and_its_grant() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    connect(
        &state,
        ConnectorId::Gmail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    expire(&state, ProviderId::Google).await;
    *providers.grants.token_down.lock().unwrap() = true;
    let error = tokens::get_valid_access_token(&state, ProviderId::Google)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Network(_)), "{error}");
    assert!(stored(&state, ProviderId::Google).await.is_some(), "kept");
    assert_eq!(
        card(&state, ConnectorId::Gmail).await.state,
        ConnectorState::Connected
    );
    // Back online: the next request refreshes normally.
    *providers.grants.token_down.lock().unwrap() = false;
    assert_eq!(
        tokens::get_valid_access_token(&state, ProviderId::Google)
            .await
            .unwrap(),
        "g-at-2"
    );
}

/// A credential store that counts reads and can be made to hang.
#[derive(Default)]
struct SlowStore {
    reads: Mutex<usize>,
    hang: Mutex<bool>,
    inner: crate::secrets::MemoryStore,
}

impl crate::secrets::SecretStore for SlowStore {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        *self.reads.lock().unwrap() += 1;
        if *self.hang.lock().unwrap() {
            std::thread::sleep(Duration::from_secs(2));
        }
        self.inner.get(account)
    }

    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        self.inner.set(account, secret)
    }

    fn delete(&self, account: &str) -> AppResult<()> {
        self.inner.delete(account)
    }
}

#[tokio::test]
async fn settings_never_wait_on_the_keychain_for_cards_that_were_never_connected() {
    let providers = providers().await;
    let (mut state, _) = state_for(&providers, apps());
    let store = Arc::new(SlowStore::default());
    state.vault = crate::secrets::SecretVault::new(store.clone());
    *store.hang.lock().unwrap() = true;
    let started = std::time::Instant::now();
    let cards = overview(&state).await.unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(
        *store.reads.lock().unwrap(),
        0,
        "a fresh install reads no keychain"
    );
    assert!(cards
        .connectors
        .iter()
        .filter(|c| c.provider != ProviderId::Xing)
        .all(|c| c.state == ConnectorState::Disconnected));

    // Connected, then the keychain stops answering: the card says so and
    // the section still appears.
    *store.hang.lock().unwrap() = false;
    connect(
        &state,
        ConnectorId::Gmail,
        Browser::default().open(Consent::Allow),
    )
    .await
    .unwrap();
    let (mut restarted, _) = state_for(&providers, apps());
    restarted.db = state.db.clone();
    restarted.vault = crate::secrets::SecretVault::new(store.clone());
    *store.hang.lock().unwrap() = true;
    let started = std::time::Instant::now();
    let gmail = card(&restarted, ConnectorId::Gmail).await;
    assert!(started.elapsed() < Duration::from_millis(1500));
    assert_eq!(gmail.state, ConnectorState::Error);
    assert!(gmail.message.unwrap().contains("system keychain"));
    *store.hang.lock().unwrap() = false;
}

#[test]
fn a_card_opening_the_browser_says_so_before_it_asks_to_finish_there() {
    let record = ConnectorRecord {
        id: ConnectorId::Gmail,
        enabled: false,
        last_sync_started_at: None,
        last_sync_completed_at: None,
        last_success_at: None,
        last_error: None,
        last_error_detail: None,
        last_error_code: None,
    };
    let signing_in = |opened| SigningIn {
        connectors: vec![ConnectorId::Gmail],
        opened,
    };
    let opening = status_of(
        &record,
        None,
        Ok(false),
        Some(signing_in(false)),
        false,
        true,
        None,
    );
    assert_eq!(opening.state, ConnectorState::Connecting);
    assert_eq!(opening.message.as_deref(), Some("Opening Google sign-in…"));
    let waiting = status_of(
        &record,
        None,
        Ok(false),
        Some(signing_in(true)),
        false,
        true,
        None,
    );
    assert_eq!(
        waiting.message.as_deref(),
        Some("Finish signing in with Google in your browser.")
    );
}
