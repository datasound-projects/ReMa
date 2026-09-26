//! Provider credentials, kept in the operating system's credential store
//! (macOS Keychain, Windows Credential Manager, Secret Service on Linux).
//!
//! Secrets never touch the database or the frontend: the UI can only save,
//! replace or delete them, and ask whether one exists.

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Keychain service name; equals the application identifier.
pub const SERVICE: &str = "cloud.datasound.rema";

/// A credential for one provider.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Credential {
    ApiKey {
        key: String,
    },
    /// Tokens from an OAuth flow. `expires_at` is epoch milliseconds.
    #[serde(rename = "oauth")]
    OAuth {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<i64>,
    },
}

impl Credential {
    /// The secret values, used to scrub them from provider error messages.
    pub fn secret_values(&self) -> Vec<&str> {
        match self {
            Self::ApiKey { key } => vec![key.as_str()],
            Self::OAuth {
                access_token,
                refresh_token,
                ..
            } => std::iter::once(access_token.as_str())
                .chain(refresh_token.as_deref())
                .collect(),
        }
    }
}

// Never print secrets, even in debug logs.
impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey { .. } => f.write_str("Credential::ApiKey(<redacted>)"),
            Self::OAuth { expires_at, .. } => f
                .debug_struct("Credential::OAuth")
                .field("expires_at", expires_at)
                .finish_non_exhaustive(),
        }
    }
}

/// Raw secret storage, keyed by account (the provider id).
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> AppResult<Option<String>>;
    fn set(&self, account: &str, secret: &str) -> AppResult<()>;
    fn delete(&self, account: &str) -> AppResult<()>;
}

/// The OS credential store.
pub struct KeyringStore;

fn keyring_error(error: keyring::Error) -> AppError {
    AppError::configuration(format!(
        "secure credential storage is unavailable ({error})"
    ))
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(keyring_error)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keyring_error(error)),
        }
    }

    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|entry| entry.set_password(secret))
            .map_err(keyring_error)
    }

    fn delete(&self, account: &str) -> AppResult<()> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(keyring_error)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        }
    }
}

/// In-memory store for tests.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        self.0.lock().unwrap().insert(account.into(), secret.into());
        Ok(())
    }

    fn delete(&self, account: &str) -> AppResult<()> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

/// Typed, cached access to credentials. The cache avoids repeated keychain
/// round-trips (and OS prompts) for every request.
#[derive(Clone)]
pub struct SecretVault {
    store: Arc<dyn SecretStore>,
    cache: Arc<Mutex<HashMap<String, Option<Credential>>>>,
}

impl SecretVault {
    pub fn new(store: Arc<dyn SecretStore>) -> Self {
        Self {
            store,
            cache: Arc::default(),
        }
    }

    fn cached(&self, provider_id: &str) -> Option<Option<Credential>> {
        self.cache.lock().ok()?.get(provider_id).cloned()
    }

    fn remember(&self, provider_id: &str, credential: Option<Credential>) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(provider_id.to_string(), credential);
        }
    }

    pub async fn get(&self, provider_id: &str) -> AppResult<Option<Credential>> {
        if let Some(credential) = self.cached(provider_id) {
            return Ok(credential);
        }
        let store = self.store.clone();
        let account = provider_id.to_string();
        let raw = blocking(move || store.get(&account)).await?;
        let credential = match raw {
            Some(raw) => Some(serde_json::from_str::<Credential>(&raw).map_err(|_| {
                AppError::configuration("a stored credential is unreadable; reconnect the provider")
            })?),
            None => None,
        };
        self.remember(provider_id, credential.clone());
        Ok(credential)
    }

    pub async fn set(&self, provider_id: &str, credential: Credential) -> AppResult<()> {
        let raw =
            serde_json::to_string(&credential).map_err(|e| AppError::internal(e.to_string()))?;
        let store = self.store.clone();
        let account = provider_id.to_string();
        blocking(move || store.set(&account, &raw)).await?;
        self.remember(provider_id, Some(credential));
        Ok(())
    }

    pub async fn delete(&self, provider_id: &str) -> AppResult<()> {
        let store = self.store.clone();
        let account = provider_id.to_string();
        blocking(move || store.delete(&account)).await?;
        self.remember(provider_id, None);
        Ok(())
    }
}

impl SecretVault {
    /// Other secrets (MCP environment values, tokens, OAuth credentials),
    /// stored as text under their own account names. Not cached.
    pub async fn get_text(&self, account: &str) -> AppResult<Option<String>> {
        let store = self.store.clone();
        let account = account.to_string();
        blocking(move || store.get(&account)).await
    }

    pub async fn set_text(&self, account: &str, value: &str) -> AppResult<()> {
        let store = self.store.clone();
        let (account, value) = (account.to_string(), value.to_string());
        blocking(move || store.set(&account, &value)).await
    }

    pub async fn delete_text(&self, account: &str) -> AppResult<()> {
        let store = self.store.clone();
        let account = account.to_string();
        blocking(move || store.delete(&account)).await
    }
}

/// Credential stores may block (D-Bus, Keychain); keep them off async workers.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::internal(format!("credential task failed: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stores_reads_and_deletes_credentials() {
        let store = Arc::new(MemoryStore::default());
        let vault = SecretVault::new(store.clone());

        assert_eq!(vault.get("openai").await.unwrap(), None);
        let key = Credential::ApiKey {
            key: "sk-test".into(),
        };
        vault.set("openai", key.clone()).await.unwrap();
        assert_eq!(vault.get("openai").await.unwrap(), Some(key));

        // Stored as JSON in the OS store, not in the database.
        assert!(store.get("openai").unwrap().unwrap().contains("sk-test"));

        vault.delete("openai").await.unwrap();
        assert_eq!(vault.get("openai").await.unwrap(), None);
        assert_eq!(store.get("openai").unwrap(), None);
    }

    #[test]
    fn debug_output_never_contains_secrets() {
        let key = Credential::ApiKey {
            key: "sk-secret".into(),
        };
        assert!(!format!("{key:?}").contains("sk-secret"));

        let oauth = Credential::OAuth {
            access_token: "at-secret".into(),
            refresh_token: Some("rt-secret".into()),
            expires_at: Some(1),
        };
        let debug = format!("{oauth:?}");
        assert!(!debug.contains("at-secret") && !debug.contains("rt-secret"));
        assert_eq!(oauth.secret_values(), vec!["at-secret", "rt-secret"]);
    }
}
