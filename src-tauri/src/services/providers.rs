//! Provider configuration: connecting providers (API key or account
//! sign-in), storing credentials, syncing their model lists and choosing
//! the default model.

use crate::{
    db::providers::{self as repo, ModelRow, ProviderRow},
    error::{AppError, AppResult},
    llm::{Endpoint, FetchedModel},
    models::provider::{
        AuthMethod, ConnectionMethod, ConnectionStatus, CustomProviderInput, ModelCatalog,
        ModelOption, ModelRef, ProviderKind, ProviderModel, ProviderSettings, ProviderView,
    },
    secrets::Credential,
    state::AppState,
    time::now_ms,
};

const DEFAULT_MODEL_KEY: &str = "default_model";
/// Models enabled automatically when a provider is first connected.
const MAX_AUTO_ENABLED: usize = 5;

pub fn settings(state: &AppState) -> AppResult<ProviderSettings> {
    state.db.call(|conn| {
        let rows = repo::list(conn)?;
        let mut providers = Vec::new();
        // Built-in providers are always listed, connected or not.
        for kind in ProviderKind::BUILT_IN {
            let row = rows.iter().find(|r| r.id == kind.as_str());
            providers.push(match row {
                Some(row) => view(state, conn, row)?,
                None => ProviderView {
                    id: kind.as_str().into(),
                    kind,
                    name: kind.display_name().into(),
                    base_url: None,
                    configured_model: None,
                    connection_methods: kind.connection_methods().to_vec(),
                    connection: None,
                    account_label: None,
                    status: ConnectionStatus::Disconnected,
                    status_message: None,
                    sign_in: state.accounts.sign_in(kind.as_str()),
                    configured: false,
                    has_credential: false,
                    out_of_credits: false,
                    billing_url: None,
                    models: Vec::new(),
                },
            });
        }
        for row in rows
            .iter()
            .filter(|r| r.kind == ProviderKind::OpenaiCompatible)
        {
            providers.push(view(state, conn, row)?);
        }
        Ok(ProviderSettings {
            providers,
            default_model: default_model(conn)?,
        })
    })
}

fn view(
    state: &AppState,
    conn: &rusqlite::Connection,
    row: &ProviderRow,
) -> AppResult<ProviderView> {
    let models = repo::list_models(conn, &row.id)?
        .into_iter()
        .map(|m| ProviderModel {
            model_id: m.model_id,
            display_name: m.display_name,
            enabled: m.enabled,
        })
        .collect();
    let (status, status_message) = state
        .accounts
        .health(&row.id)
        .unwrap_or((ConnectionStatus::Connected, None));
    Ok(ProviderView {
        id: row.id.clone(),
        kind: row.kind,
        name: row.name.clone(),
        base_url: row.base_url.clone(),
        configured_model: row.model.clone(),
        connection_methods: row.kind.connection_methods().to_vec(),
        connection: Some(row.connection),
        account_label: row.account_label.clone(),
        status,
        status_message,
        sign_in: state.accounts.sign_in(&row.id),
        configured: true,
        // A key is saved whenever the provider uses one; the secret itself
        // stays in the OS credential store.
        has_credential: row.auth_method != AuthMethod::None,
        out_of_credits: repo::get_setting(conn, &credits_key(&row.id))?.is_some(),
        billing_url: billing_url(row.kind, row.connection).map(str::to_string),
        models,
    })
}

/// Where a connection's pay-as-you-go API credits are managed (not for
/// account plans such as ChatGPT, whose limits reset on their own).
fn billing_url(kind: ProviderKind, connection: ConnectionMethod) -> Option<&'static str> {
    match (kind, connection) {
        (ProviderKind::Anthropic, ConnectionMethod::ApiKey | ConnectionMethod::ClaudeConsole) => {
            Some(crate::llm::http::ANTHROPIC_BILLING_URL)
        }
        (ProviderKind::Openai, ConnectionMethod::ApiKey) => {
            Some(crate::llm::http::OPENAI_BILLING_URL)
        }
        _ => None,
    }
}

fn credits_key(provider_id: &str) -> String {
    format!("provider.{provider_id}.out_of_credits")
}

