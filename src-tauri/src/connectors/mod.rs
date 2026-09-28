//! Connectors: Gmail, Google Calendar, Outlook Mail, Outlook Calendar, and
//! the professional networks LinkedIn and XING (Network Connect).
//!
//! ```text
//! registry (this file) — the connectors, their state, connect/disconnect
//! oauth                — PKCE, state, loopback redirect, token requests
//! tokens               — the token manager (credential store, refresh, revoke)
//! api                  — authenticated provider API requests
//! google / microsoft   — provider auth config, mail and calendar clients
//! linkedin / xing      — professional-network sign-in and capabilities
//! mail / calendar      — provider-independent interfaces and models
//! sync                 — incremental synchronization and the background worker
//! legacy               — moving the old Google Workspace settings over
//! ```
//!
//! Gmail and Google Calendar are separate cards but share one Google account
//! (one token); Outlook Mail and Outlook Calendar share one Microsoft account.
//! All OAuth, tokens, API calls, sync state and permission checks live here,
//! in Rust. The interface only ever sees [`ConnectorStatus`].

pub mod api;
#[cfg(test)]
pub(crate) mod build_config;
pub mod calendar;
pub mod config;
pub mod failure;
pub mod google;
pub mod legacy;
pub mod linkedin;
pub mod mail;
pub mod microsoft;
pub mod oauth;
pub mod sync;
pub mod tokens;
pub mod validate;
pub mod xing;

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use serde_json::Value;

use tokio_util::sync::CancellationToken;

use self::{
    failure::{Failure, TokenPhase},
    google::GoogleEndpoints,
    linkedin::LinkedinEndpoints,
    microsoft::MicrosoftEndpoints,
    oauth::{AuthorizationRequest, OAuthApp, Pkce, TokenResponse},
    validate::Check,
};
use crate::{
    db::connectors::{self as repo, AccountRecord, AccountStatus, ConnectorRecord},
    error::{AppError, AppResult},
    models::connectors::{
        Capability, ConnectorErrorCode, ConnectorId, ConnectorKind, ConnectorState,
        ConnectorStatus, ConnectorsOverview, PermissionView, ProviderId,
    },
    oauth_loopback::{self, Loopback},
    state::AppState,
    time::now_ms,
};

/// How long a sign-in waits for the browser.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Writes one `[oauth]` or `[connector]` diagnostic line. Callers pass only
/// phases, categories and other non-secret facts; tests also keep the lines
/// to prove that no code, token, verifier or secret ever appears.
pub(crate) fn diag(line: String) {
    #[cfg(test)]
    diag_lines().lock().unwrap().push(line.clone());
    eprintln!("{line}");
}

#[cfg(test)]
pub(crate) fn diag_lines() -> &'static Mutex<Vec<String>> {
    static LINES: std::sync::OnceLock<Mutex<Vec<String>>> = std::sync::OnceLock::new();
    LINES.get_or_init(Mutex::default)
}

/// A sign-in waiting for the browser.
struct SignIn {
    seq: u64,
    cancel: CancellationToken,
    connectors: Vec<ConnectorId>,
    /// The browser was opened (before that the card says "Opening …").
    opened: bool,
}

/// A sign-in in progress, as a card shows it.
#[derive(Debug, Clone)]
pub struct SigningIn {
    pub connectors: Vec<ConnectorId>,
    pub opened: bool,
}

/// ReMa's app registrations (set at build time).
#[derive(Debug, Clone, Default)]
pub struct Apps {
    pub google: Option<OAuthApp>,
    pub microsoft: Option<OAuthApp>,
    pub linkedin: Option<OAuthApp>,
}

impl Apps {
    pub fn from_build() -> Self {
        Self {
            google: google::app(),
            microsoft: microsoft::app(),
            linkedin: linkedin::app(),
        }
    }
}

/// Shared connector state: endpoints, app registrations, the HTTP client,
/// sign-ins in progress and how the last one failed, access tokens in
/// memory, refresh locks and running syncs.
#[derive(Clone)]
pub struct ConnectorsContext {
    pub google: Arc<GoogleEndpoints>,
    pub microsoft: Arc<MicrosoftEndpoints>,
    pub linkedin: Arc<LinkedinEndpoints>,
    apps: Arc<Apps>,
    pub http: reqwest::Client,
    sign_ins: Arc<Mutex<HashMap<ProviderId, SignIn>>>,
    /// Why the last sign-in of a provider failed, for the card it started
    /// from (until the next sign-in or a disconnect).
    failures: Arc<Mutex<HashMap<ProviderId, (ConnectorId, Failure)>>>,
    seq: Arc<AtomicU64>,
    /// Access tokens by credential key, with their expiry (never persisted).
    access: Arc<Mutex<HashMap<String, (String, i64)>>>,
    /// One refresh in flight per provider: concurrent callers wait for it
    /// and share its result.
    refresh_locks: Arc<Mutex<HashMap<ProviderId, Arc<tokio::sync::Mutex<()>>>>>,
    /// Held while a grant is written or deleted, so a refresh that finishes
    /// after a disconnect cannot bring the grant back.
    grant_locks: Arc<Mutex<HashMap<ProviderId, Arc<tokio::sync::Mutex<()>>>>>,
    /// Counts each provider's disconnects: a refresh saves its result only
    /// if the count is what it was when the refresh started.
    grant_generation: Arc<Mutex<HashMap<ProviderId, u64>>>,
    /// Microsoft accounts whose calendar cannot answer `getSchedule`
    /// (personal accounts): availability comes from `calendarView` instead.
    /// Learnt at sign-in or from the first refusal, for this run of ReMa.
    microsoft_schedule_unsupported: Arc<std::sync::atomic::AtomicBool>,
    /// Running syncs (at most one per connector).
    pub(crate) syncs: Arc<Mutex<HashMap<ConnectorId, CancellationToken>>>,
    pub(crate) shutdown: CancellationToken,
    /// What this build says about its Google app: in Testing (Google ends
    /// sign-ins about 7 days after they are made), in production, or not
    /// said.
    google_in_testing: Option<bool>,
}

