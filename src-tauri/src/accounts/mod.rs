//! Provider accounts: browser sign-in through the providers' official local
//! runtimes.
//!
//! ```text
//! React ─ Tauri command ─ services::accounts ─ AccountRuntime ─ official runtime ─ provider
//! ```
//!
//! Each runtime is the provider's own software, run as a private child
//! process with a ReMa-owned home folder:
//!
//! - [`codex::CodexRuntime`] — OpenAI's Codex app-server (the interface the
//!   official Codex SDKs use). Codex performs the ChatGPT sign-in, keeps the
//!   credentials (OS keychain when available) and sends the model requests.
//! - [`claude_console::ClaudeConsole`] — Anthropic's `ant` CLI. It performs
//!   the Claude Console sign-in and keeps and refreshes the token; ReMa asks
//!   it for a short-lived access token per request batch and calls the
//!   Anthropic API directly.
//!
//! ReMa never implements a provider's OAuth protocol, never reads browser
//! sessions, and never stores, logs or returns account tokens to the UI. No
//! ReMa server is involved: traffic goes from this computer to the provider.

pub mod claude_console;
pub mod codex;
#[cfg(test)]
pub mod fake;
pub mod locate;

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use tokio_util::sync::CancellationToken;

use crate::{
    error::AppResult,
    llm::BoxFuture,
    models::provider::{ConnectionMethod, ConnectionStatus, SignInStatus, SignInView},
    secrets::Credential,
};

/// What a runtime reports about its sign-in. Never opens a browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeStatus {
    /// The runtime is not installed or too old; the message says what to do.
    NotInstalled(String),
    SignedOut,
    /// Signed in, but the sign-in expired and could not be renewed.
    Expired,
    SignedIn {
        label: Option<String>,
    },
}

/// How a browser sign-in ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInOutcome {
    Succeeded,
    Cancelled,
    Failed(String),
}

/// A sign-in in progress.
pub struct SignInAttempt {
    /// A page ReMa opens in the system browser, when the runtime does not
    /// open it itself.
    pub open_url: Option<String>,
    /// Device-code sign-in: the one-time code the user enters on that page.
    pub user_code: Option<String>,
    /// Resolves when the provider's page reports back (or it is cancelled).
    pub outcome: BoxFuture<'static, SignInOutcome>,
    /// Cancelling stops the attempt through the runtime.
    pub cancel: CancellationToken,
}

/// One provider's official local runtime.
pub trait AccountRuntime: Send + Sync {
    /// Whether the runtime is installed and who is signed in.
    fn status(&self) -> BoxFuture<'_, AppResult<RuntimeStatus>>;

    /// Starts a browser sign-in. `device_code` asks for a one-time code
    /// instead of a redirect back to this computer, where supported.
    fn start_sign_in(&self, device_code: bool) -> BoxFuture<'_, AppResult<SignInAttempt>>;

    /// Signs out with the runtime's own logout.
    fn sign_out(&self) -> BoxFuture<'_, AppResult<()>>;

    /// A credential for requests ReMa sends itself, for runtimes that only
    /// sign in. `None` when the runtime sends the requests.
    fn credential(&self) -> BoxFuture<'_, AppResult<Option<Credential>>> {
        Box::pin(async { Ok(None) })
    }

    /// Whether the runtime supports device-code sign-in.
    fn supports_device_code(&self) -> bool {
        false
    }

    /// Stops background processes (app exit).
    fn shutdown(&self) {}
}

/// A problem found with a saved connection, and why.
type Health = (ConnectionStatus, Option<String>);

struct Session {
    seq: u64,
    view: SignInView,
    cancel: Option<CancellationToken>,
}

/// The account runtimes plus the state of their sign-ins and connections.
#[derive(Clone)]
pub struct Accounts {
    chatgpt: Arc<dyn AccountRuntime>,
    claude_console: Arc<dyn AccountRuntime>,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    /// Problems found with saved connections, by provider id.
    health: Arc<Mutex<HashMap<String, Health>>>,
    seq: Arc<AtomicU64>,
}

impl Accounts {
    pub fn new(chatgpt: Arc<dyn AccountRuntime>, claude_console: Arc<dyn AccountRuntime>) -> Self {
        Self {
            chatgpt,
            claude_console,
            sessions: Arc::default(),
            health: Arc::default(),
            seq: Arc::default(),
        }
    }

    pub fn runtime(&self, method: ConnectionMethod) -> Option<Arc<dyn AccountRuntime>> {
        match method {
            ConnectionMethod::ChatgptAccount => Some(self.chatgpt.clone()),
            ConnectionMethod::ClaudeConsole => Some(self.claude_console.clone()),
            ConnectionMethod::ApiKey => None,
        }
    }

