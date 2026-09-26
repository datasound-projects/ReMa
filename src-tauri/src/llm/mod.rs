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

use std::{fmt, future::Future, pin::Pin, sync::Arc};

use reqwest::RequestBuilder;
use serde_json::Value;
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

/// A tool the model may call during this request.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    /// Unique within the request; `^[a-zA-Z0-9_-]{1,64}$` (every provider
    /// accepts it).
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments.
    pub input_schema: Value,
}

/// A tool call the model made.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The arguments; a string holds arguments that were not valid JSON.
    pub arguments: Value,
    /// Opaque provider data that must accompany the call when it is sent
    /// back (Gemini's thought signatures).
    pub provider_data: Option<Value>,
}

/// What a tool call returned, given back to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: message.into(),
            is_error: true,
        }
    }
}

/// One step of tool use within an answer: the model's text and calls, and
/// what the tools returned.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRound {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub outputs: Vec<ToolOutput>,
}

/// Runs the model's tool calls (for chat: MCP tools, with approvals).
pub trait ToolExecutor: Send + Sync {
    fn execute<'a>(&'a self, call: &'a ToolCall) -> BoxFuture<'a, ToolOutput>;
}

/// The tools of a request and who runs them.
#[derive(Clone)]
pub struct ToolBox {
    pub specs: Vec<ToolSpec>,
    pub executor: Arc<dyn ToolExecutor>,
}

impl fmt::Debug for ToolBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolBox")
            .field("specs", &self.specs)
            .finish_non_exhaustive()
    }
}

/// Most model calls in one answer when tools are used.
pub const MAX_TOOL_ROUNDS: usize = 8;

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub system: Option<String>,
    pub turns: Vec<Turn>,
    /// Output cap reported by the provider for this model, if known.
    pub max_output_tokens: Option<u32>,
    /// Tools the model may call (none unless the user selected MCP servers).
    pub tools: Option<ToolBox>,
    /// Tool use so far in this answer, after `turns`.
    pub rounds: Vec<ToolRound>,
}

impl ChatRequest {
    pub fn tool_specs(&self) -> &[ToolSpec] {
        self.tools.as_ref().map_or(&[], |t| t.specs.as_slice())
    }
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

/// Part of a tool call in a stream.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ToolDelta {
    /// Pieces with the same index belong to one call; `None` is a complete
    /// call on its own.
    pub index: Option<usize>,
    pub id: Option<String>,
    pub name: Option<String>,
    /// JSON text (a fragment when the call streams in pieces).
    pub arguments: String,
    pub provider_data: Option<Value>,
}

/// What one stream event contributed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StreamPiece {
    pub text: Option<String>,
    pub finish: Option<Finish>,
    /// The provider signalled the end of the stream.
    pub done: bool,
    pub tools: Vec<ToolDelta>,
}

/// Collects streamed tool-call pieces into calls.
#[derive(Default)]
struct CallCollector {
    calls: Vec<(Option<usize>, ToolDelta)>,
}

impl CallCollector {
    fn push(&mut self, delta: ToolDelta) {
        if let Some(index) = delta.index {
            if let Some((_, call)) = self.calls.iter_mut().find(|(i, _)| *i == Some(index)) {
                if call.id.is_none() {
                    call.id = delta.id;
                }
                if call.name.is_none() {
                    call.name = delta.name;
                }
                call.arguments.push_str(&delta.arguments);
                if delta.provider_data.is_some() {
                    call.provider_data = delta.provider_data;
                }
                return;
            }
        }
        self.calls.push((delta.index, delta));
    }

    fn finish(self) -> Vec<ToolCall> {
        self.calls
            .into_iter()
            .enumerate()
            .filter_map(|(n, (_, delta))| {
                let name = delta.name.filter(|n| !n.is_empty())?;
                let raw = delta.arguments.trim();
                let arguments = if raw.is_empty() {
                    Value::Object(Default::default())
                } else {
                    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
                };
                Some(ToolCall {
                    id: delta
                        .id
                        .filter(|id| !id.is_empty())
                        .unwrap_or_else(|| format!("call_{n}")),
                    name,
                    arguments,
                    provider_data: delta.provider_data,
                })
            })
            .collect()
    }
}

