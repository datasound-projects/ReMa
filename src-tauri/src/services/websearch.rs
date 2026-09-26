//! The search service ReMa calls itself (Settings → Web search): for models
//! without a hosted web search, and as the fallback when a model's own
//! search fails. The API key lives in the OS credential store; the
//! interface only learns whether one is stored.

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use crate::{
    db::providers as settings,
    error::{AppError, AppResult},
    retrieval::backend::{self, Service, ServiceKind, KIND_KEY, SECRET, URL_KEY},
    state::AppState,
};

/// The search service as Settings shows it. Never contains the key.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchSettings {
    pub service: Option<ServiceKind>,
    /// SearXNG address.
    pub url: Option<String>,
    pub has_key: bool,
}

/// `service: None` turns the service off (and removes its key).
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchInput {
    pub service: Option<ServiceKind>,
    pub url: Option<String>,
    /// `None` keeps the stored key.
    pub key: Option<String>,
}

/// What a test search found.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchTest {
    pub results: u32,
    pub titles: Vec<String>,
}

pub async fn settings(state: &AppState) -> AppResult<WebSearchSettings> {
    let (kind, url) = state.db.call(|c| {
        Ok((
            settings::get_setting(c, KIND_KEY)?,
            settings::get_setting(c, URL_KEY)?,
        ))
    })?;
    let service = kind.as_deref().and_then(ServiceKind::parse);
    let has_key = match service {
        Some(kind) if kind.needs_key() => state
            .vault
            .get_text(SECRET)
            .await?
            .is_some_and(|k| !k.trim().is_empty()),
        _ => false,
    };
    Ok(WebSearchSettings {
        service,
        url: url.filter(|_| service == Some(ServiceKind::Searxng)),
        has_key,
    })
}

pub async fn save(state: &AppState, input: WebSearchInput) -> AppResult<WebSearchSettings> {
    let Some(kind) = input.service else {
        state.db.call(|c| {
            settings::delete_setting(c, KIND_KEY)?;
            settings::delete_setting(c, URL_KEY)
        })?;
        state.vault.delete_text(SECRET).await?;
        return settings(state).await;
    };
    let key = input
        .key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty());
    if let Some(key) = key {
        if key.len() > 512 || key.chars().any(char::is_whitespace) {
            return Err(AppError::validation(
                "That does not look like a valid API key.",
            ));
        }
    }
    let url = input
        .url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    // Validates the address before anything is saved.
    Service::new(kind, url, None)?;
    if kind.needs_key() {
        let stored = state
            .vault
            .get_text(SECRET)
            .await?
            .is_some_and(|k| !k.trim().is_empty());
        let current = settings(state).await?.service;
        match key {
            Some(key) => state.vault.set_text(SECRET, key).await?,
            // A stored key belongs to the service it was entered for.
            None if stored && current == Some(kind) => {}
            None => {
                return Err(AppError::validation(format!(
                    "Enter your {} API key.",
                    kind.name()
                )))
            }
        }
    } else {
        state.vault.delete_text(SECRET).await?;
    }
    state.db.call(|c| {
        settings::set_setting(c, KIND_KEY, kind.as_str())?;
        match url {
            Some(url) if kind == ServiceKind::Searxng => settings::set_setting(c, URL_KEY, url),
            _ => settings::delete_setting(c, URL_KEY),
        }
    })?;
    settings(state).await
}

/// Runs one real search with the saved service.
pub async fn test(state: &AppState) -> AppResult<WebSearchTest> {
    let service = backend::configured(state)
        .await?
        .ok_or_else(|| AppError::validation("Choose a search service first."))?;
    let hits = service
        .search("software engineer jobs", None, &CancellationToken::new())
        .await
        .map_err(|e| AppError::provider(format!("{}: {e}", service.name())))?;
    Ok(WebSearchTest {
        results: hits.len() as u32,
        titles: hits.into_iter().take(3).map(|h| h.title).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing};
    use std::sync::Arc;

    #[tokio::test]
    async fn saves_the_service_with_its_key_in_the_credential_store() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let empty = settings(&state).await.unwrap();
        assert_eq!((empty.service, empty.has_key), (None, false));

        let missing_key = save(
            &state,
            WebSearchInput {
                service: Some(ServiceKind::Brave),
                url: None,
                key: None,
            },
        )
        .await;
        assert!(missing_key.is_err());

        let saved = save(
            &state,
            WebSearchInput {
                service: Some(ServiceKind::Brave),
                url: None,
                key: Some(" brave-key ".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            (saved.service, saved.has_key),
            (Some(ServiceKind::Brave), true)
        );
        assert_eq!(
            state.vault.get_text(SECRET).await.unwrap().as_deref(),
            Some("brave-key")
        );
        // Saving again without a key keeps it.
        save(
            &state,
            WebSearchInput {
                service: Some(ServiceKind::Brave),
                url: None,
                key: None,
            },
        )
        .await
        .unwrap();
        assert!(backend::configured(&state).await.unwrap().is_some());

        // SearXNG needs an address and no key; the old key is removed.
        assert!(save(
            &state,
            WebSearchInput {
                service: Some(ServiceKind::Searxng),
                url: Some("not a url".into()),
                key: None,
            },
        )
        .await
        .is_err());
        let searx = save(
            &state,
            WebSearchInput {
                service: Some(ServiceKind::Searxng),
                url: Some("http://localhost:8888".into()),
                key: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(searx.url.as_deref(), Some("http://localhost:8888"));
        assert!(state.vault.get_text(SECRET).await.unwrap().is_none());

        let off = save(
            &state,
            WebSearchInput {
                service: None,
                url: None,
                key: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(off.service, None);
        assert!(backend::configured(&state).await.unwrap().is_none());
    }
}
