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

/// What the mock providers grant and whether refreshes work.
struct Grants {
    google_scope: Mutex<String>,
    microsoft_scope: Mutex<String>,
    /// LinkedIn reports granted scopes comma-separated.
    linkedin_scope: Mutex<String>,
    refresh_ok: Mutex<bool>,
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
    });
    let g = grants.clone();
    let server = MockServer::start(move |req| {
        let refresh_ok = *g.refresh_ok.lock().unwrap();
        let exchange = req.body.contains("grant_type=authorization_code");
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
    state
        .vault
        .get(&tokens::vault_account(provider))
        .await
        .unwrap()
}

async fn expire(state: &AppState, provider: ProviderId) {
    let Some(Credential::OAuth {
        access_token,
        refresh_token,
        ..
    }) = stored(state, provider).await
    else {
        panic!("connected");
    };
    state
        .vault
        .set(
            &tokens::vault_account(provider),
            Credential::OAuth {
                access_token,
                refresh_token,
                expires_at: Some(now_ms() - 1_000),
            },
        )
        .await
        .unwrap();
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

    // The browser shows the success page.
    for _ in 0..50 {
        if browser.page.lock().unwrap().is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let page = browser.page.lock().unwrap().clone().unwrap();
    assert!(page.contains("ReMa connected successfully."));
    assert!(page.contains("You can close this tab and return to ReMa."));

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

    assert!(matches!(
        stored(&state, ProviderId::Google).await,
        Some(Credential::OAuth { ref access_token, refresh_token: Some(ref refresh), .. })
            if access_token == "g-at-1" && refresh == "g-rt-1"
    ));
    let secrets = [
        "g-at-1",
        "g-rt-1",
        "auth-code-123",
        verifier.as_str(),
        GOOGLE_SECRET,
    ];
    let database = database_text(&state);
    let overview = serde_json::to_string(&overview(&state).await.unwrap()).unwrap();
    let debug = format!(
        "{:?} {:?} {:?}",
        stored(&state, ProviderId::Google).await,
        state.connectors.app(ProviderId::Google),
        Pkce::from_verifier(verifier.clone())
    );
    for secret in secrets {
        assert!(!database.contains(secret), "{secret} in SQLite");
        assert!(!overview.contains(secret), "{secret} sent to the interface");
        assert!(!debug.contains(secret), "{secret} in debug output");
    }
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
    assert!(providers.requests("/gmail/").is_empty(), "no mail was read");
}

#[tokio::test]
async fn a_permission_left_unchecked_is_reported() {
    let providers = providers().await;
    let (state, _) = state_for(&providers, apps());
    let browser = Browser::default();
    // The user unticks the calendar permission on Google's consent screen.
    providers.grant_google(&["openid", "email", "profile", google::SCOPE_GMAIL_READONLY]);
    connect(
        &state,
        ConnectorId::GoogleCalendar,
        browser.open(Consent::Allow),
    )
    .await
    .unwrap();
    let calendar = card(&state, ConnectorId::GoogleCalendar).await;
    assert_eq!(calendar.state, ConnectorState::PermissionMissing);
    assert!(calendar.message.unwrap().contains("Calendar — Read events"));
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

    let error = connect(
        &state,
        ConnectorId::Gmail,
        browser.open(Consent::WrongState),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("invalid state"), "{error}");

    assert!(
        providers.requests("/token").is_empty(),
        "no code was exchanged"
    );
    assert!(stored(&state, ProviderId::Google).await.is_none());
    assert_eq!(
        card(&state, ConnectorId::Gmail).await.state,
        ConnectorState::Disconnected
    );
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
async fn an_unanswered_sign_in_times_out() {
    let loopback = Loopback::ipv4().await.unwrap();
    let error = oauth::wait_for_code(
        loopback,
        "state",
        "Google",
        &CancellationToken::new(),
        Duration::from_millis(100),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().to_lowercase().contains("time"), "{error}");
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
    assert_eq!(
        card(&state, ConnectorId::Linkedin).await.state,
        ConnectorState::Disconnected
    );
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
        Some(Credential::OAuth { refresh_token: Some(ref r), .. }) if r == "g-rt-1"
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
        Some(Credential::OAuth { refresh_token: Some(ref r), .. }) if r == "m-rt-2"
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
    assert!(gmail.message.unwrap().contains("Reconnect"));
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

    assert!(matches!(
        stored(&state, ProviderId::Google).await,
        Some(Credential::OAuth { ref access_token, .. }) if access_token == "old-at"
    ));
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
