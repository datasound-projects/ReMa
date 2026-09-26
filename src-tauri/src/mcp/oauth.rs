//! OAuth sign-in for remote MCP servers (MCP authorization).
//!
//! `rmcp` implements the protocol: authorization server discovery (protected
//! resource metadata), client registration (Client ID Metadata Documents or
//! Dynamic Client Registration, as the server supports), PKCE, the token
//! exchange and refresh. ReMa supplies the browser, the loopback redirect
//! and storage: tokens are kept in the OS credential store, never in the
//! database, logs or the interface.

use std::{sync::Arc, time::Duration};

use rmcp::transport::auth::{
    AuthError, AuthorizationManager, AuthorizationRequest, CredentialStore, OAuthState,
    StoredCredentials,
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{
    error::{AppError, AppResult},
    oauth_loopback,
    secrets::SecretVault,
};

use super::client::SharedStore;

/// How long ReMa waits for the browser sign-in.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// OAuth credentials of one server in the OS credential store.
pub struct KeychainCredentials {
    vault: SecretVault,
    account: String,
}

impl KeychainCredentials {
    pub fn new(vault: SecretVault, account: String) -> Self {
        Self { vault, account }
    }
}

fn store_error(error: AppError) -> AuthError {
    AuthError::CredentialStoreError(error.to_string())
}

#[async_trait::async_trait]
impl CredentialStore for KeychainCredentials {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let raw = self
            .vault
            .get_text(&self.account)
            .await
            .map_err(store_error)?;
        // Unreadable credentials mean signing in again, not a failure.
        Ok(raw.and_then(|raw| serde_json::from_str(&raw).ok()))
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let raw = serde_json::to_string(&credentials)
            .map_err(|e| AuthError::CredentialStoreError(e.to_string()))?;
        self.vault
            .set_text(&self.account, &raw)
            .await
            .map_err(store_error)
    }

    async fn clear(&self) -> Result<(), AuthError> {
        self.vault
            .delete_text(&self.account)
            .await
            .map_err(store_error)
    }
}

fn auth_failed(server: &str, error: AuthError) -> AppError {
    let detail: String = error.to_string().chars().take(300).collect();
    match error {
        AuthError::NoAuthorizationSupport => AppError::validation(format!(
            "{server} does not offer OAuth sign-in. Use a token instead."
        )),
        _ => AppError::authentication(format!("{server} sign-in failed: {detail}")),
    }
}

/// Signs in through the browser and stores the credentials. `open` shows
/// the authorization page in the system browser.
pub async fn sign_in(
    server: &str,
    url: &str,
    store: Arc<dyn CredentialStore>,
    open: impl Fn(&str) -> AppResult<()>,
    cancel: &CancellationToken,
) -> AppResult<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let mut manager = AuthorizationManager::new(url)
        .await
        .map_err(|e| auth_failed(server, e))?;
    manager.set_credential_store(SharedStore(store));
    let mut state = OAuthState::Unauthorized(manager);
    state
        .start_authorization(
            AuthorizationRequest::new(redirect_uri.clone())
                .with_client_name("ReMa")
                .with_application_type("native"),
        )
        .await
        .map_err(|e| auth_failed(server, e))?;
    let authorization_url = state
        .get_authorization_url()
        .await
        .map_err(|e| auth_failed(server, e))?;
    open(&authorization_url)?;

    let target = oauth_loopback::receive(
        listener,
        |target| target.starts_with("/callback"),
        |target| target.contains("code="),
        oauth_loopback::Pages { service: server },
        cancel,
        SIGN_IN_TIMEOUT,
    )
    .await?;
    if !target.contains("code=") {
        return Err(AppError::authentication(format!(
            "{server} sign-in was not completed."
        )));
    }
    state
        .handle_callback_url(&format!("http://127.0.0.1:{port}{target}"))
        .await
        .map_err(|e| auth_failed(server, e))
}
