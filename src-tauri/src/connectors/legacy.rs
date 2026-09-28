//! Moves the old "Google Workspace" settings (one Google sign-in with an
//! optional user-entered OAuth client) to connectors. Runs once at startup.
//!
//! - Tokens issued to ReMa's built-in Google client keep working: they move to
//!   the connector token entry and Gmail / Google Calendar stay connected.
//! - Tokens issued to a user-entered OAuth client cannot be refreshed by
//!   ReMa's own client: they are revoked (best effort) and deleted, and the
//!   connectors ask the user to reconnect. The user-entered client ID and
//!   secret are deleted; users no longer configure OAuth clients.

use super::{google, tokens};
use crate::{
    db::{
        connectors::{self as repo, AccountRecord, AccountStatus},
        providers as settings,
    },
    error::AppResult,
    models::connectors::{ConnectorId, ProviderId},
    secrets::Credential,
    state::AppState,
    time::now_ms,
};

const OLD_TOKEN: &str = "google";
const OLD_CLIENT_SECRET: &str = "google-oauth-client";
const KEY_CLIENT_ID: &str = "google.client_id";
const KEY_EMAIL: &str = "google.email";
const KEY_SCOPES: &str = "google.scopes";
const KEY_NEEDS_RECONNECT: &str = "google.needs_reconnect";
const KEY_GMAIL_ENABLED: &str = "google.gmail_enabled";
const KEY_CALENDAR_ENABLED: &str = "google.calendar_enabled";
const OLD_KEYS: [&str; 6] = [
    KEY_CLIENT_ID,
    KEY_EMAIL,
    KEY_SCOPES,
    KEY_NEEDS_RECONNECT,
    KEY_GMAIL_ENABLED,
    KEY_CALENDAR_ENABLED,
];

pub async fn migrate(state: &AppState) -> AppResult<()> {
    let old = state.db.call(|c| {
        let mut values = Vec::new();
        for key in OLD_KEYS {
            values.push(settings::get_setting(c, key)?);
        }
        Ok(values)
    })?;
    let [client_id, email, scopes, needs_reconnect, gmail_on, calendar_on] =
        <[Option<String>; 6]>::try_from(old).expect("six keys");
    let old_token = state.vault.get(OLD_TOKEN).await?;
    if client_id.is_none()
        && email.is_none()
        && scopes.is_none()
        && needs_reconnect.is_none()
        && old_token.is_none()
    {
        return Ok(()); // nothing to move (fresh install or already migrated)
    }

    let granted: Vec<String> = scopes
        .as_deref()
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    let flag = |v: &Option<String>| v.as_deref() != Some("0");
    let mut connectors = Vec::new();
    if flag(&gmail_on) && granted.iter().any(|s| s == google::SCOPE_GMAIL_READONLY) {
        connectors.push(ConnectorId::Gmail);
    }
    if flag(&calendar_on) && granted.iter().any(|s| s == google::SCOPE_CALENDAR_EVENTS) {
        connectors.push(ConnectorId::GoogleCalendar);
    }
    let custom_client = client_id.is_some();
    let reusable = !custom_client
        && matches!(old_token, Some(Credential::OAuth { .. }))
        && state.connectors.app(ProviderId::Google).is_some();
    let now = now_ms();

    if reusable {
        if let Some(credential) = old_token.clone() {
            state
                .vault
                // Moves to the account's own key when it is first used.
                .set(&tokens::legacy_key(ProviderId::Google), credential)
                .await?;
        }
    } else if let Some(Credential::OAuth {
        access_token,
        refresh_token,
        ..
    }) = &old_token
    {
        // Not usable with ReMa's own app: end that grant at Google.
        let token = refresh_token
            .clone()
            .unwrap_or_else(|| access_token.clone());
        let _ = state
            .connectors
            .http
            .post(&state.connectors.google.revoke)
            .form(&[("token", token.as_str())])
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;
    }

    let had_connection = old_token.is_some() || needs_reconnect.is_some();
    state.db.call(|c| {
        let tx = c.transaction()?;
        if had_connection && !connectors.is_empty() {
            repo::save_account(
                &tx,
                &AccountRecord {
                    provider: ProviderId::Google,
                    account_id: None,
                    email: email.clone(),
                    display_name: None,
                    granted_scopes: granted.clone(),
                    status: if reusable {
                        AccountStatus::Connected
                    } else {
                        AccountStatus::ReauthRequired
                    },
                    status_reason: (!reusable).then(|| {
                        "ReMa now connects with its own Google sign-in. Reconnect to continue."
                            .to_string()
                    }),
                    status_cause: None,
                    connected_at: now,
                    updated_at: now,
                },
            )?;
            for id in &connectors {
                repo::set_enabled(&tx, *id, true, now)?;
            }
        }
        for key in OLD_KEYS {
            settings::delete_setting(&tx, key)?;
        }
        tx.commit()?;
        Ok(())
    })?;
    state.vault.delete(OLD_TOKEN).await?;
    state.vault.delete(OLD_CLIENT_SECRET).await?;
    state.events.connectors_changed();
    Ok(())
}
