//! Account connections: browser sign-in, checks and sign-out through the
//! providers' official runtimes (see `crate::accounts`).
//!
//! A sign-in runs in the background. Its progress (connecting, opening the
//! browser, waiting for authorization, cancelled, failed) is part of the
//! provider's view and every change emits `ProvidersChanged`. When the
//! provider's page reports success, ReMa reads who signed in, lists the
//! models this connection offers and saves the connection.

use std::sync::Arc;

use crate::{
    accounts::{AccountRuntime, RuntimeStatus, SignInOutcome},
    error::{AppError, AppResult},
    llm::Endpoint,
    models::provider::{
        ConnectionMethod, ConnectionStatus, ProviderKind, ProviderView, SignInStatus,
    },
    services::providers,
    state::AppState,
};

fn account_runtime(
    state: &AppState,
    kind: ProviderKind,
) -> AppResult<(ConnectionMethod, Arc<dyn AccountRuntime>)> {
    let method = kind.account_method().ok_or_else(|| {
        AppError::validation(format!("{} has no account sign-in.", kind.display_name()))
    })?;
    let runtime = state
        .accounts
        .runtime(method)
        .ok_or_else(|| AppError::internal("account runtime missing"))?;
    Ok((method, runtime))
}

/// Starts the browser sign-in for a provider's account. Returns at once
/// with the sign-in in progress; `open_url` opens a page in the system
/// browser.
pub async fn start_sign_in(
    state: &AppState,
    kind: ProviderKind,
    device_code: bool,
    open_url: impl Fn(&str) -> AppResult<()>,
) -> AppResult<ProviderView> {
    let (method, runtime) = account_runtime(state, kind)?;
    let id = kind.as_str();
    let seq = state.accounts.begin(id, method);
    state.events.providers_changed();

    let fail = |message: String| {
        state.accounts.update(id, seq, |v| {
            v.status = SignInStatus::Failed;
            v.message = Some(message);
        });
        state.events.providers_changed();
        providers::provider_view(state, id)
    };

    let status = match runtime.status().await {
        Ok(status) => status,
        Err(error) => return fail(error.to_string()),
    };
    match status {
        RuntimeStatus::NotInstalled(message) => return fail(message),
        // Already signed in (e.g. after "Disconnect"): connect without a browser.
        RuntimeStatus::SignedIn { label } => {
            return match complete(state, kind, method, label).await {
                Ok(()) => {
                    state.accounts.finish(id, seq);
                    state.events.providers_changed();
                    providers::provider_view(state, id)
                }
                Err(error) => fail(error.to_string()),
            };
        }
        RuntimeStatus::SignedOut | RuntimeStatus::Expired => {}
    }

    let device_code = device_code && runtime.supports_device_code();
    let attempt = match runtime.start_sign_in(device_code).await {
        Ok(attempt) => attempt,
        Err(error) => return fail(error.to_string()),
    };
    state.accounts.set_cancel(id, seq, attempt.cancel.clone());

    if let Some(url) = &attempt.open_url {
        state.accounts.update(id, seq, |v| {
            v.status = SignInStatus::OpeningBrowser;
            if device_code {
                v.user_code = attempt.user_code.clone();
                v.verification_url = Some(url.clone());
            }
        });
        state.events.providers_changed();
        if let Err(error) = open_url(url) {
            attempt.cancel.cancel();
            return fail(format!("ReMa could not open your browser: {error}"));
        }
    }
    state.accounts.update(id, seq, |v| {
        v.status = SignInStatus::WaitingForAuthorization;
    });
    state.events.providers_changed();

    let background = state.clone();
    let outcome = attempt.outcome;
    tauri::async_runtime::spawn(async move {
        let state = background;
        let result = match outcome.await {
            SignInOutcome::Succeeded => {
                // Who signed in, as the runtime reports it.
                let label = match runtime.status().await {
                    Ok(RuntimeStatus::SignedIn { label }) => Ok(label),
                    Ok(_) => Err("The sign-in did not complete.".to_string()),
                    Err(error) => Err(error.to_string()),
                };
                match label {
                    Ok(label) => complete(&state, kind, method, label)
                        .await
                        .map_err(|e| e.to_string()),
                    Err(message) => Err(message),
                }
            }
            SignInOutcome::Cancelled => {
                state.accounts.update(id, seq, |v| {
                    v.status = SignInStatus::Cancelled;
                    v.message = None;
                });
                state.events.providers_changed();
                return;
            }
            SignInOutcome::Failed(message) => Err(message),
        };
        match result {
            Ok(()) => state.accounts.finish(id, seq),
            Err(message) => {
                state.accounts.update(id, seq, |v| {
                    v.status = SignInStatus::Failed;
                    v.message = Some(message);
                });
            }
        }
        state.events.providers_changed();
    });

    providers::provider_view(state, id)
}

