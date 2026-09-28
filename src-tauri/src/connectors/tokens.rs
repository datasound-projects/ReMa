//! The token manager: the only code that touches connector tokens.
//!
//! A connected account's grant lives in the OS credential store under
//! `connector:<provider>:<account id>` (the provider plus its stable account
//! id, never the email address): the refresh token for Google and
//! Microsoft; for LinkedIn, which issues no refresh tokens, its access token.
//! Google and Microsoft access tokens are kept in memory only and renewed
//! silently; after a restart the first request refreshes. Concurrent
//! requests for one account share a single refresh. Nothing here reaches
//! SQLite, settings, logs, the frontend or a model's context.
//!
//! When a refresh is rejected (revoked consent, password change, expired
//! refresh token) the account becomes "reauth_required" and stays so until
//! the user clicks Reconnect: ReMa never opens a sign-in on its own. A
//! provider that cannot be reached leaves the grant alone.

use super::{
    failure::{self, TokenPhase},
    oauth,
    oauth::TokenResponse,
};
use crate::{
    db::connectors::{self as repo, AccountStatus},
    error::{AppError, AppResult},
    models::connectors::{ConnectorErrorCode, ProviderId},
    secrets::Credential,
    state::AppState,
    time::now_ms,
};

/// An access token is renewed this long before it expires.
pub const EXPIRY_MARGIN_MS: i64 = 5 * 60_000;

/// How often connected accounts are renewed while ReMa runs. A grant that
/// is never used expires: Microsoft after 90 days, Google after about six
/// months; one renewal a day (and one at start) keeps it alive.
pub const KEEP_ALIVE_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Keeps connected Google and Microsoft accounts signed in: renews each
/// one's grant (no mail or calendar is read), so a connection made once
/// keeps working. A grant the provider rejects marks its account
/// "Reconnect needed" as any request would; ReMa never opens a sign-in.
pub async fn keep_alive(state: &AppState) {
    for provider in [ProviderId::Google, ProviderId::Microsoft] {
        let connected = state
            .db
            .call(|c| repo::account(c, provider))
            .ok()
            .flatten()
            .is_some_and(|a| a.status == AccountStatus::Connected);
        if !connected || state.connectors.app(provider).is_none() {
            continue;
        }
        match get_valid_access_token(state, provider).await {
            Ok(_) => super::diag(format!(
                "[connector] provider={} keep_alive=ok",
                provider.as_str()
            )),
            Err(error) => super::diag(format!(
                "[connector] provider={} keep_alive=failed category={:?}",
                provider.as_str(),
                error.code()
            )),
        }
    }
}

/// Runs [`keep_alive`] shortly after start and then once a day, until
/// ReMa quits.
pub fn start_keep_alive(state: AppState) {
    tauri::async_runtime::spawn(async move {
        let stop = state.connectors.shutdown.clone();
        let mut wait = std::time::Duration::from_secs(60);
        loop {
            tokio::select! {
                _ = stop.cancelled() => return,
                _ = tokio::time::sleep(wait) => {}
            }
            keep_alive(&state).await;
            wait = KEEP_ALIVE_EVERY;
        }
    });
}

/// The credential-store key of one connected account.
pub fn vault_key(provider: ProviderId, account_id: &str) -> String {
    format!("connector:{}:{account_id}", provider.as_str())
}

/// The provider-wide key grants were stored under before per-account keys.
/// A grant found there moves to its account's key when it is first used.
pub fn legacy_key(provider: ProviderId) -> String {
    format!("connector:{}", provider.as_str())
}

/// The key of the provider's connected account (the legacy key while the
/// account has no stable id).
fn key_of(state: &AppState, provider: ProviderId) -> AppResult<String> {
    let account = state.db.call(|c| repo::account(c, provider))?;
    Ok(match account.and_then(|a| a.account_id) {
        Some(id) => vault_key(provider, &id),
        None => legacy_key(provider),
    })
}