impl ConnectorsContext {
    pub fn new(google: GoogleEndpoints, microsoft: MicrosoftEndpoints, apps: Apps) -> Self {
        Self {
            google: Arc::new(google),
            microsoft: Arc::new(microsoft),
            linkedin: Arc::new(LinkedinEndpoints::from_env()),
            apps: Arc::new(apps),
            http: crate::llm::http::client(),
            sign_ins: Arc::default(),
            failures: Arc::default(),
            seq: Arc::default(),
            access: Arc::default(),
            refresh_locks: Arc::default(),
            grant_locks: Arc::default(),
            grant_generation: Arc::default(),
            microsoft_schedule_unsupported: Arc::default(),
            syncs: Arc::default(),
            shutdown: CancellationToken::new(),
            google_in_testing: config::google_in_testing(),
        }
    }

    /// Another Google publishing status than the build's (tests).
    pub fn with_google_in_testing(mut self, testing: Option<bool>) -> Self {
        self.google_in_testing = testing;
        self
    }

    pub fn google_in_testing(&self) -> Option<bool> {
        self.google_in_testing
    }

    /// LinkedIn at other endpoints (tests).
    pub fn with_linkedin(mut self, endpoints: LinkedinEndpoints) -> Self {
        self.linkedin = Arc::new(endpoints);
        self
    }

    pub fn app(&self, provider: ProviderId) -> Option<OAuthApp> {
        match provider {
            ProviderId::Google => self.apps.google.clone(),
            ProviderId::Microsoft => self.apps.microsoft.clone(),
            ProviderId::Linkedin => self.apps.linkedin.clone(),
            // No desktop sign-in exists (see `xing`).
            ProviderId::Xing => None,
        }
    }

    pub fn token_url(&self, provider: ProviderId) -> &str {
        match provider {
            ProviderId::Google => &self.google.token,
            ProviderId::Microsoft => &self.microsoft.token,
            ProviderId::Linkedin => &self.linkedin.token,
            ProviderId::Xing => "",
        }
    }

    /// The lock that makes concurrent refreshes of one provider's account a
    /// single one.
    pub(crate) fn refresh_lock(&self, provider: ProviderId) -> Arc<tokio::sync::Mutex<()>> {
        self.refresh_locks
            .lock()
            .unwrap()
            .entry(provider)
            .or_default()
            .clone()
    }

    /// The lock held while a provider's grant is written or deleted.
    pub(crate) fn grant_lock(&self, provider: ProviderId) -> Arc<tokio::sync::Mutex<()>> {
        self.grant_locks
            .lock()
            .unwrap()
            .entry(provider)
            .or_default()
            .clone()
    }

    /// How many times the provider's grant has been deleted so far.
    pub(crate) fn grant_generation(&self, provider: ProviderId) -> u64 {
        self.grant_generation
            .lock()
            .unwrap()
            .get(&provider)
            .copied()
            .unwrap_or(0)
    }

    /// Records that the provider's grant was deleted: a refresh started
    /// before this must not save what it gets back.
    pub(crate) fn bump_grant_generation(&self, provider: ProviderId) {
        *self
            .grant_generation
            .lock()
            .unwrap()
            .entry(provider)
            .or_default() += 1;
    }

    /// Whether the connected Microsoft account's calendar has to be read
    /// with `calendarView` because `getSchedule` is not available to it.
    pub fn microsoft_schedule_unsupported(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.microsoft_schedule_unsupported.clone()
    }

    /// Keeps an access token in memory until shortly before it expires.
    pub(crate) fn cache_access(&self, key: &str, token: String, expires_at: i64) {
        self.access
            .lock()
            .unwrap()
            .insert(key.to_string(), (token, expires_at));
    }

    /// The account's access token, if it is still good for a while.
    pub(crate) fn cached_access(&self, key: &str) -> Option<String> {
        self.access
            .lock()
            .unwrap()
            .get(key)
            .filter(|(_, at)| *at > now_ms() + tokens::EXPIRY_MARGIN_MS)
            .map(|(token, _)| token.clone())
    }

    pub(crate) fn forget_access(&self, key: &str) {
        self.access.lock().unwrap().remove(key);
    }

    /// Forgets every access token of a provider's accounts (tests use it to
    /// let a token "expire").
    #[cfg(test)]
    pub(crate) fn forget_provider_access(&self, provider: ProviderId) {
        let prefix = format!("connector:{}", provider.as_str());
        self.access
            .lock()
            .unwrap()
            .retain(|key, _| !key.starts_with(&prefix));
    }

    fn signing_in(&self, provider: ProviderId) -> Option<SigningIn> {
        self.sign_ins
            .lock()
            .ok()?
            .get(&provider)
            .map(|s| SigningIn {
                connectors: s.connectors.clone(),
                opened: s.opened,
            })
    }