impl StreamPiece {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::default()
        }
    }

    fn tool(delta: ToolDelta) -> Self {
        Self {
            tools: vec![delta],
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
                // The Codex runtime runs the tool loop itself (dynamic tools).
                return self
                    .codex()?
                    .stream_chat(model_id, request, cancel, on_delta)
                    .await;
            }
            let Some(tools) = request.tools.as_ref() else {
                return self
                    .stream_once(endpoint, model_id, request, &cancel, on_delta)
                    .await
                    .map(|(finish, _)| finish);
            };
            // Tools: call the model, run the tools it asks for, and call it
            // again with the results, until it answers without tools.
            let mut request = request.clone();
            let mut separate = false;
            for _ in 0..=MAX_TOOL_ROUNDS {
                let mut round_text = String::new();
                let (finish, calls) = {
                    let mut forward = |text: &str| {
                        // A blank line between the text of successive steps.
                        if separate && round_text.is_empty() && !text.trim().is_empty() {
                            on_delta("\n\n");
                        }
                        round_text.push_str(text);
                        on_delta(text);
                    };
                    self.stream_once(endpoint, model_id, &request, &cancel, &mut forward)
                        .await?
                };
                separate |= !round_text.trim().is_empty();
                if calls.is_empty() || finish != Finish::Complete {
                    return Ok(finish);
                }
                if request.rounds.len() == MAX_TOOL_ROUNDS {
                    return Err(AppError::provider(
                        "The model kept calling tools without answering. Try a narrower request.",
                    ));
                }
                let mut outputs = Vec::with_capacity(calls.len());
                for call in &calls {
                    if cancel.is_cancelled() {
                        return Ok(Finish::Cancelled);
                    }
                    outputs.push(tools.executor.execute(call).await);
                }
                if cancel.is_cancelled() {
                    return Ok(Finish::Cancelled);
                }
                request.rounds.push(ToolRound {
                    text: round_text,
                    calls,
                    outputs,
                });
            }
            Err(AppError::provider("Too many tool calls in one answer."))
        })
    }
}

impl ProviderLanguageModel {
    /// One HTTPS call to the model: streams its text and returns how it
    /// finished and the tools it called.
    async fn stream_once(
        &self,
        endpoint: &Endpoint,
        model_id: &str,
        request: &ChatRequest,
        cancel: &CancellationToken,
        on_delta: DeltaSink<'_>,
    ) -> AppResult<(Finish, Vec<ToolCall>)> {
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
        let Some(response) = http::send(http_request, cancel, &endpoint.name, &secrets).await?
        else {
            return Ok((Finish::Cancelled, Vec::new()));
        };
        drive_stream(response, cancel, on_delta, parse).await
    }
}

/// Reads a provider stream, forwarding text and collecting tool calls.
async fn drive_stream(
    response: reqwest::Response,
    cancel: &CancellationToken,
    on_delta: DeltaSink<'_>,
    parse: fn(&SseEvent) -> AppResult<StreamPiece>,
) -> AppResult<(Finish, Vec<ToolCall>)> {
    let mut finish = Finish::Complete;
    let mut calls = CallCollector::default();
    let end = read_sse(response, cancel, |event| {
        let piece = parse(&event)?;
        if let Some(text) = piece.text.as_deref().filter(|t| !t.is_empty()) {
            on_delta(text);
        }
        if let Some(reason) = piece.finish {
            finish = reason;
        }
        for delta in piece.tools {
            calls.push(delta);
        }
        Ok(if piece.done {
            Flow::Stop
        } else {
            Flow::Continue
        })
    })
    .await?;
    Ok(match end {
        StreamEnd::Cancelled => (Finish::Cancelled, Vec::new()),
        StreamEnd::Completed => (finish, calls.finish()),
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
