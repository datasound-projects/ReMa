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
pub mod calendar;
pub mod google;
pub mod legacy;
pub mod linkedin;
pub mod mail;
pub mod microsoft;
pub mod oauth;
pub mod sync;
pub mod tokens;
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
    google::GoogleEndpoints,
    linkedin::LinkedinEndpoints,
    microsoft::MicrosoftEndpoints,
    oauth::{AuthorizationRequest, OAuthApp, Pkce, TokenResponse},
};
use crate::{
    db::connectors::{self as repo, AccountRecord, AccountStatus, ConnectorRecord},
    error::{AppError, AppResult},
    models::connectors::{
        Capability, ConnectorId, ConnectorKind, ConnectorState, ConnectorStatus,
        ConnectorsOverview, PermissionView, ProviderId,
    },
    oauth_loopback::Loopback,
    secrets::Credential,
    state::AppState,
    time::now_ms,
};

/// How long a sign-in waits for the browser.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// A sign-in waiting for the browser.
struct SignIn {
    seq: u64,
    cancel: CancellationToken,
    connectors: Vec<ConnectorId>,
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
/// sign-ins in progress, refresh locks and running syncs.
#[derive(Clone)]
pub struct ConnectorsContext {
    pub google: Arc<GoogleEndpoints>,
    pub microsoft: Arc<MicrosoftEndpoints>,
    pub linkedin: Arc<LinkedinEndpoints>,
    apps: Arc<Apps>,
    pub http: reqwest::Client,
    sign_ins: Arc<Mutex<HashMap<ProviderId, SignIn>>>,
    seq: Arc<AtomicU64>,
    refresh_locks: Arc<[tokio::sync::Mutex<()>; 4]>,
    /// Running syncs (at most one per connector).
    pub(crate) syncs: Arc<Mutex<HashMap<ConnectorId, CancellationToken>>>,
    pub(crate) shutdown: CancellationToken,
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
            seq: Arc::default(),
            refresh_locks: Arc::new(std::array::from_fn(|_| tokio::sync::Mutex::new(()))),
            syncs: Arc::default(),
            shutdown: CancellationToken::new(),
        }
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

    pub(crate) fn refresh_lock(&self, provider: ProviderId) -> &tokio::sync::Mutex<()> {
        &self.refresh_locks[provider.index()]
    }

    fn signing_in(&self, provider: ProviderId) -> Option<Vec<ConnectorId>> {
        self.sign_ins
            .lock()
            .ok()?
            .get(&provider)
            .map(|s| s.connectors.clone())
    }

    /// Starts a sign-in, cancelling an abandoned one for the same provider.
    fn begin_sign_in(
        &self,
        provider: ProviderId,
        connectors: Vec<ConnectorId>,
    ) -> (u64, CancellationToken) {
        let cancel = CancellationToken::new();
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        if let Some(previous) = self.sign_ins.lock().unwrap().insert(
            provider,
            SignIn {
                seq,
                cancel: cancel.clone(),
                connectors,
            },
        ) {
            previous.cancel.cancel();
        }
        (seq, cancel)
    }

    fn end_sign_in(&self, provider: ProviderId, seq: u64) {
        let mut sign_ins = self.sign_ins.lock().unwrap();
        if sign_ins.get(&provider).is_some_and(|s| s.seq == seq) {
            sign_ins.remove(&provider);
        }
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

/// Why a provider cannot be connected.
pub fn unavailable_reason(provider: ProviderId) -> String {
    match provider {
        ProviderId::Xing => xing::UNAVAILABLE.to_string(),
        _ => format!(
            "{} sign-in is not available in this build of ReMa.",
            provider.name()
        ),
    }
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

/// Account details from the ID token (and Microsoft Graph when needed).
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
    if provider == ProviderId::Microsoft && profile.email.is_none() {
        // Graph /me with the new token (User.Read).
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
        if let Ok(response) = me {
            if let Ok(me) = response.json::<Value>().await {
                profile.email = claim(&me, "mail").or_else(|| claim(&me, "userPrincipalName"));
                profile.name = profile.name.or_else(|| claim(&me, "displayName"));
                profile.account_id = profile.account_id.or_else(|| claim(&me, "id"));
            }
        }
    }
    profile
}

/// Runs a sign-in for a connector: the default browser shows the provider's
/// own account chooser and consent screen; ReMa waits for the loopback
/// redirect, checks `state`, exchanges the code with the PKCE verifier and
/// stores the tokens. `open_browser` opens a URL in the system browser.
pub async fn connect(
    state: &AppState,
    id: ConnectorId,
    open_browser: impl FnOnce(&str) -> AppResult<()>,
) -> AppResult<()> {
    let provider = id.provider();
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
    let (loopback, redirect_uri) = match provider {
        // Microsoft: `http://localhost` (registered without a port; any port
        // matches), listened for on IPv4 and IPv6 loopback.
        ProviderId::Microsoft => {
            let loopback = Loopback::dual_stack().await?;
            let uri = format!("http://localhost:{}", loopback.port());
            (loopback, uri)
        }
        // Google and LinkedIn (native clients): the loopback IP literal.
        _ => {
            let loopback = Loopback::ipv4().await?;
            let uri = format!("http://127.0.0.1:{}", loopback.port());
            (loopback, uri)
        }
    };
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
        ProviderId::Xing => return Err(unavailable_error(provider)),
    };
    let url = oauth::authorization_url(&AuthorizationRequest {
        endpoint,
        client_id: &app.client_id,
        redirect_uri: &redirect_uri,
        scopes: &scopes,
        challenge: &pkce.challenge,
        state: &csrf,
        extra,
    })?;

    let (seq, cancel) = state.connectors.begin_sign_in(provider, wanted.clone());
    state.events.connectors_changed();
    let result = async {
        open_browser(&url)?;
        let code = oauth::wait_for_code(loopback, &csrf, provider.name(), &cancel, SIGN_IN_TIMEOUT)
            .await?;
        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code),
            ("code_verifier", pkce.verifier.clone()),
            ("redirect_uri", redirect_uri.clone()),
        ];
        if provider == ProviderId::Microsoft {
            form.push(("scope", scopes.join(" ")));
        }
        let tokens = oauth::token_request(
            &state.connectors.http,
            state.connectors.token_url(provider),
            &app,
            provider.name(),
            form,
        )
        .await
        .map_err(|f| f.into_error(provider.name()))?;
        finish_sign_in(state, provider, &wanted, &scopes, tokens).await
    }
    .await;
    state.connectors.end_sign_in(provider, seq);
    state.events.connectors_changed();
    result
}