/// Saves the account connection with the models it offers.
async fn complete(
    state: &AppState,
    kind: ProviderKind,
    method: ConnectionMethod,
    label: Option<String>,
) -> AppResult<()> {
    let runtime = state
        .accounts
        .runtime(method)
        .ok_or_else(|| AppError::internal("account runtime missing"))?;
    let endpoint = Endpoint {
        kind,
        name: kind.display_name().into(),
        connection: method,
        base_url: Endpoint::default_base_url(kind).unwrap_or_default(),
        credential: runtime.credential().await?,
    };
    let fetched = state.llm.list_models(&endpoint).await?;
    if fetched.is_empty() {
        return Err(AppError::provider(format!(
            "No models are available to this {}.",
            method.display_name()
        )));
    }
    providers::save_connection(state, kind, method, label, &fetched)?;
    // The key is no longer used: don't leave it in the keychain.
    state.vault.delete(kind.as_str()).await?;
    Ok(())
}

pub fn cancel_sign_in(state: &AppState, kind: ProviderKind) -> AppResult<ProviderView> {
    let id = kind.as_str();
    if !state.accounts.cancel(id) {
        // Nothing running: clear a finished (cancelled or failed) attempt.
        state.accounts.dismiss(id);
        state.events.providers_changed();
    }
    providers::provider_view(state, id)
}

/// Checks an account connection with its runtime and records the result:
/// connected (refreshing who is signed in), expired, signed out elsewhere,
/// or runtime unavailable.
pub async fn check(state: &AppState, provider_id: &str) -> AppResult<ProviderView> {
    let row = state
        .db
        .call(|c| crate::db::providers::get(c, provider_id))?
        .ok_or_else(|| AppError::not_found("Provider not found"))?;
    let Some(runtime) = state.accounts.runtime(row.connection) else {
        return providers::provider_view(state, provider_id);
    };
    let (status, message) = match runtime.status().await {
        Ok(RuntimeStatus::SignedIn { label }) => {
            if label.is_some() && label != row.account_label {
                state.db.call(|c| {
                    crate::db::providers::set_account_label(c, provider_id, label.as_deref())
                })?;
            }
            (ConnectionStatus::Connected, None)
        }
        Ok(RuntimeStatus::Expired) => (ConnectionStatus::Expired, None),
        Ok(RuntimeStatus::SignedOut) => (ConnectionStatus::ReauthRequired, None),
        Ok(RuntimeStatus::NotInstalled(message)) => (ConnectionStatus::Unavailable, Some(message)),
        Err(error) => (ConnectionStatus::Unavailable, Some(error.to_string())),
    };
    state.accounts.set_health(provider_id, status, message);
    state.events.providers_changed();
    providers::provider_view(state, provider_id)
}