/// Remembers from a model request's outcome whether the provider's account
/// ran out of credits, so Settings can say so until a request succeeds.
pub fn note_outcome<T>(state: &AppState, provider_id: &str, outcome: &AppResult<T>) {
    let key = credits_key(provider_id);
    let changed = state.db.call(|conn| {
        let flagged = repo::get_setting(conn, &key)?.is_some();
        match outcome {
            Err(AppError::Billing(_)) if !flagged => {
                repo::set_setting(conn, &key, "1")?;
                Ok(true)
            }
            Ok(_) if flagged => {
                repo::delete_setting(conn, &key)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    });
    match changed {
        Ok(true) => state.events.providers_changed(),
        Ok(false) => {}
        Err(error) => eprintln!("could not record the state of {provider_id}: {error}"),
    }
}

pub fn provider_view(state: &AppState, id: &str) -> AppResult<ProviderView> {
    let found = state.db.call(|conn| match repo::get(conn, id)? {
        Some(row) => view(state, conn, &row).map(Some),
        None => Ok(None),
    })?;
    match found {
        Some(view) => Ok(view),
        // A built-in provider that is not connected.
        None => settings(state)?
            .providers
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::not_found("Provider not found")),
    }
}

fn validate_api_key(api_key: &str) -> AppResult<String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err(AppError::validation("Enter an API key."));
    }
    if key.len() > 2048 || key.chars().any(char::is_whitespace) {
        return Err(AppError::validation(
            "That does not look like a valid API key.",
        ));
    }
    Ok(key.to_string())
}

/// Connects a cloud provider with an API key. The key is verified by
/// fetching the provider's model list before anything is saved.
pub async fn connect(
    state: &AppState,
    kind: ProviderKind,
    api_key: &str,
) -> AppResult<ProviderView> {
    if kind == ProviderKind::OpenaiCompatible {
        return Err(AppError::validation(
            "Use an OpenAI-compatible endpoint for custom servers.",
        ));
    }
    let credential = Credential::ApiKey {
        key: validate_api_key(api_key)?,
    };
    let endpoint = Endpoint {
        kind,
        name: kind.display_name().into(),
        connection: ConnectionMethod::ApiKey,
        base_url: Endpoint::default_base_url(kind).unwrap_or_default(),
        credential: Some(credential.clone()),
    };
    let fetched = state.llm.list_models(&endpoint).await?;

    let id = kind.as_str();
    state.vault.set(id, credential).await?;
    save_connection(state, kind, ConnectionMethod::ApiKey, None, &fetched)?;
    // An account sign-in that was running is replaced by the key.
    state.accounts.dismiss(id);
    state.events.providers_changed();
    provider_view(state, id)
}

/// Saves a built-in provider's connection with the models discovered for
/// it. Switching methods replaces the model list: models are never assumed
/// to be the same across connections.
pub fn save_connection(
    state: &AppState,
    kind: ProviderKind,
    connection: ConnectionMethod,
    account_label: Option<String>,
    fetched: &[FetchedModel],
) -> AppResult<()> {
    let id = kind.as_str();
    let now = now_ms();
    state.db.call(|conn| {
        let tx = conn.transaction()?;
        let existing = repo::get(&tx, id)?;
        let fresh = existing.as_ref().is_none_or(|e| e.connection != connection);
        repo::upsert(
            &tx,
            &ProviderRow {
                id: id.into(),
                kind,
                name: kind.display_name().into(),
                base_url: None,
                model: None,
                auth_method: if connection == ConnectionMethod::ApiKey {
                    AuthMethod::ApiKey
                } else {
                    // The runtime keeps the account's credentials.
                    AuthMethod::None
                },
                connection,
                account_label,
                created_at: existing.map_or(now, |e| e.created_at),
                updated_at: now,
            },
        )?;
        sync_models(&tx, id, fetched, fresh, None)?;
        ensure_default_model(&tx)?;
        repo::delete_setting(&tx, &credits_key(id))?;
        tx.commit()?;
        Ok(())
    })?;
    state
        .accounts
        .set_health(id, ConnectionStatus::Connected, None);
    Ok(())
}

/// Removes a provider, its models and its stored credential. An account's
/// runtime stays signed in (see [`crate::services::accounts::sign_out`]).
pub async fn disconnect(state: &AppState, provider_id: &str) -> AppResult<()> {
    state.vault.delete(provider_id).await?;
    state.accounts.dismiss(provider_id);
    state
        .accounts
        .set_health(provider_id, ConnectionStatus::Connected, None);
    state.db.call(|conn| {
        let tx = conn.transaction()?;
        repo::delete(&tx, provider_id)?;
        repo::delete_setting(&tx, &credits_key(provider_id))?;
        ensure_default_model(&tx)?;
        tx.commit()?;
        Ok(())
    })?;
    state.events.providers_changed();
    Ok(())
}