async fn finish_sign_in(
    state: &AppState,
    provider: ProviderId,
    connectors: &[ConnectorId],
    requested: &[String],
    tokens: TokenResponse,
) -> AppResult<()> {
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
    let previous = state.db.call(|c| repo::account(c, provider))?;
    let same_account = previous
        .as_ref()
        .is_some_and(|p| p.account_id.is_some() && p.account_id == profile.account_id);
    // A refresh token from the previous sign-in of the same account still works.
    let previous_refresh = match state.vault.get(&tokens::vault_account(provider)).await? {
        Some(Credential::OAuth { refresh_token, .. }) if same_account => refresh_token,
        _ => None,
    };
    tokens::store(state, provider, &tokens, previous_refresh).await?;
    let now = now_ms();
    state.db.call(|c| {
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
                account_id: profile.account_id.clone(),
                email: profile.email.clone(),
                display_name: profile.name.clone(),
                granted_scopes: granted.clone(),
                status: AccountStatus::Connected,
                status_reason: None,
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
    })?;
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

/// Every connector with its state, for Settings.
pub async fn overview(state: &AppState) -> AppResult<ConnectorsOverview> {
    let (records, accounts) = state.db.call(|c| {
        let mut accounts = Vec::new();
        for provider in ProviderId::ALL {
            accounts.push(repo::account(c, provider)?);
        }
        Ok((repo::connectors(c)?, accounts))
    })?;
    let mut connectors = Vec::new();
    for record in records {
        let provider = record.id.provider();
        let account = accounts[provider.index()].clone();
        let has_token = tokens::usable(state, provider).await?;
        connectors.push(status_of(
            &record,
            account.as_ref(),
            has_token,
            state.connectors.signing_in(provider),
            state.connectors.is_syncing(record.id),
            state.connectors.app(provider).is_some(),
        ));
    }
    Ok(ConnectorsOverview {
        connectors,
        background: crate::services::background::settings(state)?,
    })
}

/// The card for one connector.
pub fn status_of(
    record: &ConnectorRecord,
    account: Option<&AccountRecord>,
    has_token: bool,
    signing_in: Option<Vec<ConnectorId>>,
    syncing: bool,
    available: bool,
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

    let (state, message, detail) = if signing_in.is_some_and(|c| c.contains(&id)) {
        (
            ConnectorState::Connecting,
            Some(format!(
                "Finish signing in with {} in your browser.",
                provider.name()
            )),
            None,
        )
    } else if !record.enabled {
        if available {
            (ConnectorState::Disconnected, None, None)
        } else {
            (
                ConnectorState::Unavailable,
                Some(unavailable_reason(provider)),
                None,
            )
        }
    } else {
        match account {
            None => (
                ConnectorState::ReauthRequired,
                Some("Sign in again to use this connector.".into()),
                None,
            ),
            Some(a) if a.status == AccountStatus::ReauthRequired || !has_token => (
                ConnectorState::ReauthRequired,
                Some(format!(
                    "{} access was revoked or has expired. Reconnect to continue.",
                    provider.name()
                )),
                a.status_reason.clone(),
            ),
            Some(_) if !missing.is_empty() => (
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
            ),
            Some(_) if syncing => (ConnectorState::Syncing, None, None),
            Some(_) if record.last_error.is_some() => (
                ConnectorState::Error,
                record.last_error.clone(),
                record.last_error_detail.clone(),
            ),
            Some(_) => (ConnectorState::Connected, None, None),
        }
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
    }
}

#[cfg(test)]
mod tests;