    /// Starts a sign-in. Only one runs per provider (Spec B §54): while one
    /// waits for the browser, another is refused; Cancel ends the first.
    fn begin_sign_in(
        &self,
        provider: ProviderId,
        connectors: Vec<ConnectorId>,
    ) -> AppResult<(u64, CancellationToken)> {
        let mut sign_ins = self.sign_ins.lock().unwrap();
        if sign_ins.contains_key(&provider) {
            return Err(AppError::conflict(format!(
                "A {} sign-in is already open in your browser. Finish it there or cancel it first.",
                provider.name()
            )));
        }
        let cancel = CancellationToken::new();
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        sign_ins.insert(
            provider,
            SignIn {
                seq,
                cancel: cancel.clone(),
                connectors,
                opened: false,
            },
        );
        self.failures.lock().unwrap().remove(&provider);
        Ok((seq, cancel))
    }

    fn browser_opened(&self, provider: ProviderId, seq: u64) {
        if let Some(sign_in) = self.sign_ins.lock().unwrap().get_mut(&provider) {
            if sign_in.seq == seq {
                sign_in.opened = true;
            }
        }
    }

    fn end_sign_in(&self, provider: ProviderId, seq: u64) {
        let mut sign_ins = self.sign_ins.lock().unwrap();
        if sign_ins.get(&provider).is_some_and(|s| s.seq == seq) {
            sign_ins.remove(&provider);
        }
    }

    fn record_failure(&self, provider: ProviderId, id: ConnectorId, failure: Failure) {
        self.failures
            .lock()
            .unwrap()
            .insert(provider, (id, failure));
    }

    fn failure(&self, id: ConnectorId) -> Option<Failure> {
        self.failures
            .lock()
            .unwrap()
            .get(&id.provider())
            .filter(|(failed, _)| *failed == id)
            .map(|(_, failure)| failure.clone())
    }

    fn clear_failure(&self, provider: ProviderId) {
        self.failures.lock().unwrap().remove(&provider);
    }

    pub fn cancel_sign_in(&self, provider: ProviderId) {
        if let Some(sign_in) = self.sign_ins.lock().unwrap().remove(&provider) {
            sign_in.cancel.cancel();
        }
    }

    pub fn is_syncing(&self, id: ConnectorId) -> bool {
        self.syncs.lock().is_ok_and(|s| s.contains_key(&id))
    }

    /// Stops running syncs (app exit).
    pub fn shutdown(&self) {
        self.shutdown.cancel();
        if let Ok(syncs) = self.syncs.lock() {
            for cancel in syncs.values() {
                cancel.cancel();
            }
        }
    }
}

pub fn unavailable_error(provider: ProviderId) -> AppError {
    AppError::configuration(unavailable_reason(provider))
}

/// Why a provider cannot be connected. Release builds always include
/// Google and Microsoft (`build.rs` fails without them), so for them this
/// is a development build's diagnostic, saying how to add the registration.
/// LinkedIn is optional (it needs LinkedIn's approval); XING has no sign-in.
pub fn unavailable_reason(provider: ProviderId) -> String {
    let setting = match provider {
        ProviderId::Google => build_setting::GOOGLE,
        ProviderId::Microsoft => build_setting::MICROSOFT,
        ProviderId::Linkedin => build_setting::LINKEDIN,
        ProviderId::Xing => return xing::UNAVAILABLE.to_string(),
    };
    if cfg!(debug_assertions) {
        format!(
            "Development build without ReMa's {} app registration: add it to \
             src-tauri/connectors.toml or set {setting} (see docs/connectors/registration.md).",
            provider.name()
        )
    } else if provider == ProviderId::Linkedin {
        "LinkedIn sign-in is not part of this version of ReMa. Company, job and public people \
         research work without it."
            .to_string()
    } else {
        format!(
            "{} sign-in is missing from this copy of ReMa.",
            provider.name()
        )
    }
}

/// The build settings that hold each registration (for diagnostics).
mod build_setting {
    pub const GOOGLE: &str = "GOOGLE_DESKTOP_CLIENT_ID (GOOGLE_DESKTOP_CLIENT_SECRET is optional)";
    pub const MICROSOFT: &str = "MICROSOFT_PUBLIC_CLIENT_ID";
    pub const LINKEDIN: &str = "LINKEDIN_CLIENT_ID";
}

/// Whether a provider issues refresh tokens to ReMa. LinkedIn's
/// self-service access tokens last 60 days and cannot be refreshed; the
/// member signs in again after that.
pub fn issues_refresh_tokens(provider: ProviderId) -> bool {
    matches!(provider, ProviderId::Google | ProviderId::Microsoft)
}

/// The scopes a provider sign-in asks for.
pub fn provider_scopes(provider: ProviderId, connectors: &[ConnectorId]) -> Vec<String> {
    match provider {
        ProviderId::Google => google::scopes(connectors),
        ProviderId::Microsoft => microsoft::scopes(connectors),
        ProviderId::Linkedin => linkedin::scopes(),
        ProviderId::Xing => Vec::new(),
    }
}

pub fn allows(provider: ProviderId, capability: Capability, granted: &[String]) -> bool {
    match provider {
        ProviderId::Google => google::allows(capability, granted),
        ProviderId::Microsoft => microsoft::allows(capability, granted),
        ProviderId::Linkedin => linkedin::allows(capability, granted),
        ProviderId::Xing => xing::allows(capability, granted),
    }
}

