//! LLM provider layer.
//!
//! The rest of ReMa talks to [`LanguageModel`] with provider-neutral types.
//! Four concepts stay separate:
//!
//! - **provider** ([`ProviderKind`]) — who serves the model;
//! - **transport** — how requests travel: the provider's HTTPS API (the
//!   `openai`, `anthropic` and `gemini` adapters) or the provider's official
//!   local runtime (`accounts::codex` for a ChatGPT account);
//! - **authentication** ([`ConnectionMethod`]) — an API key from the OS
//!   credential store, or an account sign-in owned by a runtime;
//! - **model** — discovered per connection, never assumed: a ChatGPT
//!   account and an OpenAI API key can offer different models.
//!
//! Each HTTP adapter only knows how to build its requests and parse its
//! stream events; streaming, cancellation and error mapping are shared. The
//! OpenAI adapter also serves every OpenAI-compatible endpoint (Ollama, LM
//! Studio, vLLM, …).

pub mod anthropic;
pub mod gemini;
pub mod http;
pub mod openai;
pub mod sse;

use std::{future::Future, pin::Pin, sync::Arc};

use reqwest::RequestBuilder;
use tokio_util::sync::CancellationToken;

use crate::{
    accounts::codex::CodexRuntime,
    error::{AppError, AppResult},
    models::{
        chat::MessageRole,
        provider::{ConnectionMethod, ProviderKind},
    },
    secrets::Credential,
};
use http::{read_sse, Flow, StreamEnd};
use sse::SseEvent;

/// One message of the conversation sent to a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub system: Option<String>,
    pub turns: Vec<Turn>,
    /// Output cap reported by the provider for this model, if known.
    pub max_output_tokens: Option<u32>,
}

impl ChatRequest {
    /// Turns in the shape every provider accepts: no empty messages,
    /// strictly alternating roles (consecutive ones merged), starting with
    /// the user.
    pub fn normalized_turns(&self) -> Vec<Turn> {
        let mut turns: Vec<Turn> = Vec::new();
        for turn in &self.turns {
            let content = turn.content.trim();
            if content.is_empty() {
                continue;
            }
            if turns.is_empty() && turn.role == MessageRole::Assistant {
                continue;
            }
            match turns.last_mut() {
                Some(last) if last.role == turn.role => {
                    last.content.push_str("\n\n");
                    last.content.push_str(content);
                }
                _ => turns.push(Turn {
                    role: turn.role,
                    content: content.to_string(),
                }),
            }
        }
        turns
    }
}

/// Why a stream ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    Complete,
    /// The model hit its output limit; the text is truncated.
    MaxTokens,
    /// The provider declined to answer (safety filters).
    Refused,
    /// ReMa cancelled the request (stop button, shutdown).
    Cancelled,
}

/// Where and how to reach a provider. Built by `services::providers`.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub kind: ProviderKind,
    /// Display name used in error messages.
    pub name: String,
    /// The transport and authentication in use.
    pub connection: ConnectionMethod,
    /// HTTPS transports only.
    pub base_url: String,
    /// Sent with HTTPS requests: an API key, or a Claude Console access
    /// token from the Anthropic CLI. `None` when a runtime authenticates.
    pub credential: Option<Credential>,
}

impl Endpoint {
    /// A built-in provider's API. Debug builds accept a local mock server
    /// (`REMA_ANTHROPIC_BASE_URL`, …) for end-to-end tests.
    pub fn default_base_url(kind: ProviderKind) -> Option<String> {
        let official = match kind {
            ProviderKind::Openai => openai::DEFAULT_BASE_URL,
            ProviderKind::Anthropic => anthropic::DEFAULT_BASE_URL,
            ProviderKind::Gemini => gemini::DEFAULT_BASE_URL,
            ProviderKind::OpenaiCompatible => return None,
        };
        #[cfg(debug_assertions)]
        if let Ok(url) = std::env::var(format!("REMA_{}_BASE_URL", kind.as_str().to_uppercase())) {
            return Some(url);
        }
        Some(official.to_string())
    }

    fn secrets(&self) -> Vec<&str> {
        self.credential
            .as_ref()
            .map(Credential::secret_values)
            .unwrap_or_default()
    }

    /// `Authorization: Bearer …` for API keys and OAuth tokens alike.
    fn bearer(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.credential {
            Some(Credential::ApiKey { key }) => request.bearer_auth(key),
            Some(Credential::OAuth { access_token, .. }) => request.bearer_auth(access_token),
            None => request,
        }
    }
}

/// A model reported by a provider's model list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedModel {
    pub id: String,
    pub display_name: String,
    pub max_output_tokens: Option<u32>,
    /// Suggested to enable by default when the provider is first connected.
    pub recommended: bool,
}

/// What one stream event contributed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StreamPiece {
    pub text: Option<String>,
    pub finish: Option<Finish>,
    /// The provider signalled the end of the stream.
    pub done: bool,
}

impl StreamPiece {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::default()
        }
    }

    fn done() -> Self {
        Self {
            done: true,
            ..Self::default()
        }
    }
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Receives streamed text as it arrives.
pub type DeltaSink<'a> = &'a mut (dyn FnMut(&str) + Send);

