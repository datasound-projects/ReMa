//! Connection checks after a sign-in (Spec B §47–§49): tokens alone never
//! make a connector "Connected". Each connector makes one small request with
//! the new access token — Gmail's profile, one upcoming Google Calendar
//! event, one Outlook message id, the Outlook calendar — which reads no mail
//! content and no event details. A missing permission, an API that is not
//! enabled or an account without a mailbox is told apart from a provider
//! that could not be reached (which leaves the connector connected; the next
//! use tries again).

use std::time::Duration;

use super::failure::{self, Failure};
use crate::{
    models::connectors::{ConnectorErrorCode, ConnectorId},
    state::AppState,
    time::now_ms,
};

/// What a connector's check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Passed,
    Failed(Failure),
    /// The provider could not be reached: nothing is concluded.
    Skipped(String),
}

/// The request that proves a connector works (none for networks, whose
/// identity the sign-in already checked).
fn probe(state: &AppState, id: ConnectorId) -> Option<String> {
    let ctx = &state.connectors;
    match id {
        ConnectorId::Gmail => Some(format!("{}/users/me/profile", ctx.google.gmail)),
        ConnectorId::GoogleCalendar => {
            let now = jiff::Timestamp::from_millisecond(now_ms())
                .map(|t| t.to_string())
                .unwrap_or_default();
            Some(format!(
                "{}/calendars/primary/events?maxResults=1&singleEvents=true&fields=kind&timeMin={}",
                ctx.google.calendar,
                urlencode(&now)
            ))
        }
        ConnectorId::OutlookMail => Some(format!(
            "{}/me/messages?$top=1&$select=id",
            ctx.microsoft.graph
        )),
        ConnectorId::OutlookCalendar => {
            Some(format!("{}/me/calendar?$select=id", ctx.microsoft.graph))
        }
        ConnectorId::Linkedin | ConnectorId::Xing => None,
    }
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Checks one connector with the account's new access token.
pub async fn connector(state: &AppState, id: ConnectorId, access_token: &str) -> Check {
    let Some(url) = probe(state, id) else {
        return Check::Passed;
    };
    let provider = id.provider().name();
    let response = state
        .connectors
        .http
        .get(&url)
        .bearer_auth(access_token)
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    let check = match response {
        Err(error) => Check::Skipped(if error.is_timeout() {
            "timed out".into()
        } else {
            "unreachable".into()
        }),
        Ok(response) if response.status().is_success() => Check::Passed,
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            let failure = failure::from_api(provider, id.name(), status, &body);
            if failure.code == ConnectorErrorCode::NetworkError {
                Check::Skipped(format!("{provider} answered {status}"))
            } else {
                Check::Failed(failure)
            }
        }
    };
    let capability = id.as_str();
    let provider_key = id.provider().as_str();
    match &check {
        Check::Passed => super::diag(format!(
            "[connector] provider={provider_key} capability={capability} validation=success"
        )),
        Check::Failed(failure) => super::diag(format!(
            "[connector] provider={provider_key} capability={capability} validation=failure category={}",
            failure.code.as_str()
        )),
        Check::Skipped(reason) => super::diag(format!(
            "[connector] provider={provider_key} capability={capability} validation=skipped reason={}",
            reason.replace(' ', "_")
        )),
    }
    check
}