    /// Starts tracking a sign-in for `provider_id`, cancelling any earlier
    /// one. Returns its sequence number.
    pub fn begin(&self, provider_id: &str, method: ConnectionMethod) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let previous = self.sessions.lock().unwrap().insert(
            provider_id.to_string(),
            Session {
                seq,
                view: SignInView {
                    method,
                    status: SignInStatus::Connecting,
                    message: None,
                    user_code: None,
                    verification_url: None,
                },
                cancel: None,
            },
        );
        if let Some(cancel) = previous.and_then(|s| s.cancel) {
            cancel.cancel();
        }
        seq
    }

    /// Updates a sign-in unless a newer one replaced it. Returns whether it
    /// is still current.
    pub fn update(
        &self,
        provider_id: &str,
        seq: u64,
        change: impl FnOnce(&mut SignInView),
    ) -> bool {
        let mut sessions = self.sessions.lock().unwrap();
        match sessions.get_mut(provider_id) {
            Some(session) if session.seq == seq => {
                change(&mut session.view);
                true
            }
            _ => false,
        }
    }

    pub fn set_cancel(&self, provider_id: &str, seq: u64, cancel: CancellationToken) {
        let mut sessions = self.sessions.lock().unwrap();
        match sessions.get_mut(provider_id) {
            Some(session) if session.seq == seq => session.cancel = Some(cancel),
            // Replaced meanwhile: stop this attempt.
            _ => cancel.cancel(),
        }
    }

    /// Ends a sign-in that succeeded (the connection now shows instead).
    pub fn finish(&self, provider_id: &str, seq: u64) {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.get(provider_id).is_some_and(|s| s.seq == seq) {
            sessions.remove(provider_id);
        }
    }

    /// Cancels a running sign-in. Returns whether one was running.
    pub fn cancel(&self, provider_id: &str) -> bool {
        let sessions = self.sessions.lock().unwrap();
        let running = sessions.get(provider_id).filter(|s| {
            !matches!(
                s.view.status,
                SignInStatus::Cancelled | SignInStatus::Failed
            )
        });
        match running.and_then(|s| s.cancel.clone()) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Forgets a sign-in, cancelling it if it is still running.
    pub fn dismiss(&self, provider_id: &str) {
        if let Some(cancel) = self
            .sessions
            .lock()
            .unwrap()
            .remove(provider_id)
            .and_then(|s| s.cancel)
        {
            cancel.cancel();
        }
    }

    pub fn sign_in(&self, provider_id: &str) -> Option<SignInView> {
        self.sessions
            .lock()
            .unwrap()
            .get(provider_id)
            .map(|s| s.view.clone())
    }

    /// Records the result of a connection check (`Connected` clears it).
    pub fn set_health(&self, provider_id: &str, status: ConnectionStatus, message: Option<String>) {
        let mut health = self.health.lock().unwrap();
        if status == ConnectionStatus::Connected {
            health.remove(provider_id);
        } else {
            health.insert(provider_id.to_string(), (status, message));
        }
    }

    pub fn health(&self, provider_id: &str) -> Option<Health> {
        self.health.lock().unwrap().get(provider_id).cloned()
    }

    /// Stops sign-ins and runtime processes (app exit).
    pub fn shutdown(&self) {
        for session in self.sessions.lock().unwrap().values() {
            if let Some(cancel) = &session.cancel {
                cancel.cancel();
            }
        }
        self.chatgpt.shutdown();
        self.claude_console.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake::FakeAccountRuntime;

    fn accounts() -> Accounts {
        Accounts::new(
            Arc::new(FakeAccountRuntime::signed_out()),
            Arc::new(FakeAccountRuntime::signed_out()),
        )
    }

    #[test]
    fn a_new_sign_in_replaces_and_cancels_the_previous_one() {
        let accounts = accounts();
        let first = accounts.begin("openai", ConnectionMethod::ChatgptAccount);
        let token = CancellationToken::new();
        accounts.set_cancel("openai", first, token.clone());

        let second = accounts.begin("openai", ConnectionMethod::ChatgptAccount);
        assert!(token.is_cancelled());
        // The old attempt can no longer change what the UI shows.
        assert!(!accounts.update("openai", first, |v| v.status = SignInStatus::Failed));
        assert!(accounts.update("openai", second, |v| {
            v.status = SignInStatus::WaitingForAuthorization
        }));
        assert_eq!(
            accounts.sign_in("openai").unwrap().status,
            SignInStatus::WaitingForAuthorization
        );

        // A late cancel handle for a replaced attempt is cancelled at once.
        let late = CancellationToken::new();
        accounts.set_cancel("openai", first, late.clone());
        assert!(late.is_cancelled());

        accounts.finish("openai", first);
        assert!(accounts.sign_in("openai").is_some());
        accounts.finish("openai", second);
        assert!(accounts.sign_in("openai").is_none());
    }

    #[test]
    fn tracks_connection_health() {
        let accounts = accounts();
        accounts.set_health("openai", ConnectionStatus::ReauthRequired, None);
        assert_eq!(
            accounts.health("openai").unwrap().0,
            ConnectionStatus::ReauthRequired
        );
        accounts.set_health("openai", ConnectionStatus::Connected, None);
        assert!(accounts.health("openai").is_none());
    }
}