/// Capabilities a connector cannot work without (free/busy is optional:
/// availability then comes from the event list; a professional network's
/// connection list is optional: most apps are never approved for it).
fn essential(id: ConnectorId) -> &'static [Capability] {
    match id.kind() {
        ConnectorKind::Mail => &[Capability::MailRead],
        ConnectorKind::Calendar => &[Capability::CalendarRead, Capability::CalendarWrite],
        ConnectorKind::Network => &[Capability::NetworkIdentity],
    }
}

fn enabled_of(records: &[ConnectorRecord], provider: ProviderId) -> Vec<ConnectorId> {
    records
        .iter()
        .filter(|r| r.enabled && r.id.provider() == provider)
        .map(|r| r.id)
        .collect()
}

/// Account details from the ID token, and for Microsoft from Graph `/me`
/// (the identity check of Spec B §49).
struct Profile {
    account_id: Option<String>,
    email: Option<String>,
    name: Option<String>,
}

fn claim(claims: &Value, key: &str) -> Option<String> {
    claims
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|v| !v.is_empty())
}

async fn profile(state: &AppState, provider: ProviderId, tokens: &TokenResponse) -> Profile {
    let claims = tokens
        .id_token
        .as_deref()
        .and_then(oauth::id_token_claims)
        .unwrap_or(Value::Null);
    let mut profile = match provider {
        ProviderId::Microsoft => Profile {
            account_id: claim(&claims, "oid").or_else(|| claim(&claims, "sub")),
            email: claim(&claims, "email").or_else(|| claim(&claims, "preferred_username")),
            name: claim(&claims, "name"),
        },
        _ => Profile {
            account_id: claim(&claims, "sub"),
            email: claim(&claims, "email"),
            name: claim(&claims, "name"),
        },
    };
    if provider == ProviderId::Linkedin && (profile.account_id.is_none() || profile.name.is_none())
    {
        // OpenID Connect userinfo with the new token.
        let info = state
            .connectors
            .http
            .get(&state.connectors.linkedin.userinfo)
            .bearer_auth(&tokens.access_token)
            .timeout(Duration::from_secs(15))
            .send()
            .await;
        if let Ok(response) = info {
            if let Ok(info) = response.json::<Value>().await {
                let (sub, email, name) = linkedin::userinfo_profile(&info);
                profile.account_id = profile.account_id.or(sub);
                profile.email = profile.email.or(email);
                profile.name = profile.name.or(name);
            }
        }
    }
    if provider == ProviderId::Microsoft {
        // A personal Microsoft account (the consumers tenant) cannot use
        // Graph's getSchedule: its availability comes from the calendar view.
        let personal = claim(&claims, "tid")
            .is_some_and(|tid| tid.eq_ignore_ascii_case(microsoft::CONSUMERS_TENANT));
        state
            .connectors
            .microsoft_schedule_unsupported
            .store(personal, std::sync::atomic::Ordering::Relaxed);
        // Graph /me with the new token (User.Read): the Graph user and its
        // mail address or user principal name.
        let me = state
            .connectors
            .http
            .get(format!(
                "{}/me?$select=id,displayName,mail,userPrincipalName",
                state.connectors.microsoft.graph
            ))
            .bearer_auth(&tokens.access_token)
            .timeout(Duration::from_secs(15))
            .send()
            .await;
        let checked = match me {
            Ok(response) if response.status().is_success() => {
                match response.json::<Value>().await {
                    Ok(me) => {
                        profile.email = claim(&me, "mail")
                            .or_else(|| claim(&me, "userPrincipalName"))
                            .or(profile.email.take());
                        profile.name = profile.name.or_else(|| claim(&me, "displayName"));
                        profile.account_id = profile.account_id.or_else(|| claim(&me, "id"));
                        "success"
                    }
                    Err(_) => "unreadable",
                }
            }
            Ok(_) => "refused",
            Err(_) => "unreachable",
        };
        diag(format!(
            "[connector] provider=microsoft identity=graph_me validation={checked}"
        ));
    }
    profile
}

/// Runs a sign-in for a connector: the default browser shows the provider's
/// own account chooser and consent screen; ReMa waits on the loopback
/// redirect (bound before the browser opens), checks `state`, exchanges the
/// code with the PKCE verifier, stores the grant, identifies the account and
/// checks each connector with one small request. The browser tab shows how
/// it ended. `open_browser` opens a URL in the system browser.
///
/// A failure is remembered on the card the user clicked (Error with Retry)
/// unless the user cancelled.
pub async fn connect(
    state: &AppState,
    id: ConnectorId,
    open_browser: impl FnOnce(&str) -> AppResult<()>,
) -> AppResult<()> {
    let provider = id.provider();
    let name = provider.name();
    let app = state
        .connectors
        .app(provider)
        .ok_or_else(|| unavailable_error(provider))?;
    // No incremental authorization for installed apps: ask for the union of
    // this connector and the provider's other enabled connectors.
    let mut wanted = state
        .db
        .call(|c| repo::connectors(c))
        .map(|records| enabled_of(&records, provider))?;
    if !wanted.contains(&id) {
        wanted.push(id);
    }
    let scopes = provider_scopes(provider, &wanted);
    let pkce = Pkce::new()?;
    let csrf = oauth::random_token(24)?;
    let (seq, cancel) = state.connectors.begin_sign_in(provider, wanted.clone())?;
    state.events.connectors_changed();
    oauth::log(
        name,
        "started",
        &format!(
            "connectors={}",
            wanted
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ),
    );
    let result = sign_in(
        state,
        &SignInRequest {
            id,
            app: &app,
            wanted: &wanted,
            scopes: &scopes,
            pkce: &pkce,
            csrf: &csrf,
            seq,
        },
        &cancel,
        open_browser,
    )
    .await;
    state.connectors.end_sign_in(provider, seq);
    let outcome = match result {
        Ok(()) => {
            oauth::log(name, "connected", "");
            Ok(())
        }
        Err(failure) => {
            let phase = match failure.code {
                ConnectorErrorCode::UserCancelled => "cancelled",
                ConnectorErrorCode::SignInTimedOut => "timed_out",
                _ => "failed",
            };
            oauth::log(name, phase, &format!("category={}", failure.code.as_str()));
            if failure.code != ConnectorErrorCode::UserCancelled {
                state
                    .connectors
                    .record_failure(provider, id, failure.clone());
            }
            Err(failure.into_error())
        }
    };
    state.events.connectors_changed();
    outcome
}