/// Creates or updates an OpenAI-compatible endpoint.
pub async fn save_custom(state: &AppState, input: CustomProviderInput) -> AppResult<ProviderView> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(AppError::validation("Enter a name (up to 60 characters)."));
    }
    let base_url = input.base_url.trim().trim_end_matches('/').to_string();
    match reqwest::Url::parse(&base_url) {
        Ok(url) if matches!(url.scheme(), "http" | "https") && url.host().is_some() => {}
        _ => {
            return Err(AppError::validation(
                "Enter a valid base URL, e.g. http://localhost:11434/v1",
            ))
        }
    }
    let model = input.model.trim().to_string();
    if model.is_empty() || model.len() > 200 {
        return Err(AppError::validation("Enter the model name."));
    }

    let id = match &input.id {
        Some(id) => {
            let existing = state.db.call(|c| repo::get(c, id))?;
            match existing {
                Some(row) if row.kind == ProviderKind::OpenaiCompatible => id.clone(),
                _ => return Err(AppError::not_found("Endpoint not found")),
            }
        }
        None => state.db.call(|c| repo::next_custom_id(c))?,
    };

    // Credential: `None` keeps the stored key, "" removes it.
    let credential = match input.api_key.as_deref().map(str::trim) {
        Some("") => {
            state.vault.delete(&id).await?;
            None
        }
        Some(key) => {
            let credential = Credential::ApiKey {
                key: validate_api_key(key)?,
            };
            state.vault.set(&id, credential.clone()).await?;
            Some(credential)
        }
        None if input.id.is_some() => state.vault.get(&id).await?,
        None => None,
    };

    let endpoint = Endpoint {
        kind: ProviderKind::OpenaiCompatible,
        name: name.into(),
        connection: ConnectionMethod::ApiKey,
        base_url: base_url.clone(),
        credential: credential.clone(),
    };
    // Many local servers do not list models (or are not running yet); the
    // configured model is enough to chat.
    let fetched = state.llm.list_models(&endpoint).await.unwrap_or_default();

    let now = now_ms();
    state.db.call(|conn| {
        let tx = conn.transaction()?;
        let existing = repo::get(&tx, &id)?;
        repo::upsert(
            &tx,
            &ProviderRow {
                id: id.clone(),
                kind: ProviderKind::OpenaiCompatible,
                name: name.into(),
                base_url: Some(base_url.clone()),
                model: Some(model.clone()),
                auth_method: if credential.is_some() {
                    AuthMethod::ApiKey
                } else {
                    AuthMethod::None
                },
                connection: ConnectionMethod::ApiKey,
                account_label: None,
                created_at: existing.map_or(now, |e| e.created_at),
                updated_at: now,
            },
        )?;
        // Only the configured model is enabled; others can be enabled later.
        sync_models(&tx, &id, &fetched, false, Some(&model))?;
        ensure_default_model(&tx)?;
        tx.commit()?;
        Ok(())
    })?;
    state.events.providers_changed();
    provider_view(state, &id)
}

/// Re-fetches a provider's model list.
pub async fn refresh_models(state: &AppState, provider_id: &str) -> AppResult<ProviderView> {
    let endpoint = resolve_endpoint(state, provider_id).await?;
    let fetched = state.llm.list_models(&endpoint).await?;
    state.db.call(|conn| {
        let tx = conn.transaction()?;
        let pinned = repo::get(&tx, provider_id)?.and_then(|p| p.model);
        sync_models(&tx, provider_id, &fetched, false, pinned.as_deref())?;
        ensure_default_model(&tx)?;
        tx.commit()?;
        Ok(())
    })?;
    state.events.providers_changed();
    provider_view(state, provider_id)
}

