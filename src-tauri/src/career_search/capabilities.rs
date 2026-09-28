//! What the selected model's runtime can do for search (§5–§6, §58, §67).
//!
//! Signing in says only that the model can answer. Whether it can search
//! the web itself — live, on chosen sites, with citations — depends on the
//! runtime ReMa uses for that connection, never on the provider's name: an
//! OpenAI API key goes through the Responses API, a ChatGPT account through
//! the Codex runtime; Claude with an API key or a Console sign-in goes
//! through the Messages API; a local server has no web access unless it is
//! Unsloth Studio. ReMa's own career search is there for every model.
//!
//! Capabilities come from what the runtime reports (Codex's web search
//! policy for the account, whether a local server is Unsloth Studio), the
//! model's tool versions, and what earlier searches showed (a provider that
//! refused its search is remembered for a while). No billable search is run
//! to find out.

use std::time::Duration;

use serde::Serialize;
use specta::Type;

use crate::{
    llm::{anthropic, Endpoint},
    models::provider::{ConnectionMethod, ProviderKind},
    state::AppState,
};

/// How long a provider's refusal of its own search is remembered (an
/// administrator may turn it back on).
pub const REFUSAL_TTL: Duration = Duration::from_secs(30 * 60);

/// How the model is reached and signed in (authentication only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    ApiKey,
    ChatgptAccount,
    ClaudeConsole,
    /// A local or self-hosted server (a key is optional).
    LocalServer,
}

/// The runtime that carries the model's requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeType {
    OpenaiResponses,
    CodexAppServer,
    AnthropicMessages,
    GeminiApi,
    UnslothStudio,
    OpenaiCompatible,
}

impl RuntimeType {
    /// The name diagnostics use.
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenaiResponses => "responses",
            Self::CodexAppServer => "codex-app-server",
            Self::AnthropicMessages => "messages",
            Self::GeminiApi => "gemini",
            Self::UnslothStudio => "unsloth-studio",
            Self::OpenaiCompatible => "openai-compatible",
        }
    }
}

impl AuthMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ApiKey => "api-key",
            Self::ChatgptAccount => "chatgpt",
            Self::ClaudeConsole => "claude-console",
            Self::LocalServer => "local",
        }
    }
}

/// `ProviderRuntimeCapabilities` (§6) of one model.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    /// "OpenAI", "Anthropic", …
    pub provider: String,
    pub auth_mode: AuthMode,
    pub runtime: RuntimeType,
    /// The connection can answer: a key or a signed-in runtime is there.
    pub inference_available: bool,
    /// The model's own hosted search can be used.
    pub native_search_available: bool,
    /// It reads the live web, not only a cached index.
    pub native_search_live: bool,
    /// It can be kept to the career sites of a request.
    pub native_domain_filtering: bool,
    /// It reports the pages it used, so ReMa can check them.
    pub native_citations: bool,
    /// ReMa's own career search (always).
    pub rema_search_available: bool,
    /// The tool or mode ReMa uses ("web_search_20260318", "live").
    pub native_detail: Option<String>,
    /// Why the model's own search is off or limited.
    pub note: Option<String>,
}

/// The key that ties what was learned about a provider's search to it.
pub fn provider_key(endpoint: &Endpoint) -> String {
    format!(
        "{}|{}|{}",
        endpoint.kind.as_str(),
        endpoint.connection.as_str(),
        endpoint.base_url.trim_end_matches('/').to_lowercase()
    )
}