/// What one sign-in needs.
struct SignInRequest<'a> {
    id: ConnectorId,
    app: &'a OAuthApp,
    wanted: &'a [ConnectorId],
    scopes: &'a [String],
    pkce: &'a Pkce,
    csrf: &'a str,
    seq: u64,
}

async fn sign_in(
    state: &AppState,
    request: &SignInRequest<'_>,
    cancel: &CancellationToken,
    open_browser: impl FnOnce(&str) -> AppResult<()>,
) -> Result<(), Failure> {
    let provider = request.id.provider();
    let name = provider.name();
    // The listener exists before the browser opens (Spec B §10).
    let bound = match provider {
        // Microsoft: `http://localhost` (registered without a port; any port
        // matches), listened for on IPv4 and IPv6 loopback.
        ProviderId::Microsoft => Loopback::dual_stack()
            .await
            .map(|l| (format!("http://localhost:{}", l.port()), l)),
        // Google and LinkedIn (native clients): the loopback IP literal.
        _ => Loopback::ipv4()
            .await
            .map(|l| (format!("http://127.0.0.1:{}", l.port()), l)),
    };
    let (redirect_uri, loopback) = bound.map_err(|e| {
        Failure::new(
            ConnectorErrorCode::NetworkError,
            "ReMa could not listen for the sign-in on this computer.",
        )
        .with_detail(e.to_string())
    })?;
    oauth::log(name, "listener_bound", &format!("port={}", loopback.port()));
    let (endpoint, extra): (&str, &[(&str, &str)]) = match provider {
        ProviderId::Google => (
            &state.connectors.google.auth,
            // Refresh token, account chooser, and a fresh consent screen that
            // lists every requested scope.
            &[
                ("access_type", "offline"),
                ("prompt", "consent select_account"),
            ],
        ),
        ProviderId::Microsoft => (
            &state.connectors.microsoft.authorize,
            &[("prompt", "select_account"), ("response_mode", "query")],
        ),
        ProviderId::Linkedin => (&state.connectors.linkedin.auth, &[]),
        ProviderId::Xing => {
            return Err(Failure::new(
                ConnectorErrorCode::ProviderConfigurationError,
                unavailable_reason(provider),
            ))
        }
    };
    let url = oauth::authorization_url(&AuthorizationRequest {
        endpoint,
        client_id: &request.app.client_id,
        redirect_uri: &redirect_uri,
        scopes: request.scopes,
        challenge: &request.pkce.challenge,
        state: request.csrf,
        extra,
    })
    .map_err(|e| failure::browser_unavailable(&e.to_string()))?;
    open_browser(&url).map_err(|e| failure::browser_unavailable(&e.to_string()))?;
    state.connectors.browser_opened(provider, request.seq);
    state.events.connectors_changed();
    oauth::log(name, "browser_opened", "");

    let (code, reply) =
        oauth::wait_for_callback(loopback, request.csrf, name, cancel, SIGN_IN_TIMEOUT).await?;
    let finished = async {
        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code),
            ("code_verifier", request.pkce.verifier.clone()),
            ("redirect_uri", redirect_uri.clone()),
        ];
        if provider == ProviderId::Microsoft {
            form.push(("scope", request.scopes.join(" ")));
        }
        let exchanged = oauth::token_request(
            &state.connectors.http,
            state.connectors.token_url(provider),
            request.app,
            name,
            TokenPhase::Exchange,
            form,
        )
        .await;
        let tokens = match exchanged {
            Ok(tokens) => tokens,
            Err(failure) => {
                oauth::log(
                    name,
                    "token_exchange_failed",
                    &format!("category={}", failure.code.as_str()),
                );
                return Err(failure);
            }
        };
        oauth::log(name, "token_exchange_success", "");
        let access_token = tokens.access_token.clone();
        finish_sign_in(state, provider, request.wanted, request.scopes, tokens).await?;
        check(state, request.id, request.wanted, &access_token).await
    }
    .await;
    reply
        .send(if finished.is_ok() {
            oauth_loopback::CONNECTED
        } else {
            oauth_loopback::NOT_CONNECTED
        })
        .await;
    finished
}