/// Stores the fetched model list. With `auto_enable` (a cloud provider's
/// first connect) its recommended models are enabled; otherwise new models
/// start disabled and the user's choices are kept. `pinned` (an endpoint's
/// configured model) is always kept and enabled.
fn sync_models(
    conn: &rusqlite::Connection,
    provider_id: &str,
    fetched: &[FetchedModel],
    auto_enable: bool,
    pinned: Option<&str>,
) -> AppResult<()> {
    let mut auto_enabled = 0;
    let mut keep = Vec::new();
    for (index, model) in fetched.iter().enumerate() {
        let enable = auto_enable && model.recommended && auto_enabled < MAX_AUTO_ENABLED;
        if enable {
            auto_enabled += 1;
        }
        repo::upsert_model(
            conn,
            &ModelRow {
                provider_id: provider_id.into(),
                model_id: model.id.clone(),
                display_name: model.display_name.clone(),
                enabled: enable,
                max_output_tokens: model.max_output_tokens,
                sort_order: index as i64,
            },
        )?;
        keep.push(model.id.clone());
    }
    if let Some(pinned) = pinned {
        if !keep.iter().any(|id| id == pinned) {
            repo::upsert_model(
                conn,
                &ModelRow {
                    provider_id: provider_id.into(),
                    model_id: pinned.into(),
                    display_name: pinned.into(),
                    enabled: true,
                    max_output_tokens: None,
                    sort_order: -1,
                },
            )?;
            keep.push(pinned.to_string());
        }
        repo::set_model_enabled(conn, provider_id, pinned, true)?;
    }
    repo::retain_models(conn, provider_id, &keep)?;

    // Never leave a freshly connected provider without a usable model.
    if auto_enable
        && !repo::list_models(conn, provider_id)?
            .iter()
            .any(|m| m.enabled)
    {
        if let Some(first) = keep.first() {
            repo::set_model_enabled(conn, provider_id, first, true)?;
        }
    }
    Ok(())
}

pub fn set_model_enabled(state: &AppState, model: &ModelRef, enabled: bool) -> AppResult<()> {
    state.db.call(|conn| {
        let tx = conn.transaction()?;
        if !repo::set_model_enabled(&tx, &model.provider_id, &model.model_id, enabled)? {
            return Err(AppError::not_found("Model not found"));
        }
        ensure_default_model(&tx)?;
        tx.commit()?;
        Ok(())
    })?;
    state.events.providers_changed();
    Ok(())
}

pub fn set_default_model(state: &AppState, model: &ModelRef) -> AppResult<()> {
    state.db.call(|conn| {
        let tx = conn.transaction()?;
        if repo::get_model(&tx, &model.provider_id, &model.model_id)?.is_none() {
            return Err(AppError::not_found("Model not found"));
        }
        repo::set_model_enabled(&tx, &model.provider_id, &model.model_id, true)?;
        repo::set_setting(&tx, DEFAULT_MODEL_KEY, &encode(model))?;
        tx.commit()?;
        Ok(())
    })?;
    state.events.providers_changed();
    Ok(())
}

/// Models the user can choose in the chat, in provider order.
pub fn catalog(state: &AppState) -> AppResult<ModelCatalog> {
    state.db.call(|conn| {
        Ok(ModelCatalog {
            models: enabled_models(conn)?,
            default_model: default_model(conn)?,
        })
    })
}

fn enabled_models(conn: &rusqlite::Connection) -> AppResult<Vec<ModelOption>> {
    let mut rows = repo::list(conn)?;
    // Built-in providers first (OpenAI, Anthropic, Gemini), then endpoints.
    let rank = |r: &ProviderRow| {
        ProviderKind::BUILT_IN
            .iter()
            .position(|k| *k == r.kind)
            .unwrap_or(ProviderKind::BUILT_IN.len())
    };
    rows.sort_by_key(|r| (rank(r), r.created_at));
    let mut options = Vec::new();
    for row in rows {
        for model in repo::list_models(conn, &row.id)?
            .into_iter()
            .filter(|m| m.enabled)
        {
            options.push(ModelOption {
                model: ModelRef {
                    provider_id: row.id.clone(),
                    model_id: model.model_id,
                },
                provider_name: row.name.clone(),
                display_name: model.display_name,
            });
        }
    }
    Ok(options)
}

fn encode(model: &ModelRef) -> String {
    format!("{}/{}", model.provider_id, model.model_id)
}

fn default_model(conn: &rusqlite::Connection) -> AppResult<Option<ModelRef>> {
    Ok(
        repo::get_setting(conn, DEFAULT_MODEL_KEY)?.and_then(|value| {
            let (provider_id, model_id) = value.split_once('/')?;
            Some(ModelRef {
                provider_id: provider_id.into(),
                model_id: model_id.into(),
            })
        }),
    )
}