/// The stored grant of the provider's account and its key. A grant under
/// the legacy key moves to the account's key; a still-valid access token it
/// held is kept in memory.
async fn load(state: &AppState, provider: ProviderId) -> AppResult<Option<(String, Credential)>> {
    let key = key_of(state, provider)?;
    if let Some(credential) = state.vault.get(&key).await? {
        return Ok(Some((key, credential)));
    }
    let legacy = legacy_key(provider);
    if key == legacy {
        return Ok(None);
    }
    let Some(old) = state.vault.get(&legacy).await? else {
        return Ok(None);
    };
    let grant = match old {
        Credential::OAuth {
            access_token,
            refresh_token: Some(refresh_token),
            expires_at,
        } if super::issues_refresh_tokens(provider) => {
            if let Some(at) = expires_at {
                state.connectors.cache_access(&key, access_token, at);
            }
            Credential::RefreshToken { refresh_token }
        }
        other => other,
    };
    state.vault.set(&key, grant.clone()).await?;
    state.vault.delete(&legacy).await?;
    super::diag(format!(
        "[connector] provider={} credential=moved_to_account_key",
        provider.as_str()
    ));
    Ok(Some((key, grant)))
}

/// The refresh token of the provider's current grant, if it has one.
pub async fn current_refresh_token(
    state: &AppState,
    provider: ProviderId,
) -> AppResult<Option<String>> {
    Ok(match load(state, provider).await? {
        Some((_, Credential::RefreshToken { refresh_token })) => Some(refresh_token),
        Some((_, Credential::OAuth { refresh_token, .. })) => refresh_token,
        _ => None,
    })
}

/// Stores the grant of a sign-in for `account_id`. Google and Microsoft keep
/// only the refresh token (`previous_refresh` stands in when the provider
/// issued none this time); the access token goes to memory. LinkedIn's
/// access token is its grant.
pub async fn store(
    state: &AppState,
    provider: ProviderId,
    account_id: &str,
    tokens: &TokenResponse,
    previous_refresh: Option<String>,
) -> AppResult<()> {
    let key = vault_key(provider, account_id);
    let expires_at = now_ms() + tokens.expires_in.unwrap_or(3_600) * 1000;
    let refresh_token = tokens.refresh_token.clone().or(previous_refresh);
    let grant = if super::issues_refresh_tokens(provider) {
        let Some(refresh_token) = refresh_token else {
            return Err(AppError::authentication(format!(
                "{} did not grant lasting access. Try connecting again.",
                provider.name()
            )));
        };
        Credential::RefreshToken { refresh_token }
    } else {
        Credential::OAuth {
            access_token: tokens.access_token.clone(),
            refresh_token,
            expires_at: tokens.expires_in.map(|_| expires_at),
        }
    };
    state.vault.set(&key, grant).await?;
    state
        .connectors
        .cache_access(&key, tokens.access_token.clone(), expires_at);
    Ok(())
}

fn reconnect_error(provider: ProviderId) -> AppError {
    AppError::authentication(format!(
        "{} access was revoked or has expired. Reconnect in Settings → Connectors.",
        provider.name()
    ))
}

/// A valid access token for the provider's account: from memory, or renewed
/// once for every caller waiting at the same time.
pub async fn get_valid_access_token(state: &AppState, provider: ProviderId) -> AppResult<String> {
    let key = key_of(state, provider)?;
    if let Some(token) = state.connectors.cached_access(&key) {
        return Ok(token);
    }
    let lock = state.connectors.refresh_lock(&key);
    let _guard = lock.lock().await;
    // Another caller may have renewed it while this one waited.
    if let Some(token) = state.connectors.cached_access(&key) {
        return Ok(token);
    }
    renew(state, provider).await
}

/// Renews the access token after the API rejected it, unless a concurrent
/// caller already did.
pub async fn refresh_access_token(state: &AppState, provider: ProviderId) -> AppResult<String> {
    let key = key_of(state, provider)?;
    let rejected = state.connectors.cached_access(&key);
    let lock = state.connectors.refresh_lock(&key);
    let _guard = lock.lock().await;
    if let Some(token) = state.connectors.cached_access(&key) {
        if Some(&token) != rejected.as_ref() {
            return Ok(token);
        }
    }
    state.connectors.forget_access(&key);
    renew(state, provider).await
}