/// Signs out with the runtime's own logout, then disconnects the provider.
pub async fn sign_out(state: &AppState, provider_id: &str) -> AppResult<()> {
    let row = state
        .db
        .call(|c| crate::db::providers::get(c, provider_id))?
        .ok_or_else(|| AppError::not_found("Provider not found"))?;
    if let Some(runtime) = state.accounts.runtime(row.connection) {
        runtime.sign_out().await?;
    }
    providers::disconnect(state, provider_id).await
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use super::*;
    use crate::{
        accounts::{fake::FakeAccountRuntime, Accounts},
        llm::fake::FakeLanguageModel,
        secrets::Credential,
        state::testing,
    };

    struct World {
        state: AppState,
        chatgpt: Arc<FakeAccountRuntime>,
        console: Arc<FakeAccountRuntime>,
        opened: Arc<std::sync::Mutex<Vec<String>>>,
    }

    fn world() -> World {
        let (mut state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let chatgpt = Arc::new(FakeAccountRuntime::signed_out());
        let mut console = FakeAccountRuntime::signed_out();
        console.open_url = None;
        console.signed_in_label = "ana@example.com · Acme".into();
        console.credential = Some(Credential::OAuth {
            access_token: "console-token".into(),
            refresh_token: None,
            expires_at: None,
        });
        let console = Arc::new(console);
        state.accounts = Accounts::new(chatgpt.clone(), console.clone());
        World {
            state,
            chatgpt,
            console,
            opened: Arc::default(),
        }
    }

    impl World {
        async fn start(&self, kind: ProviderKind, device_code: bool) -> ProviderView {
            let opened = self.opened.clone();
            start_sign_in(&self.state, kind, device_code, move |url| {
                opened.lock().unwrap().push(url.to_string());
                Ok(())
            })
            .await
            .unwrap()
        }

        fn view(&self, id: &str) -> ProviderView {
            providers::provider_view(&self.state, id).unwrap()
        }

        /// Waits for the background task to finish the sign-in.
        async fn settle(&self, id: &str) -> ProviderView {
            for _ in 0..200 {
                let view = self.view(id);
                let waiting = view
                    .sign_in
                    .as_ref()
                    .is_some_and(|s| s.status == SignInStatus::WaitingForAuthorization);
                if !waiting {
                    return view;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            panic!("sign-in did not finish");
        }
    }

    #[tokio::test]
    async fn chatgpt_sign_in_opens_the_browser_then_connects_with_account_models() {
        let world = world();
        let view = world.start(ProviderKind::Openai, false).await;
        let sign_in = view.sign_in.unwrap();
        assert_eq!(sign_in.method, ConnectionMethod::ChatgptAccount);
        assert_eq!(sign_in.status, SignInStatus::WaitingForAuthorization);
        assert_eq!(view.connection, None);
        assert_eq!(
            *world.opened.lock().unwrap(),
            ["https://auth.example.com/authorize?state=1"]
        );

        world.chatgpt.finish(SignInOutcome::Succeeded);
        let view = world.settle("openai").await;
        assert!(view.sign_in.is_none());
        assert_eq!(view.connection, Some(ConnectionMethod::ChatgptAccount));
        assert_eq!(view.status, ConnectionStatus::Connected);
        assert_eq!(
            view.account_label.as_deref(),
            Some("ana@example.com · Plus")
        );
        // ReMa stores no secret for an account connection.
        assert!(!view.has_credential);
        assert!(world.state.vault.get("openai").await.unwrap().is_none());
        assert!(view.models.iter().any(|m| m.enabled));

        // The chat talks to the runtime, without a credential.
        let endpoint = providers::resolve_endpoint(&world.state, "openai")
            .await
            .unwrap();
        assert_eq!(endpoint.connection, ConnectionMethod::ChatgptAccount);
        assert!(endpoint.credential.is_none());
    }

    #[tokio::test]
    async fn device_code_sign_in_shows_the_code_and_page() {
        let world = world();
        let view = world.start(ProviderKind::Openai, true).await;
        let sign_in = view.sign_in.unwrap();
        assert_eq!(sign_in.user_code.as_deref(), Some("ABCD-1234"));
        assert_eq!(
            sign_in.verification_url.as_deref(),
            Some("https://auth.example.com/authorize?state=1")
        );
    }

    #[tokio::test]
    async fn cancelled_and_failed_sign_ins_are_reported_and_can_be_dismissed() {
        let world = world();
        world.start(ProviderKind::Openai, false).await;
        cancel_sign_in(&world.state, ProviderKind::Openai).unwrap();
        let view = world.settle("openai").await;
        assert_eq!(view.sign_in.unwrap().status, SignInStatus::Cancelled);
        assert_eq!(view.connection, None);

        // Dismissing clears the notice.
        let view = cancel_sign_in(&world.state, ProviderKind::Openai).unwrap();
        assert!(view.sign_in.is_none());

        world.start(ProviderKind::Openai, false).await;
        world
            .chatgpt
            .finish(SignInOutcome::Failed("workspace blocks sign-in".into()));
        let sign_in = world.settle("openai").await.sign_in.unwrap();
        assert_eq!(sign_in.status, SignInStatus::Failed);
        assert_eq!(sign_in.message.as_deref(), Some("workspace blocks sign-in"));
    }

    #[tokio::test]
    async fn a_missing_runtime_fails_with_install_instructions() {
        let mut world = world();
        let missing = Arc::new(FakeAccountRuntime::with_status(
            RuntimeStatus::NotInstalled("Install Codex first.".into()),
        ));
        world.state.accounts = Accounts::new(missing, world.console.clone());
        let view = world.start(ProviderKind::Openai, false).await;
        let sign_in = view.sign_in.unwrap();
        assert_eq!(sign_in.status, SignInStatus::Failed);
        assert_eq!(sign_in.message.as_deref(), Some("Install Codex first."));
        assert!(world.opened.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_existing_runtime_sign_in_connects_without_a_browser() {
        let world = world();
        *world.chatgpt.status.lock().unwrap() = RuntimeStatus::SignedIn {
            label: Some("ana@example.com · Pro".into()),
        };
        let view = world.start(ProviderKind::Openai, false).await;
        assert!(world.opened.lock().unwrap().is_empty());
        assert_eq!(*world.chatgpt.sign_ins.lock().unwrap(), 0);
        assert_eq!(view.connection, Some(ConnectionMethod::ChatgptAccount));
        assert_eq!(view.account_label.as_deref(), Some("ana@example.com · Pro"));
    }

    #[tokio::test]
    async fn claude_console_sign_in_uses_the_cli_token_for_requests() {
        let world = world();
        let view = world.start(ProviderKind::Anthropic, false).await;
        // The CLI opens the browser itself.
        assert!(world.opened.lock().unwrap().is_empty());
        assert_eq!(
            view.sign_in.unwrap().status,
            SignInStatus::WaitingForAuthorization
        );
        world.console.finish(SignInOutcome::Succeeded);
        let view = world.settle("anthropic").await;
        assert_eq!(view.connection, Some(ConnectionMethod::ClaudeConsole));
        assert_eq!(
            view.account_label.as_deref(),
            Some("ana@example.com · Acme")
        );

        let endpoint = providers::resolve_endpoint(&world.state, "anthropic")
            .await
            .unwrap();
        assert_eq!(endpoint.base_url, "https://api.anthropic.com/v1");
        assert!(matches!(
            endpoint.credential,
            Some(Credential::OAuth { ref access_token, .. }) if access_token == "console-token"
        ));
    }

    #[tokio::test]
    async fn switching_methods_replaces_the_key_and_the_models() {
        let world = world();
        providers::connect(&world.state, ProviderKind::Openai, "sk-key")
            .await
            .unwrap();
        assert!(world.state.vault.get("openai").await.unwrap().is_some());

        world.start(ProviderKind::Openai, false).await;
        // Still connected with the key while the sign-in runs.
        assert_eq!(
            world.view("openai").connection,
            Some(ConnectionMethod::ApiKey)
        );
        world.chatgpt.finish(SignInOutcome::Succeeded);
        let view = world.settle("openai").await;
        assert_eq!(view.connection, Some(ConnectionMethod::ChatgptAccount));
        // The unused key is removed from the keychain.
        assert!(world.state.vault.get("openai").await.unwrap().is_none());

        // And back to a key: the account label is gone.
        let view = providers::connect(&world.state, ProviderKind::Openai, "sk-key-2")
            .await
            .unwrap();
        assert_eq!(view.connection, Some(ConnectionMethod::ApiKey));
        assert_eq!(view.account_label, None);
    }

    #[tokio::test]
    async fn checks_report_expired_and_signed_out_accounts() {
        let world = world();
        world.start(ProviderKind::Openai, false).await;
        world.chatgpt.finish(SignInOutcome::Succeeded);
        world.settle("openai").await;

        *world.chatgpt.status.lock().unwrap() = RuntimeStatus::SignedOut;
        let view = check(&world.state, "openai").await.unwrap();
        assert_eq!(view.status, ConnectionStatus::ReauthRequired);

        *world.chatgpt.status.lock().unwrap() = RuntimeStatus::Expired;
        assert_eq!(
            check(&world.state, "openai").await.unwrap().status,
            ConnectionStatus::Expired
        );

        *world.chatgpt.status.lock().unwrap() =
            RuntimeStatus::NotInstalled("Install Codex.".into());
        let view = check(&world.state, "openai").await.unwrap();
        assert_eq!(view.status, ConnectionStatus::Unavailable);
        assert_eq!(view.status_message.as_deref(), Some("Install Codex."));

        *world.chatgpt.status.lock().unwrap() = RuntimeStatus::SignedIn {
            label: Some("ana@example.com · Pro".into()),
        };
        let view = check(&world.state, "openai").await.unwrap();
        assert_eq!(view.status, ConnectionStatus::Connected);
        assert_eq!(view.account_label.as_deref(), Some("ana@example.com · Pro"));
    }

    #[tokio::test]
    async fn sign_out_uses_the_runtime_logout_and_disconnects() {
        let world = world();
        world.start(ProviderKind::Openai, false).await;
        world.chatgpt.finish(SignInOutcome::Succeeded);
        world.settle("openai").await;

        sign_out(&world.state, "openai").await.unwrap();
        assert_eq!(*world.chatgpt.sign_outs.lock().unwrap(), 1);
        assert_eq!(world.view("openai").connection, None);
        assert!(providers::catalog(&world.state).unwrap().models.is_empty());
    }

    #[tokio::test]
    async fn plain_disconnect_keeps_the_runtime_signed_in() {
        let world = world();
        world.start(ProviderKind::Openai, false).await;
        world.chatgpt.finish(SignInOutcome::Succeeded);
        world.settle("openai").await;

        providers::disconnect(&world.state, "openai").await.unwrap();
        assert_eq!(*world.chatgpt.sign_outs.lock().unwrap(), 0);
        assert_eq!(world.view("openai").connection, None);
    }

    #[tokio::test]
    async fn gemini_has_no_account_sign_in() {
        let world = world();
        let error = start_sign_in(&world.state, ProviderKind::Gemini, false, |_| Ok(()))
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::Validation(_)));
    }
}