/// Keeps the default model valid: it must exist and be enabled; otherwise
/// the first enabled model becomes the default (or none is set).
fn ensure_default_model(conn: &rusqlite::Connection) -> AppResult<()> {
    let enabled = enabled_models(conn)?;
    let current = default_model(conn)?;
    if current.is_some_and(|d| enabled.iter().any(|o| o.model == d)) {
        return Ok(());
    }
    match enabled.first() {
        Some(first) => repo::set_setting(conn, DEFAULT_MODEL_KEY, &encode(&first.model)),
        None => repo::delete_setting(conn, DEFAULT_MODEL_KEY),
    }
}

/// Everything needed to call a provider: its endpoint and credential.
pub async fn resolve_endpoint(state: &AppState, provider_id: &str) -> AppResult<Endpoint> {
    let row = state
        .db
        .call(|c| repo::get(c, provider_id))?
        .ok_or_else(|| {
            AppError::configuration(
                "This model's provider is no longer connected. Choose another model.",
            )
        })?;
    let credential = match (row.connection, row.auth_method) {
        // Codex authenticates its own requests.
        (ConnectionMethod::ChatgptAccount, _) => None,
        // A short-lived token from the Anthropic CLI, never stored by ReMa.
        (ConnectionMethod::ClaudeConsole, _) => {
            let runtime = state
                .accounts
                .runtime(ConnectionMethod::ClaudeConsole)
                .ok_or_else(|| AppError::configuration("Claude Console sign-in is unavailable"))?;
            match runtime.credential().await {
                Ok(credential) => credential,
                Err(error) => {
                    if matches!(error, AppError::Authentication(_)) {
                        state.accounts.set_health(
                            &row.id,
                            ConnectionStatus::ReauthRequired,
                            Some(error.to_string()),
                        );
                        state.events.providers_changed();
                    }
                    return Err(error);
                }
            }
        }
        (ConnectionMethod::ApiKey, AuthMethod::None) => None,
        (ConnectionMethod::ApiKey, AuthMethod::ApiKey | AuthMethod::OAuth) => {
            let credential = state.vault.get(&row.id).await?.ok_or_else(|| {
                AppError::authentication(format!(
                    "No credentials are stored for {}. Reconnect it in Settings.",
                    row.name
                ))
            })?;
            if let Credential::OAuth {
                expires_at: Some(expires_at),
                ..
            } = &credential
            {
                if *expires_at <= now_ms() {
                    return Err(AppError::authentication(format!(
                        "The sign-in for {} has expired. Reconnect it in Settings.",
                        row.name
                    )));
                }
            }
            Some(credential)
        }
    };
    let base_url = row
        .base_url
        .clone()
        .or_else(|| Endpoint::default_base_url(row.kind))
        .ok_or_else(|| AppError::configuration(format!("{} has no base URL.", row.name)))?;
    Ok(Endpoint {
        kind: row.kind,
        name: row.name,
        connection: row.connection,
        base_url,
        credential,
    })
}

