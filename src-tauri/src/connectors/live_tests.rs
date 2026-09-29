//! Live smoke tests against the real Google and Microsoft sign-in, through
//! ReMa's own connection runtime: connect in the system browser → the
//! access token is renewed with the refresh token → one mail read → one
//! calendar read → disconnect. Interactive: a person signs in when the
//! browser opens; ReMa never automates a consent screen.
//!
//! They never run by default and read only `REMA_LIVE_*` variables:
//!
//! ```sh
//! REMA_LIVE_GOOGLE_CLIENT_ID=… pnpm test:live:google
//! REMA_LIVE_MICROSOFT_CLIENT_ID=… pnpm test:live:microsoft
//! ```
//!
//! `REMA_LIVE_GOOGLE_CLIENT_SECRET` is optional (Google's Desktop clients
//! may be created with one). A test whose client ID is not set prints
//! `BLOCKED` and what it needs. Tokens stay in the test's in-memory
//! credential store; nothing is written to the system keychain, the
//! database or the log.

use std::sync::Arc;

use super::*;
use crate::{
    connectors::{
        calendar::CalendarProvider,
        mail::{MailProvider, MailQuery},
        oauth::OAuthApp,
        sync,
    },
    llm::fake::FakeLanguageModel,
    models::connectors::ConnectionState,
    state::testing,
};

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn blocked(check: &str, needs: &str) {
    println!("BLOCKED {check}: set {needs} to run it.");
}

fn open_browser(url: &str) -> AppResult<()> {
    println!("Opening the system browser for the sign-in (finish it there)…");
    open::that(url).map_err(|e| AppError::internal(format!("could not open the browser: {e}")))
}

async fn account_state(state: &AppState, provider: ProviderId) -> ProviderAccount {
    overview(state)
        .await
        .unwrap()
        .accounts
        .into_iter()
        .find(|a| a.provider == provider)
        .unwrap()
}

/// Connect → renew → read mail → read calendar → disconnect, for one
/// provider whose registration is in `apps`.
async fn smoke(provider: ProviderId, apps: Apps) {
    let (mut state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
    state.connectors = ConnectorsContext::new(
        GoogleEndpoints::from_env(),
        MicrosoftEndpoints::from_env(),
        apps,
    );
    let name = provider.name();

    println!("1. Connect {name} (browser)…");
    connect_provider(&state, provider, open_browser)
        .await
        .unwrap_or_else(|e| panic!("{name} sign-in failed: {e}"));
    let account = account_state(&state, provider).await;
    assert_eq!(account.state, ConnectionState::Connected, "{account:?}");
    println!(
        "   connected as {} ({})",
        account.email.as_deref().unwrap_or("?"),
        account
            .capabilities
            .iter()
            .filter(|c| c.granted)
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    println!("2. Renew the access token with the refresh token…");
    let first = tokens::get_valid_access_token(&state, provider)
        .await
        .unwrap();
    let renewed = tokens::refresh_access_token(&state, provider)
        .await
        .unwrap();
    assert_ne!(first, renewed, "the provider issued a new access token");
    let account = account_state(&state, provider).await;
    assert!(account.last_refreshed_at.is_some(), "renewal recorded");

    println!("3. Read mail (one search, 3 messages at most)…");
    let mail_id = ConnectorId::of(provider, ConnectorKind::Mail);
    let (mail, _): (Box<dyn MailProvider>, String) =
        sync::mail_client(&state, mail_id).await.unwrap();
    let messages = mail
        .search(&MailQuery::default(), 3)
        .await
        .unwrap_or_else(|e| panic!("mail search failed: {e}"));
    println!("   {} message(s) found", messages.len());

    println!("4. Read the calendar (the next 7 days)…");
    let calendar_id = ConnectorId::of(provider, ConnectorKind::Calendar);
    let calendar: Box<dyn CalendarProvider> =
        sync::calendar_client(&state, calendar_id).await.unwrap();
    let now = now_ms();
    let events = calendar
        .list_events(now, now + 7 * 24 * 60 * 60 * 1000)
        .await
        .unwrap_or_else(|e| panic!("calendar read failed: {e}"));
    println!("   {} event(s) in the next week", events.len());

    println!("5. Disconnect (revoke where the provider allows it)…");
    disconnect_provider(&state, provider).await.unwrap();
    let account = account_state(&state, provider).await;
    assert_eq!(account.state, ConnectionState::Disconnected);
    assert!(tokens::grant(&state, provider).await.is_none());
    println!("OK {name}: connect → renew → mail → calendar → disconnect");
}

#[tokio::test]
#[ignore = "interactive live sign-in; run with pnpm test:live:google"]
async fn google() {
    let Some(client_id) = var("REMA_LIVE_GOOGLE_CLIENT_ID") else {
        return blocked(
            "Google connect → renew → Gmail → Calendar",
            "REMA_LIVE_GOOGLE_CLIENT_ID (a Desktop OAuth client of a project with the Gmail \
             and Calendar APIs enabled; REMA_LIVE_GOOGLE_CLIENT_SECRET is optional)",
        );
    };
    smoke(
        ProviderId::Google,
        Apps {
            google: Some(OAuthApp {
                client_id,
                client_secret: var("REMA_LIVE_GOOGLE_CLIENT_SECRET"),
            }),
            ..Apps::default()
        },
    )
    .await;
}

#[tokio::test]
#[ignore = "interactive live sign-in; run with pnpm test:live:microsoft"]
async fn microsoft() {
    let Some(client_id) = var("REMA_LIVE_MICROSOFT_CLIENT_ID") else {
        return blocked(
            "Microsoft connect → renew → Outlook Mail → Outlook Calendar",
            "REMA_LIVE_MICROSOFT_CLIENT_ID (a public client, mobile and desktop platform, \
             redirect http://localhost, any org and personal accounts)",
        );
    };
    smoke(
        ProviderId::Microsoft,
        Apps {
            microsoft: Some(OAuthApp {
                client_id,
                client_secret: None,
            }),
            ..Apps::default()
        },
    )
    .await;
}