/// Checks every connector of the sign-in with the new access token and
/// records the result on its card. The sign-in fails when the connector the
/// user clicked does not work; a provider that could not be reached decides
/// nothing (the connector stays connected and its next use tries again).
async fn check(
    state: &AppState,
    clicked: ConnectorId,
    connectors: &[ConnectorId],
    access_token: &str,
) -> Result<(), Failure> {
    let granted = state
        .db
        .call(|c| repo::account(c, clicked.provider()))
        .ok()
        .flatten()
        .map(|a| a.granted_scopes)
        .unwrap_or_default();
    let mut outcome = Ok(());
    for id in connectors {
        let result = if essential(*id)
            .iter()
            .all(|c| allows(id.provider(), *c, &granted))
        {
            validate::connector(state, *id, access_token).await
        } else {
            // A permission left unticked on the consent screen: no request
            // needed to know it will be refused.
            let missing = Failure::new(
                ConnectorErrorCode::ScopeNotGranted,
                format!(
                    "{} did not grant ReMa permission to use {}. Reconnect and allow access on \
                     {}'s screen.",
                    id.provider().name(),
                    id.name(),
                    id.provider().name()
                ),
            );
            diag(format!(
                "[connector] provider={} capability={} validation=failure category=SCOPE_NOT_GRANTED",
                id.provider().as_str(),
                id.as_str()
            ));
            Check::Failed(missing)
        };
        let saved = state.db.call(|c| match &result {
            Check::Failed(failure) => repo::set_check_error(
                c,
                *id,
                failure.code,
                &failure.message,
                failure.detail.as_deref(),
            ),
            Check::Passed | Check::Skipped(_) => repo::clear_error(c, *id),
        });
        if let Err(error) = saved {
            eprintln!("[connector] could not record a check: {error}");
        }
        if let (Check::Failed(failure), true) = (result, *id == clicked) {
            outcome = Err(failure);
        }
    }
    outcome
}