/// The provider-reported output limit for a model, if known.
pub fn max_output_tokens(state: &AppState, model: &ModelRef) -> AppResult<Option<u32>> {
    state.db.call(|c| {
        Ok(repo::get_model(c, &model.provider_id, &model.model_id)?
            .and_then(|m| m.max_output_tokens))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing};

    fn state() -> AppState {
        testing::state(Arc::new(FakeLanguageModel::replying(&[]))).0
    }

    fn model(provider_id: &str, model_id: &str) -> ModelRef {
        ModelRef {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
        }
    }

    #[tokio::test]
    async fn connects_a_provider_and_picks_a_default_model() {
        let state = state();
        let view = connect(&state, ProviderKind::Anthropic, " sk-ant-key ")
            .await
            .unwrap();

        assert!(view.configured && view.has_credential);
        let enabled: Vec<_> = view
            .models
            .iter()
            .filter(|m| m.enabled)
            .map(|m| &m.model_id)
            .collect();
        assert_eq!(enabled, ["model-a", "model-b"]);

        let catalog = catalog(&state).unwrap();
        assert_eq!(catalog.models.len(), 2);
        assert_eq!(catalog.default_model, Some(model("anthropic", "model-a")));

        // The key is in the credential store, not in the database.
        let stored = state.vault.get("anthropic").await.unwrap();
        assert_eq!(
            stored,
            Some(Credential::ApiKey {
                key: "sk-ant-key".into()
            })
        );
        let endpoint = resolve_endpoint(&state, "anthropic").await.unwrap();
        assert_eq!(endpoint.base_url, "https://api.anthropic.com/v1");
    }

    #[tokio::test]
    async fn rejects_invalid_keys_without_saving_anything() {
        let state = state();
        let error = connect(&state, ProviderKind::Openai, "bad-key")
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::Authentication(_)));
        assert!(state.vault.get("openai").await.unwrap().is_none());
        assert!(!settings(&state).unwrap().providers[0].configured);

        assert!(matches!(
            connect(&state, ProviderKind::Openai, "   ")
                .await
                .unwrap_err(),
            AppError::Validation(_)
        ));
    }

    #[tokio::test]
    async fn disconnecting_removes_models_credentials_and_default() {
        let state = state();
        connect(&state, ProviderKind::Openai, "k1").await.unwrap();
        connect(&state, ProviderKind::Gemini, "k2").await.unwrap();
        set_default_model(&state, &model("gemini", "model-c")).unwrap();

        disconnect(&state, "gemini").await.unwrap();
        assert!(state.vault.get("gemini").await.unwrap().is_none());
        let catalog = catalog(&state).unwrap();
        assert!(catalog
            .models
            .iter()
            .all(|m| m.model.provider_id == "openai"));
        assert_eq!(catalog.default_model, Some(model("openai", "model-a")));
    }

    #[tokio::test]
    async fn keeps_model_choices_on_refresh_and_moves_default_when_disabled() {
        let state = state();
        connect(&state, ProviderKind::Openai, "k").await.unwrap();
        set_model_enabled(&state, &model("openai", "model-c"), true).unwrap();
        set_model_enabled(&state, &model("openai", "model-a"), false).unwrap();

        // Default moved away from the disabled model.
        assert_eq!(
            catalog(&state).unwrap().default_model,
            Some(model("openai", "model-b"))
        );

        let view = refresh_models(&state, "openai").await.unwrap();
        let enabled: Vec<_> = view
            .models
            .iter()
            .filter(|m| m.enabled)
            .map(|m| m.model_id.as_str())
            .collect();
        assert_eq!(enabled, ["model-b", "model-c"]);
    }

    #[tokio::test]
    async fn saves_openai_compatible_endpoints() {
        let state = state();
        let view = save_custom(
            &state,
            CustomProviderInput {
                id: None,
                name: "Local".into(),
                base_url: "http://localhost:11434/v1/".into(),
                model: "my-model".into(),
                api_key: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(view.id, "custom-1");
        assert_eq!(view.base_url.as_deref(), Some("http://localhost:11434/v1"));
        assert!(!view.has_credential);
        let enabled: Vec<_> = view
            .models
            .iter()
            .filter(|m| m.enabled)
            .map(|m| m.model_id.as_str())
            .collect();
        assert_eq!(enabled, ["my-model"]);

        // Add a key later, then remove it again.
        let updated = save_custom(
            &state,
            CustomProviderInput {
                id: Some("custom-1".into()),
                name: "Local".into(),
                base_url: "http://localhost:11434/v1".into(),
                model: "my-model".into(),
                api_key: Some("local-secret".into()),
            },
        )
        .await
        .unwrap();
        assert!(updated.has_credential);
        let endpoint = resolve_endpoint(&state, "custom-1").await.unwrap();
        assert!(endpoint.credential.is_some());

        let cleared = save_custom(
            &state,
            CustomProviderInput {
                id: Some("custom-1".into()),
                name: "Local".into(),
                base_url: "http://localhost:11434/v1".into(),
                model: "my-model".into(),
                api_key: Some(String::new()),
            },
        )
        .await
        .unwrap();
        assert!(!cleared.has_credential);
    }

    #[tokio::test]
    async fn validates_endpoint_input() {
        let state = state();
        let input = |base_url: &str, model: &str| CustomProviderInput {
            id: None,
            name: "Local".into(),
            base_url: base_url.into(),
            model: model.into(),
            api_key: None,
        };
        for bad in [
            input("localhost:1234", "m"),
            input("ftp://x/v1", "m"),
            input("http://x/v1", " "),
        ] {
            assert!(matches!(
                save_custom(&state, bad).await.unwrap_err(),
                AppError::Validation(_)
            ));
        }
    }

    #[tokio::test]
    async fn resolving_a_removed_provider_is_a_clear_error() {
        let state = state();
        let error = resolve_endpoint(&state, "openai").await.unwrap_err();
        assert!(matches!(error, AppError::Configuration(_)));
    }
}
