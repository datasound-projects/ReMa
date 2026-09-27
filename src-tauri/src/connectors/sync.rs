//! Provider clients for connected accounts, and the rule that at most one
//! synchronization per connector runs at a time.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::{
    api::{ApiClient, ConnectorTokens},
    calendar::CalendarProvider,
    google::{calendar::GoogleCalendar, gmail::Gmail},
    mail::MailProvider,
    microsoft::{calendar::OutlookCalendar, mail::OutlookMail},
    ConnectorsContext,
};
use crate::{
    db::connectors::AccountRecord,
    error::{AppError, AppResult},
    models::connectors::{ConnectorId, ConnectorKind, ProviderId},
    state::AppState,
};

/// Holds a connector's single sync slot; released on drop.
pub struct SyncGuard {
    ctx: ConnectorsContext,
    id: ConnectorId,
    pub cancel: CancellationToken,
}

impl Drop for SyncGuard {
    fn drop(&mut self) {
        if let Ok(mut syncs) = self.ctx.syncs.lock() {
            syncs.remove(&self.id);
        }
    }
}

/// Claims the connector's sync slot; `None` if a sync is already running.
pub fn begin(ctx: &ConnectorsContext, id: ConnectorId) -> Option<SyncGuard> {
    let mut syncs = ctx.syncs.lock().ok()?;
    if syncs.contains_key(&id) {
        return None;
    }
    let cancel = ctx.shutdown.child_token();
    syncs.insert(id, cancel.clone());
    Some(SyncGuard {
        ctx: ctx.clone(),
        id,
        cancel,
    })
}

fn api(
    state: &AppState,
    provider: ProviderId,
    service: &'static str,
    connector: &'static str,
    base: &str,
) -> ApiClient {
    ApiClient::new(
        state.connectors.http.clone(),
        Arc::new(ConnectorTokens {
            state: state.clone(),
            provider,
        }),
        service,
        connector,
        base,
    )
}

fn account_key(account: &AccountRecord) -> String {
    account
        .account_id
        .clone()
        .or_else(|| account.email.clone())
        .unwrap_or_else(|| "me".into())
}

/// The mailbox of a connected mail connector.
pub async fn mail_client(
    state: &AppState,
    id: ConnectorId,
) -> AppResult<(Box<dyn MailProvider>, String)> {
    if id.kind() != ConnectorKind::Mail {
        return Err(AppError::internal("not a mail connector"));
    }
    let account = super::require(state, id).await?;
    let account_id = account_key(&account);
    let client: Box<dyn MailProvider> = match id.provider() {
        ProviderId::Google => Box::new(Gmail {
            api: api(
                state,
                ProviderId::Google,
                "Gmail",
                "Gmail",
                &state.connectors.google.gmail,
            ),
            account_id: account_id.clone(),
            email: account.email.clone(),
        }),
        ProviderId::Microsoft => Box::new(OutlookMail {
            api: api(
                state,
                ProviderId::Microsoft,
                "Outlook Mail",
                "Outlook Mail",
                &state.connectors.microsoft.graph,
            ),
            account_id: account_id.clone(),
        }),
    };
    Ok((client, account_id))
}

/// The primary calendar of a connected calendar connector.
pub async fn calendar_client(
    state: &AppState,
    id: ConnectorId,
) -> AppResult<Box<dyn CalendarProvider>> {
    if id.kind() != ConnectorKind::Calendar {
        return Err(AppError::internal("not a calendar connector"));
    }
    let account = super::require(state, id).await?;
    let email = account.email.clone().unwrap_or_default();
    Ok(match id.provider() {
        ProviderId::Google => Box::new(GoogleCalendar {
            api: api(
                state,
                ProviderId::Google,
                "Google Calendar",
                "Google Calendar",
                &state.connectors.google.calendar,
            ),
            calendar_id: email,
        }),
        ProviderId::Microsoft => Box::new(OutlookCalendar {
            api: api(
                state,
                ProviderId::Microsoft,
                "Outlook Calendar",
                "Outlook Calendar",
                &state.connectors.microsoft.graph,
            ),
            email,
        }),
    })
}

/// Every calendar that can be used right now.
pub async fn ready_calendars(state: &AppState) -> Vec<Box<dyn CalendarProvider>> {
    let mut calendars = Vec::new();
    for id in super::ready(state, ConnectorKind::Calendar).await {
        if let Ok(client) = calendar_client(state, id).await {
            calendars.push(client);
        }
    }
    calendars
}