async fn finish_sign_in(
    state: &AppState,
    provider: ProviderId,
    connectors: &[ConnectorId],
    requested: &[String],
    tokens: TokenResponse,
) -> Result<(), Failure> {
    let internal = |e: AppError| {
        Failure::new(
            ConnectorErrorCode::TokenExchangeFailed,
            format!("ReMa could not save the {} sign-in.", provider.name()),
        )
        .with_detail(e.to_string())
    };
    // Space-separated (Google, Microsoft) or comma-separated (LinkedIn).
    let granted: Vec<String> = tokens
        .scope
        .as_deref()
        .map(|s| {
            s.split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| requested.to_vec());
    let profile = profile(state, provider, &tokens).await;
    // The provider's stable account id keys the grant (Spec B §56, §69).
    let Some(account_id) = profile.account_id.clone() else {
        return Err(Failure::new(
            ConnectorErrorCode::TokenExchangeFailed,
            format!(
                "{} did not say which account signed in, so nothing was connected. Click Retry.",
                provider.name()
            ),
        ));
    };
    oauth::log(provider.name(), "account_identified", "");
    let previous = state
        .db
        .call(|c| repo::account(c, provider))
        .map_err(internal)?;
    let same_account = previous
        .as_ref()
        .is_some_and(|p| p.account_id.as_deref() == Some(account_id.as_str()));
    // The previous refresh token of the same account stands in only when the
    // provider issued none now and that grant was still good; a rejected one
    // was deleted when the account needed reconnecting.
    let previous_refresh = if same_account {
        tokens::current_refresh_token(state, provider)
            .await
            .map_err(internal)?
    } else {
        None
    };
    if let Some(previous) = previous.as_ref().filter(|_| !same_account) {
        // Another account replaces this provider's account entirely.
        tokens::forget_account(state, provider, previous.account_id.as_deref())
            .await
            .map_err(internal)?;
    }
    tokens::store(state, provider, &account_id, &tokens, previous_refresh)
        .await
        .map_err(|e| Failure::new(ConnectorErrorCode::TokenExchangeFailed, e.to_string()))?;
    let now = now_ms();
    state
        .db
        .call(|c| {
            let tx = c.transaction()?;
            if previous.is_some() && !same_account {
                // Another account: its sync position means nothing here.
                tx.execute(
                    "DELETE FROM sync_cursors WHERE provider = ?1",
                    [provider.as_str()],
                )?;
            }
            repo::save_account(
                &tx,
                &AccountRecord {
                    provider,
                    account_id: Some(account_id.clone()),
                    email: profile.email.clone(),
                    display_name: profile.name.clone(),
                    granted_scopes: granted.clone(),
                    status: AccountStatus::Connected,
                    status_reason: None,
                    status_cause: None,
                    connected_at: now,
                    updated_at: now,
                },
            )?;
            for id in connectors {
                let record = repo::connector(&tx, *id)?;
                if !record.enabled {
                    repo::set_enabled(&tx, *id, true, now)?;
                } else {
                    // Reconnected: clear the old error. Connecting reads no
                    // mail (only "Job Mail & Interview Sync" does), so no sync
                    // is recorded either.
                    repo::clear_error(&tx, *id)?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .map_err(internal)?;
    Ok(())
}

/// Removes a connector. The last connector of an account also signs out:
/// ReMa revokes its access (Google) and deletes the stored tokens. The
/// application tracker and its history are kept.
pub async fn disconnect(state: &AppState, id: ConnectorId) -> AppResult<()> {
    let provider = id.provider();
    if let Some(cancel) = state.connectors.syncs.lock().unwrap().get(&id) {
        cancel.cancel();
    }
    state.connectors.clear_failure(provider);
    let now = now_ms();
    let remaining = state.db.call(|c| {
        let tx = c.transaction()?;
        repo::set_enabled(&tx, id, false, now)?;
        repo::clear_sync_state(&tx, id)?;
        if id.kind() == ConnectorKind::Mail {
            repo::delete_cursors(&tx, provider, "mail:")?;
        }
        let remaining = enabled_of(&repo::connectors(&tx)?, provider);
        tx.commit()?;
        Ok(remaining)
    })?;
    if remaining.is_empty() {
        tokens::revoke_connection(state, provider).await?;
        state.db.call(|c| repo::delete_account(c, provider))?;
        diag(format!(
            "[connector] provider={} state=disconnected",
            provider.as_str()
        ));
        if provider == ProviderId::Linkedin {
            // Nothing LinkedIn returned outlives the connection.
            state.network.forget_provider_data();
        }
    }
    state.events.connectors_changed();
    Ok(())
}

/// Checks that a connector can be used right now; explains what to do if not.
pub async fn require(state: &AppState, id: ConnectorId) -> AppResult<AccountRecord> {
    let (record, account) = state
        .db
        .call(|c| Ok((repo::connector(c, id)?, repo::account(c, id.provider())?)))?;
    let name = id.name();
    if !record.enabled {
        return Err(AppError::configuration(format!(
            "Connect {name} in Settings → Connectors first."
        )));
    }
    let Some(account) = account else {
        return Err(AppError::authentication(format!(
            "Connect {name} in Settings → Connectors first."
        )));
    };
    if tokens::requires_reauthentication(state, id.provider()).await? {
        return Err(AppError::authentication(format!(
            "{name} needs to be reconnected in Settings → Connectors."
        )));
    }
    if !essential(id)
        .iter()
        .all(|c| allows(id.provider(), *c, &account.granted_scopes))
    {
        return Err(AppError::authentication(format!(
            "ReMa does not have permission to use {name}. Reconnect it in Settings → Connectors \
             and allow access."
        )));
    }
    Ok(account)
}

/// Connectors that can be used right now, of one kind.
pub async fn ready(state: &AppState, kind: ConnectorKind) -> Vec<ConnectorId> {
    let mut ready = Vec::new();
    for id in ConnectorId::ALL {
        if id.kind() == kind && require(state, id).await.is_ok() {
            ready.push(id);
        }
    }
    ready
}

/// Every connector with its state, for Settings. The system keychain is read
/// only for connected accounts, and a keychain that does not answer shows as
/// an error on those cards instead of hiding the section.
pub async fn overview(state: &AppState) -> AppResult<ConnectorsOverview> {
    let (records, accounts) = state.db.call(|c| {
        let mut accounts = Vec::new();
        for provider in ProviderId::ALL {
            accounts.push(repo::account(c, provider)?);
        }
        Ok((repo::connectors(c)?, accounts))
    })?;
    let mut grants: HashMap<ProviderId, Result<bool, String>> = HashMap::new();
    let mut connectors = Vec::new();
    for record in records {
        let provider = record.id.provider();
        let account = accounts[provider.index()].clone();
        let has_token = if record.enabled && account.is_some() {
            match grants.get(&provider) {
                Some(known) => known.clone(),
                None => {
                    let known =
                        match tokio::time::timeout(KEYCHAIN_WAIT, tokens::usable(state, provider))
                            .await
                        {
                            Ok(Ok(usable)) => Ok(usable),
                            Ok(Err(error)) => Err(error.to_string()),
                            Err(_) => Err("the system keychain did not answer".to_string()),
                        };
                    grants.insert(provider, known.clone());
                    known
                }
            }
        } else {
            Ok(false)
        };
        let mut status = status_of(
            &record,
            account.as_ref(),
            has_token,
            state.connectors.signing_in(provider),
            state.connectors.is_syncing(record.id),
            state.connectors.app(provider).is_some(),
            state.connectors.failure(record.id),
        );
        explain_sign_in(
            &mut status,
            account.as_ref(),
            state.connectors.google_in_testing(),
        );
        connectors.push(status);
    }
    Ok(ConnectorsOverview {
        connectors,
        background: crate::services::background::settings(state)?,
        mail_processing: mail_processing(state)?,
    })
}

/// Where job-related email goes to be read by a model (Spec B §64).
pub fn mail_processing(
    state: &AppState,
) -> AppResult<Option<crate::models::connectors::MailProcessing>> {
    use crate::models::provider::{ConnectionMethod, ProviderKind};
    let model = match crate::services::tasks::job_mail_sync(state)? {
        Some(task) => Some(task.model),
        None => state
            .db
            .call(|c| crate::services::providers::default_model(c))?,
    };
    let Some(model) = model else {
        return Ok(None);
    };
    let Some(row) = state
        .db
        .call(|c| crate::db::providers::get(c, &model.provider_id))?
    else {
        return Ok(None);
    };
    let (recipient, on_device) = match row.kind {
        ProviderKind::OpenaiCompatible => {
            let host = row
                .base_url
                .as_deref()
                .and_then(|u| reqwest::Url::parse(u).ok())
                .and_then(|u| u.host_str().map(|h| h.trim_matches(['[', ']']).to_string()))
                .unwrap_or_default();
            let local = host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback());
            if local {
                ("this computer".to_string(), true)
            } else {
                (host, false)
            }
        }
        ProviderKind::Openai if row.connection == ConnectionMethod::ChatgptAccount => {
            ("OpenAI (your ChatGPT account)".to_string(), false)
        }
        ProviderKind::Openai => ("OpenAI".to_string(), false),
        ProviderKind::Anthropic => ("Anthropic".to_string(), false),
        ProviderKind::Gemini => ("Google (Gemini)".to_string(), false),
    };
    Ok(Some(crate::models::connectors::MailProcessing {
        model: model.model_id,
        recipient,
        on_device,
    }))
}

/// How long Settings waits for the system keychain before showing the
/// connected cards as unreadable.
const KEYCHAIN_WAIT: Duration = Duration::from_millis(if cfg!(test) { 300 } else { 10_000 });

/// The card for one connector.
pub fn status_of(
    record: &ConnectorRecord,
    account: Option<&AccountRecord>,
    has_token: Result<bool, String>,
    signing_in: Option<SigningIn>,
    syncing: bool,
    available: bool,
    failure: Option<Failure>,
) -> ConnectorStatus {
    let id = record.id;
    let provider = id.provider();
    let granted = account.map(|a| a.granted_scopes.as_slice()).unwrap_or(&[]);
    let connected_account = account.filter(|_| record.enabled);
    let permissions: Vec<PermissionView> = id
        .capabilities()
        .iter()
        .map(|c| PermissionView {
            capability: *c,
            label: c.label().to_string(),
            granted: connected_account.is_some() && allows(provider, *c, granted),
        })
        .collect();
    let missing: Vec<&PermissionView> = permissions
        .iter()
        .filter(|p| !p.granted && essential(id).contains(&p.capability))
        .collect();

    let mut error_code = None;
    let (state, message, detail) = match &signing_in {
        Some(sign_in) if sign_in.connectors.contains(&id) => (
            ConnectorState::Connecting,
            Some(if sign_in.opened {
                format!(
                    "Finish signing in with {} in your browser.",
                    provider.name()
                )
            } else {
                format!("Opening {} sign-in…", provider.name())
            }),
            None,
        ),
        _ if !record.enabled => match &failure {
            // The last sign-in from this card failed: Retry is offered.
            Some(failure) => {
                error_code = Some(failure.code);
                (
                    ConnectorState::Error,
                    Some(failure.message.clone()),
                    failure.detail.clone(),
                )
            }
            None if available => (ConnectorState::Disconnected, None, None),
            None => (
                ConnectorState::Unavailable,
                Some(unavailable_reason(provider)),
                None,
            ),
        },
        _ => match (account, &has_token) {
            (None, _) => (
                ConnectorState::ReauthRequired,
                Some("Sign in again to use this connector.".into()),
                None,
            ),
            (Some(_), Err(reason)) => {
                error_code = Some(ConnectorErrorCode::CredentialStoreUnavailable);
                (
                    ConnectorState::Error,
                    Some(
                        "ReMa could not read its sign-in from your system keychain. Unlock the \
                         keychain, or restart ReMa; the connection itself is unchanged."
                            .into(),
                    ),
                    Some(reason.clone()),
                )
            }
            (Some(a), Ok(has_token)) if a.status == AccountStatus::ReauthRequired || !has_token => {
                error_code = Some(ConnectorErrorCode::ReauthRequired);
                (
                    ConnectorState::ReauthRequired,
                    Some(format!(
                        "Reconnect required: {} access was revoked or has expired.",
                        provider.name()
                    )),
                    a.status_reason.clone(),
                )
            }
            (Some(_), _) if !missing.is_empty() => {
                error_code = Some(ConnectorErrorCode::ScopeNotGranted);
                (
                    ConnectorState::PermissionMissing,
                    Some(format!(
                        "Permission not granted: {}. Reconnect and allow access.",
                        missing
                            .iter()
                            .map(|p| p.label.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    None,
                )
            }
            (Some(_), _) if syncing => (ConnectorState::Syncing, None, None),
            (Some(_), _) if record.last_error.is_some() => {
                error_code = record.last_error_code;
                (
                    ConnectorState::Error,
                    record.last_error.clone(),
                    record.last_error_detail.clone(),
                )
            }
            (Some(_), _) => match &failure {
                // A reconnect that failed leaves the working connection as
                // it was, and says why.
                Some(failure) => {
                    error_code = Some(failure.code);
                    (
                        ConnectorState::Connected,
                        Some(failure.message.clone()),
                        failure.detail.clone(),
                    )
                }
                None => (ConnectorState::Connected, None, None),
            },
        },
    };
    ConnectorStatus {
        id,
        provider,
        kind: id.kind(),
        name: id.name().into(),
        publisher: provider.name().into(),
        description: id.description().into(),
        state,
        enabled: record.enabled,
        account_email: connected_account.and_then(|a| a.email.clone()),
        account_name: connected_account.and_then(|a| a.display_name.clone()),
        permissions,
        last_sync_started_at: record.last_sync_started_at,
        last_sync_at: record.last_success_at,
        message,
        detail,
        error_code,
        sign_in_ends_at: None,
    }
}

/// Says why a connection must be renewed (the cause the provider gave, and
/// whose setting it is), and, while this build says ReMa's Google app is in
/// Testing, about when Google will end a sign-in (`sign_in_ends_at`: an
/// estimate from the sign-in time, since Google says "7 days" and no more).
pub fn explain_sign_in(
    status: &mut ConnectorStatus,
    account: Option<&AccountRecord>,
    google_in_testing: Option<bool>,
) {
    let Some(account) = account else {
        return;
    };
    match status.state {
        ConnectorState::ReauthRequired if account.status == AccountStatus::ReauthRequired => {
            status.message = Some(failure::reauth_message(
                status.provider,
                account.status_cause,
                google_in_testing,
            ));
        }
        ConnectorState::Connected | ConnectorState::Syncing | ConnectorState::Error
            if status.provider == ProviderId::Google
                && google_in_testing == Some(true)
                && account.status == AccountStatus::Connected =>
        {
            status.sign_in_ends_at = Some(account.connected_at + failure::GOOGLE_TESTING_GRANT_MS);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