/// Whether a failure of the provider's search means the account or
/// organization may not search (as opposed to a passing error).
pub fn is_refusal(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    [
        "not enabled",
        "turned off",
        "does not offer web search",
        "web search is not available",
        "web search is disabled",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// The capabilities of a model, from its runtime (no search is run).
pub async fn detect(state: &AppState, endpoint: &Endpoint, model_id: &str) -> RuntimeCapabilities {
    let mut caps = base(endpoint, model_id);
    if endpoint.connection == ConnectionMethod::ChatgptAccount {
        // Codex knows the account's policy: live, indexed, cached or none.
        match state.llm.web_search_policy(endpoint).await {
            Some(Ok(mode)) => {
                caps.native_search_live = mode == "live";
                if mode != "live" {
                    caps.note = Some(format!(
                        "this ChatGPT workspace allows only {mode} web results"
                    ));
                }
                caps.native_detail = Some(mode);
            }
            Some(Err(reason)) => {
                caps.native_search_available = false;
                caps.native_search_live = false;
                caps.note = Some(format!("not available for this ChatGPT account: {reason}"));
            }
            None => {}
        }
    }
    if let Some(reason) = state.career.refusal(&provider_key(endpoint)) {
        caps.native_search_available = false;
        caps.native_search_live = false;
        caps.note = Some(format!("turned off by the provider ({reason})"));
    }
    caps
}

/// What the runtime offers by its kind, transport and model.
pub fn base(endpoint: &Endpoint, model_id: &str) -> RuntimeCapabilities {
    let auth_mode = match (endpoint.kind, endpoint.connection) {
        (_, ConnectionMethod::ChatgptAccount) => AuthMode::ChatgptAccount,
        (_, ConnectionMethod::ClaudeConsole) => AuthMode::ClaudeConsole,
        (ProviderKind::OpenaiCompatible, _) => AuthMode::LocalServer,
        _ => AuthMode::ApiKey,
    };
    let inference_available = match endpoint.connection {
        // The runtime owns the sign-in; resolving the endpoint checked it.
        ConnectionMethod::ChatgptAccount => true,
        _ => endpoint.credential.is_some() || endpoint.kind == ProviderKind::OpenaiCompatible,
    };
    let mut caps = RuntimeCapabilities {
        provider: endpoint.kind.display_name().to_string(),
        auth_mode,
        runtime: RuntimeType::OpenaiCompatible,
        inference_available,
        native_search_available: false,
        native_search_live: false,
        native_domain_filtering: false,
        native_citations: false,
        rema_search_available: true,
        native_detail: None,
        note: None,
    };
    match (endpoint.kind, endpoint.connection) {
        (ProviderKind::Openai, ConnectionMethod::ChatgptAccount) => {
            caps.runtime = RuntimeType::CodexAppServer;
            caps.native_search_available = true;
            caps.native_search_live = true;
            // Codex's `tools.web_search.allowed_domains`.
            caps.native_domain_filtering = true;
            // Codex reports queries and opened pages, not its results.
            caps.native_citations = false;
            caps.native_detail = Some("live".into());
        }
        (ProviderKind::Openai, _) => {
            caps.runtime = RuntimeType::OpenaiResponses;
            caps.native_search_available = true;
            caps.native_search_live = true;
            caps.native_domain_filtering = true;
            caps.native_citations = true;
            caps.native_detail = Some("web_search".into());
        }
        (ProviderKind::Anthropic, _) => {
            caps.runtime = RuntimeType::AnthropicMessages;
            caps.native_search_available = true;
            caps.native_search_live = true;
            caps.native_domain_filtering = true;
            caps.native_citations = true;
            caps.native_detail = Some(anthropic::web_tool_choice(model_id).search.to_string());
        }
        (ProviderKind::Gemini, _) => {
            caps.runtime = RuntimeType::GeminiApi;
            caps.native_search_available = true;
            caps.native_search_live = true;
            caps.native_citations = true;
            caps.native_detail = Some("google_search".into());
        }
        (ProviderKind::OpenaiCompatible, _) if endpoint.server_web_search => {
            caps.runtime = RuntimeType::UnslothStudio;
            caps.native_search_available = true;
            caps.native_search_live = true;
            caps.native_citations = true;
            caps.native_detail = Some("web_search".into());
        }
        (ProviderKind::OpenaiCompatible, _) => {
            caps.note = Some("no web search of its own; ReMa searches for it".into());
        }
    }
    caps
}

/// One diagnostic line (§66): runtime facts only — no query, no page text,
/// no credential. Without a model, ReMa's own search ran alone.
pub fn log_line(
    caps: Option<&RuntimeCapabilities>,
    event: &str,
    fields: &[(&str, String)],
) -> String {
    let mut line = format!("[career-search] {event}");
    match caps {
        Some(caps) => {
            line.push_str(&format!(
                " provider={} auth={} runtime={} native_search={}",
                caps.provider.to_lowercase().replace(' ', "-"),
                caps.auth_mode.label(),
                caps.runtime.label(),
                caps.native_search_available
            ));
            if let Some(detail) = &caps.native_detail {
                line.push_str(&format!(" native_mode={}", detail.replace(' ', "_")));
            }
            line.push_str(&format!(
                " live={} domain_filter={} citations={}",
                caps.native_search_live, caps.native_domain_filtering, caps.native_citations
            ));
        }
        None => line.push_str(" provider=none native_search=false"),
    }
    line.push_str(" rema_search=true");
    for (key, value) in fields {
        line.push_str(&format!(" {key}={value}"));
    }
    line
}

/// [`provision`] in the background (after a sign-in or a saved server,
/// without delaying the answer to the user).
pub fn provision_later(state: &AppState, provider_id: &str) {
    let state = state.clone();
    let provider_id = provider_id.to_string();
    tauri::async_runtime::spawn(async move {
        provision(&state, &provider_id).await;
    });
}

/// Prepares search for a provider once it is connected or changed (§58):
/// the runtime is asked what it may do and the result is logged. Nothing
/// for the user to set up.
pub async fn provision(state: &AppState, provider_id: &str) -> Option<RuntimeCapabilities> {
    let endpoint = crate::services::providers::resolve_endpoint(state, provider_id)
        .await
        .ok()?;
    let model_id = crate::services::providers::catalog(state)
        .ok()
        .and_then(|c| {
            c.models
                .iter()
                .find(|m| m.model.provider_id == provider_id)
                .map(|m| m.model.model_id.clone())
        })
        .unwrap_or_default();
    let caps = detect(state, &endpoint, &model_id).await;
    // The lightweight health check of §67: metadata only, nothing billed.
    let inference = if model_id.is_empty() {
        "no-models"
    } else {
        "ready"
    };
    let jobs_mcp = if crate::rema_mcp::is_enabled(state) {
        "ready"
    } else {
        "off"
    };
    eprintln!(
        "{}",
        log_line(
            Some(&caps),
            "ready",
            &[
                ("inference", inference.to_string()),
                ("jobs_mcp", jobs_mcp.to_string()),
            ]
        )
    );
    Some(caps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::Credential;

    fn endpoint(kind: ProviderKind, connection: ConnectionMethod) -> Endpoint {
        Endpoint {
            kind,
            name: kind.display_name().into(),
            connection,
            base_url: "https://api.example.com/v1".into(),
            credential: (connection == ConnectionMethod::ApiKey).then(|| Credential::ApiKey {
                key: "sk-test".into(),
            }),
            server_web_search: false,
        }
    }

    #[test]
    fn capability_follows_the_runtime_not_the_provider_name() {
        let api = base(
            &endpoint(ProviderKind::Openai, ConnectionMethod::ApiKey),
            "gpt-6",
        );
        let codex = base(
            &endpoint(ProviderKind::Openai, ConnectionMethod::ChatgptAccount),
            "gpt-6",
        );
        assert_eq!(api.runtime, RuntimeType::OpenaiResponses);
        assert_eq!(codex.runtime, RuntimeType::CodexAppServer);
        assert_eq!(codex.auth_mode, AuthMode::ChatgptAccount);
        assert!(api.native_citations && !codex.native_citations);
        assert_eq!(codex.native_detail.as_deref(), Some("live"));

        let claude = base(
            &endpoint(ProviderKind::Anthropic, ConnectionMethod::ClaudeConsole),
            "claude-opus-5",
        );
        assert_eq!(claude.runtime, RuntimeType::AnthropicMessages);
        assert_eq!(claude.auth_mode, AuthMode::ClaudeConsole);
        assert_eq!(claude.native_detail.as_deref(), Some("web_search_20260318"));
        let haiku = base(
            &endpoint(ProviderKind::Anthropic, ConnectionMethod::ApiKey),
            "claude-haiku-4-5",
        );
        assert_eq!(haiku.native_detail.as_deref(), Some("web_search_20250305"));

        let local = base(
            &endpoint(ProviderKind::OpenaiCompatible, ConnectionMethod::ApiKey),
            "qwen3-4b",
        );
        assert!(!local.native_search_available);
        assert!(local.rema_search_available, "ReMa's search is always there");
        assert_eq!(local.auth_mode, AuthMode::LocalServer);
        let mut unsloth_endpoint =
            endpoint(ProviderKind::OpenaiCompatible, ConnectionMethod::ApiKey);
        unsloth_endpoint.server_web_search = true;
        let unsloth = base(&unsloth_endpoint, "qwen3-4b");
        assert_eq!(unsloth.runtime, RuntimeType::UnslothStudio);
        assert!(unsloth.native_search_available);
    }

    #[test]
    fn refusals_are_told_apart_from_passing_errors() {
        assert!(is_refusal(
            "Anthropic web search: Anthropic: web search is not enabled for this organization (400)"
        ));
        assert!(is_refusal(
            "ChatGPT web search: web search is turned off for this ChatGPT workspace by its \
             administrator"
        ));
        assert!(!is_refusal(
            "Anthropic web search: the search took too long"
        ));
        assert!(!is_refusal(
            "OpenAI web search: could not reach the service"
        ));
    }

    #[test]
    fn diagnostics_carry_no_secrets_or_queries() {
        let caps = base(
            &endpoint(ProviderKind::Anthropic, ConnectionMethod::ApiKey),
            "claude-sonnet-5",
        );
        let line = log_line(
            Some(&caps),
            "search",
            &[
                ("requirement", "required".into()),
                ("executed", "true".into()),
            ],
        );
        assert!(line.starts_with("[career-search] search provider=anthropic auth=api-key"));
        assert!(line.contains("runtime=messages"));
        assert!(line.contains("native_mode=web_search_20260318"));
        assert!(line.contains("requirement=required executed=true"));
        assert!(!line.contains("sk-test"));
        let alone = log_line(None, "search", &[("jobs_mcp", "true".into())]);
        assert_eq!(
            alone,
            "[career-search] search provider=none native_search=false rema_search=true jobs_mcp=true"
        );
    }
}