/// The provider-neutral interface used by chat and the scheduler.
pub trait LanguageModel: Send + Sync {
    fn list_models<'a>(
        &'a self,
        endpoint: &'a Endpoint,
    ) -> BoxFuture<'a, AppResult<Vec<FetchedModel>>>;

    fn stream_chat<'a>(
        &'a self,
        endpoint: &'a Endpoint,
        model_id: &'a str,
        request: &'a ChatRequest,
        cancel: CancellationToken,
        on_delta: DeltaSink<'a>,
    ) -> BoxFuture<'a, AppResult<Finish>>;
}

/// The real implementation: HTTPS APIs, or the Codex runtime for a
/// ChatGPT account.
pub struct ProviderLanguageModel {
    http: reqwest::Client,
    codex: Option<Arc<CodexRuntime>>,
}

impl ProviderLanguageModel {
    pub fn new(codex: Option<Arc<CodexRuntime>>) -> Self {
        Self {
            http: http::client(),
            codex,
        }
    }

    fn codex(&self) -> AppResult<&CodexRuntime> {
        self.codex
            .as_deref()
            .ok_or_else(|| AppError::configuration("ChatGPT sign-in is not available"))
    }
}

impl LanguageModel for ProviderLanguageModel {
    fn list_models<'a>(
        &'a self,
        endpoint: &'a Endpoint,
    ) -> BoxFuture<'a, AppResult<Vec<FetchedModel>>> {
        Box::pin(async move {
            if endpoint.connection == ConnectionMethod::ChatgptAccount {
                return self.codex()?.list_models().await;
            }
            match endpoint.kind {
                ProviderKind::Openai | ProviderKind::OpenaiCompatible => {
                    openai::list_models(&self.http, endpoint).await
                }
                ProviderKind::Anthropic => anthropic::list_models(&self.http, endpoint).await,
                ProviderKind::Gemini => gemini::list_models(&self.http, endpoint).await,
            }
        })
    }

    fn stream_chat<'a>(
        &'a self,
        endpoint: &'a Endpoint,
        model_id: &'a str,
        request: &'a ChatRequest,
        cancel: CancellationToken,
        on_delta: DeltaSink<'a>,
    ) -> BoxFuture<'a, AppResult<Finish>> {
        Box::pin(async move {
            if endpoint.connection == ConnectionMethod::ChatgptAccount {
                return self
                    .codex()?
                    .stream_chat(model_id, request, cancel, on_delta)
                    .await;
            }
            let secrets = endpoint.secrets();
            let (http_request, parse): (RequestBuilder, fn(&SseEvent) -> AppResult<StreamPiece>) =
                match endpoint.kind {
                    ProviderKind::Openai | ProviderKind::OpenaiCompatible => (
                        openai::chat_request(&self.http, endpoint, model_id, request),
                        openai::parse_event,
                    ),
                    ProviderKind::Anthropic => (
                        anthropic::chat_request(&self.http, endpoint, model_id, request),
                        anthropic::parse_event,
                    ),
                    ProviderKind::Gemini => (
                        gemini::chat_request(&self.http, endpoint, model_id, request),
                        gemini::parse_event,
                    ),
                };
            let Some(response) =
                http::send(http_request, &cancel, &endpoint.name, &secrets).await?
            else {
                return Ok(Finish::Cancelled);
            };
            drive_stream(response, &cancel, on_delta, parse).await
        })
    }
}

/// Reads a provider stream, forwarding text and tracking how it finished.
async fn drive_stream(
    response: reqwest::Response,
    cancel: &CancellationToken,
    on_delta: DeltaSink<'_>,
    parse: fn(&SseEvent) -> AppResult<StreamPiece>,
) -> AppResult<Finish> {
    let mut finish = Finish::Complete;
    let end = read_sse(response, cancel, |event| {
        let piece = parse(&event)?;
        if let Some(text) = piece.text.as_deref().filter(|t| !t.is_empty()) {
            on_delta(text);
        }
        if let Some(reason) = piece.finish {
            finish = reason;
        }
        Ok(if piece.done {
            Flow::Stop
        } else {
            Flow::Continue
        })
    })
    .await?;
    Ok(match end {
        StreamEnd::Cancelled => Finish::Cancelled,
        StreamEnd::Completed => finish,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(role: MessageRole, content: &str) -> Turn {
        Turn {
            role,
            content: content.into(),
        }
    }

    #[test]
    fn normalizes_turns_for_strict_providers() {
        let request = ChatRequest {
            turns: vec![
                turn(MessageRole::Assistant, "leading reply"),
                turn(MessageRole::User, "first"),
                turn(MessageRole::User, "second"),
                turn(MessageRole::Assistant, "  "),
                turn(MessageRole::Assistant, "answer"),
            ],
            ..ChatRequest::default()
        };
        assert_eq!(
            request.normalized_turns(),
            vec![
                turn(MessageRole::User, "first\n\nsecond"),
                turn(MessageRole::Assistant, "answer"),
            ]
        );
    }
}

#[cfg(test)]
mod tests_http;

#[cfg(test)]
pub mod fake;
