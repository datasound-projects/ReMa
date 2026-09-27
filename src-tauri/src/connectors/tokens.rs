//! The token manager: the only code that touches connector tokens.
//!
//! Tokens live in the OS credential store (and in Rust memory) under
//! `connector:<provider>`; never in SQLite, settings, logs, the frontend or a
//! model's context. Access tokens are refreshed silently before they expire.
//! When a refresh is rejected (revoked consent, password change, expired
//! refresh token) the account becomes "reauth_required" and stays so until
//! the user clicks Reconnect: ReMa never opens a sign-in on its own.

use super::{oauth, oauth::TokenResponse, ConnectorsContext};
use crate::{
    db::connectors::{self as repo, AccountStatus},
    error::{AppError, AppResult},
    models::connectors::ProviderId,
    secrets::Credential,
    state::AppState,
    time::now_ms,
};

/// Refresh this long before an access token expires.
const EXPIRY_MARGIN_MS: i64 = 5 * 60_000;

/// The credential-store account for a provider's tokens.
pub fn vault_account(provider: ProviderId) -> String {
    format!("connector:{}", provider.as_str())
}

/// Stores tokens from a sign-in or refresh. A refresh response without a
/// refresh token keeps the previous one; Microsoft rotates refresh tokens,
/// so a new one always replaces the old.
pub async fn store(
    state: &AppState,
    provider: ProviderId,
    tokens: &TokenResponse,
    previous_refresh: Option<String>,
) -> AppResult<()> {
    let refresh_token = tokens.refresh_token.clone().or(previous_refresh);
    if refresh_token.is_none() {
        return Err(AppError::authentication(format!(
            "{} did not grant long-lived access. Try connecting again.",
            provider.name()
        )));
    }
    state
        .vault
        .set(
            &vault_account(provider),
            Credential::OAuth {
                access_token: tokens.access_token.clone(),
                refresh_token,
                expires_at: tokens.expires_in.map(|s| now_ms() + s * 1000),
            },
        )
        .await
}

fn reconnect_error(provider: ProviderId) -> AppError {
    AppError::authentication(format!(
        "{} access was revoked or has expired. Reconnect in Settings → Connectors.",
        provider.name()
    ))
}

/// A valid access token, refreshed silently when it is about to expire.
pub async fn get_valid_access_token(state: &AppState, provider: ProviderId) -> AppResult<String> {
    let _guard = state.connectors.refresh_lock(provider).lock().await;
    match state.vault.get(&vault_account(provider)).await? {
        Some(Credential::OAuth {
            access_token,
            expires_at,
            ..
        }) if expires_at.is_some_and(|at| at > now_ms() + EXPIRY_MARGIN_MS) => Ok(access_token),
        Some(Credential::OAuth {
            refresh_token: Some(refresh_token),
            ..
        }) => refresh_with(state, provider, refresh_token).await,
        _ => Err(reconnect_error(provider)),
    }
}

/// Forces a refresh (e.g. after the API rejected the current token).
pub async fn refresh_access_token(state: &AppState, provider: ProviderId) -> AppResult<String> {
    let _guard = state.connectors.refresh_lock(provider).lock().await;
    match state.vault.get(&vault_account(provider)).await? {
        Some(Credential::OAuth {
            refresh_token: Some(refresh_token),
            ..
        }) => refresh_with(state, provider, refresh_token).await,
        _ => Err(reconnect_error(provider)),
    }
}

async fn refresh_with(
    state: &AppState,
    provider: ProviderId,
    refresh_token: String,
) -> AppResult<String> {
    let ctx: &ConnectorsContext = &state.connectors;
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
        form,
    )
    .await
    {
        Ok(tokens) => {
            let access = tokens.access_token.clone();
            store(state, provider, &tokens, Some(refresh_token)).await?;
            Ok(access)
        }
        Err(oauth::TokenFailure::Reauthenticate(reason)) => {
            mark_reauth_required(state, provider, &reason).await?;
            Err(reconnect_error(provider))
        }
        Err(oauth::TokenFailure::Other(error)) => Err(error),
    }
}

/// The grant is gone: forget the tokens, keep the account visible and ask
/// the user to reconnect (once, as a notification).
pub async fn mark_reauth_required(
    state: &AppState,
    provider: ProviderId,
    reason: &str,
) -> AppResult<()> {
    state.vault.delete(&vault_account(provider)).await?;
    let reason: String = reason.chars().take(200).collect();
    state.db.call(|c| {
        repo::set_account_status(
            c,
            provider,
            AccountStatus::ReauthRequired,
            Some(&reason),
            now_ms(),
        )
    })?;
    crate::services::notifications::add(
        state,
        crate::services::notifications::Notice {
            kind: "connector",
            title: format!("Reconnect {}", provider.name()),
            body: format!(
                "{} access was revoked or has expired. Open Settings → Connectors and click \
                 Reconnect to continue syncing.",
                provider.name()
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
/// deletes the stored tokens. Local deletion happens even if the provider
/// cannot be reached.
pub async fn revoke_connection(state: &AppState, provider: ProviderId) -> AppResult<()> {
    if let Some(Credential::OAuth {
        access_token,
        refresh_token,
        ..
    }) = state.vault.get(&vault_account(provider)).await?
    {
        if provider == ProviderId::Google {
            let token = refresh_token.unwrap_or(access_token);
            let _ = revoke_google(&state.connectors, &token).await;
        }
        // Microsoft has no token revocation endpoint for public clients; the
        // user can remove ReMa at account.microsoft.com (shown in the UI).
    }
    state.vault.delete(&vault_account(provider)).await
}

async fn revoke_google(ctx: &ConnectorsContext, token: &str) -> AppResult<()> {
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

/// Whether the user must sign in again before this provider can be used.
pub async fn requires_reauthentication(state: &AppState, provider: ProviderId) -> AppResult<bool> {
    let account = state.db.call(|c| repo::account(c, provider))?;
    let has_token = matches!(
        state.vault.get(&vault_account(provider)).await?,
        Some(Credential::OAuth { .. })
    );
    Ok(match account {
        Some(account) => account.status == AccountStatus::ReauthRequired || !has_token,
        None => false,
    })
}