async fn renew(state: &AppState, provider: ProviderId) -> AppResult<String> {
    let loaded = load(state, provider).await?;
    // Moving a grant from the legacy key keeps its valid access token.
    if let Some(token) = loaded
        .as_ref()
        .and_then(|(key, _)| state.connectors.cached_access(key))
    {
        return Ok(token);
    }
    match loaded {
        Some((key, Credential::RefreshToken { refresh_token })) => {
            refresh_with(state, provider, &key, refresh_token).await
        }
        // An access token stored with the grant (LinkedIn, or a grant from
        // before refresh-token-only storage) is used while it is valid.
        Some((
            key,
            Credential::OAuth {
                access_token,
                refresh_token,
                expires_at,
            },
        )) => {
            if expires_at.is_none_or(|at| at > now_ms() + EXPIRY_MARGIN_MS) {
                if let Some(at) = expires_at {
                    state
                        .connectors
                        .cache_access(&key, access_token.clone(), at);
                }
                return Ok(access_token);
            }
            match refresh_token {
                Some(refresh_token) => refresh_with(state, provider, &key, refresh_token).await,
                None => {
                    mark_reauth_required(state, provider, "the access token expired").await?;
                    Err(reconnect_error(provider))
                }
            }
        }
        _ => Err(reconnect_error(provider)),
    }
}

async fn refresh_with(
    state: &AppState,
    provider: ProviderId,
    key: &str,
    refresh_token: String,
) -> AppResult<String> {
    let ctx = &state.connectors;
    let app = ctx
        .app(provider)
        .ok_or_else(|| super::unavailable_error(provider))?;
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.clone()),
    ];
    if provider == ProviderId::Microsoft {
        // Microsoft wants the scopes again; the grant decides what is issued.
        let scopes = state
            .db
            .call(|c| repo::account(c, provider))?
            .map(|a| a.granted_scopes)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| super::microsoft::scopes(&[]));
        let mut scopes: Vec<String> = scopes
            .into_iter()
            .map(|s| {
                s.trim_start_matches("https://graph.microsoft.com/")
                    .to_string()
            })
            .collect();
        if !scopes.iter().any(|s| s == "offline_access") {
            scopes.push("offline_access".into());
        }
        form.push(("scope", scopes.join(" ")));
    }
    match oauth::token_request(
        &ctx.http,
        ctx.token_url(provider),
        &app,
        provider.name(),
        TokenPhase::Refresh,
        form,
    )
    .await
    {
        Ok(tokens) => {
            // Microsoft rotates refresh tokens; an old-style grant becomes a
            // refresh-token grant. Otherwise the stored grant is unchanged.
            let rotated = tokens.refresh_token.clone().filter(|t| *t != refresh_token);
            let converting = matches!(state.vault.get(key).await?, Some(Credential::OAuth { .. }))
                && super::issues_refresh_tokens(provider);
            if rotated.is_some() || converting {
                state
                    .vault
                    .set(
                        key,
                        Credential::RefreshToken {
                            refresh_token: rotated.unwrap_or(refresh_token),
                        },
                    )
                    .await?;
            }
            let expires_at = now_ms() + tokens.expires_in.unwrap_or(3_600) * 1000;
            ctx.cache_access(key, tokens.access_token.clone(), expires_at);
            oauth::log(provider.name(), "token_refreshed", "");
            Ok(tokens.access_token)
        }
        Err(failure) if failure.code == ConnectorErrorCode::ReauthRequired => {
            oauth::log(
                provider.name(),
                "refresh_rejected",
                "category=REAUTH_REQUIRED",
            );
            let reason = failure.detail.unwrap_or(failure.message);
            mark_reauth_required(state, provider, &reason).await?;
            Err(reconnect_error(provider))
        }
        Err(failure) => {
            oauth::log(
                provider.name(),
                "refresh_failed",
                &format!("category={}", failure.code.as_str()),
            );
            Err(failure.into_error())
        }
    }
}

/// Deletes an account's grant and its access token in memory.
pub async fn forget_account(
    state: &AppState,
    provider: ProviderId,
    account_id: Option<&str>,
) -> AppResult<()> {
    let key = match account_id {
        Some(id) => vault_key(provider, id),
        None => legacy_key(provider),
    };
    state.connectors.forget_access(&key);
    state.vault.delete(&key).await?;
    let legacy = legacy_key(provider);
    if key != legacy {
        state.vault.delete(&legacy).await?;
    }
    Ok(())
}

/// The grant is gone: forget it, keep the account visible and ask the user
/// to reconnect (once, as a notification).
pub async fn mark_reauth_required(
    state: &AppState,
    provider: ProviderId,
    reason: &str,
) -> AppResult<()> {
    let account = state.db.call(|c| repo::account(c, provider))?;
    let account_id = account.as_ref().and_then(|a| a.account_id.clone());
    forget_account(state, provider, account_id.as_deref()).await?;
    if provider == ProviderId::Linkedin {
        state.network.forget_provider_data();
    }
    // Why the provider ended it, for a card that says so (and whose setting
    // it is) instead of a generic "revoked or expired".
    let now = now_ms();
    let age = account.as_ref().map(|a| now - a.connected_at);
    let testing = state.connectors.google_in_testing();
    let cause = failure::reauth_cause(provider, reason, age, testing);
    let reason: String = reason.chars().take(200).collect();
    state.db.call(|c| {
        repo::set_account_status(
            c,
            provider,
            AccountStatus::ReauthRequired,
            Some(&reason),
            cause,
            now,
        )
    })?;
    super::diag(format!(
        "[connector] provider={} state=reauth_required cause={}",
        provider.as_str(),
        cause.map_or("unknown", |c| c.as_str())
    ));
    crate::services::notifications::add(
        state,
        crate::services::notifications::Notice {
            kind: "connector",
            title: format!("Reconnect {}", provider.name()),
            body: format!(
                "{} Open Settings → Connectors and click Reconnect.",
                failure::reauth_message(provider, cause, age, testing)
            ),
            application_id: None,
            interview_id: None,
            dedupe_key: Some(format!(
                "reauth:{}:{}",
                provider.as_str(),
                now_ms() / 86_400_000
            )),
        },
    );
    state.events.connectors_changed();
    Ok(())
}

/// Revokes ReMa's access at the provider (where it offers revocation) and
/// deletes the stored grant. Local deletion happens even if the provider
/// cannot be reached.
pub async fn revoke_connection(state: &AppState, provider: ProviderId) -> AppResult<()> {
    let grant = load(state, provider).await?;
    if provider == ProviderId::Google {
        let token = match grant {
            Some((_, Credential::RefreshToken { refresh_token })) => Some(refresh_token),
            Some((
                _,
                Credential::OAuth {
                    access_token,
                    refresh_token,
                    ..
                },
            )) => Some(refresh_token.unwrap_or(access_token)),
            _ => None,
        };
        if let Some(token) = token {
            let revoked = revoke_google(state, &token).await;
            oauth::log(
                "Google",
                if revoked.is_ok() {
                    "revoked"
                } else {
                    "revoke_failed"
                },
                "",
            );
        }
        // Microsoft and LinkedIn have no token revocation for public
        // clients; the user can remove ReMa in their account settings
        // (shown in the UI).
    }
    let account_id = state
        .db
        .call(|c| repo::account(c, provider))?
        .and_then(|a| a.account_id);
    forget_account(state, provider, account_id.as_deref()).await
}

async fn revoke_google(state: &AppState, token: &str) -> AppResult<()> {
    let ctx = &state.connectors;
    let response = ctx
        .http
        .post(&ctx.google.revoke)
        .form(&[("token", token)])
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| AppError::provider("Google could not be reached to revoke access."))?;
    // 400: the token was already invalid; nothing left to revoke.
    if response.status().is_success() || response.status().as_u16() == 400 {
        Ok(())
    } else {
        Err(AppError::provider(format!(
            "Google could not revoke access ({}).",
            response.status().as_u16()
        )))
    }
}

/// Whether the stored grant can still be used: refreshable, or an access
/// token that has not expired (LinkedIn issues no refresh token).
pub async fn usable(state: &AppState, provider: ProviderId) -> AppResult<bool> {
    Ok(match load(state, provider).await? {
        Some((_, Credential::RefreshToken { .. }))
        | Some((
            _,
            Credential::OAuth {
                refresh_token: Some(_),
                ..
            },
        )) => true,
        Some((_, Credential::OAuth { expires_at, .. })) => {
            expires_at.is_none_or(|at| at > now_ms())
        }
        _ => false,
    })
}

/// Whether the user must sign in again before this provider can be used.
pub async fn requires_reauthentication(state: &AppState, provider: ProviderId) -> AppResult<bool> {
    let account = state.db.call(|c| repo::account(c, provider))?;
    Ok(match account {
        Some(account) => {
            account.status == AccountStatus::ReauthRequired || !usable(state, provider).await?
        }
        None => false,
    })
}

/// The stored grant (tests).
#[cfg(test)]
pub async fn grant(state: &AppState, provider: ProviderId) -> Option<Credential> {
    load(state, provider).await.unwrap().map(|(_, c)| c)
}
