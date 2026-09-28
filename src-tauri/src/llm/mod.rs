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
//! stream events; streaming, cancellation and error mapping are shared.
//! OpenAI's own API is reached through the Responses API (`openai_responses`);
//! the Chat Completions adapter (`openai`) serves every OpenAI-compatible
//! endpoint (Ollama, LM Studio, vLLM, …).
//!
//! **Web search** is the provider's own hosted tool, run on the provider's
//! side: Codex's live web search for a ChatGPT account, OpenAI's
//! `web_search`, Anthropic's `web_search`/`web_fetch` server tools and
//! Gemini's Google Search grounding. ReMa never runs searches itself; it
//! only reports what the model searched and read ([`WebEvent`]).

pub mod anthropic;
pub mod gemini;
pub mod http;
pub mod openai;
pub mod openai_responses;
pub mod sse;

use std::{
    collections::HashMap,
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use reqwest::RequestBuilder;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{
    accounts::{codex::CodexRuntime, AccountRuntime},
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
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolRound {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub outputs: Vec<ToolOutput>,
    /// The provider's own record of this step, sent back verbatim when set
    /// (Anthropic's content blocks, which carry server tool results).
    pub content: Option<Value>,
    /// The provider paused its own tool loop (Anthropic `pause_turn`): the
    /// step is sent back as is so the model continues where it stopped.
    pub paused: bool,
}

/// Something the model did on the web while answering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebEvent {
    /// A search or page visit began (`target` is the query or the URL; it
    /// may be empty until the provider reports it).
    Started {
        id: String,
        kind: WebKind,
        target: String,
    },
    /// It finished, with the pages found or opened, or an error.
    Finished {
        id: String,
        kind: WebKind,
        target: String,
        sources: Vec<WebSource>,
        error: Option<String>,
    },
    /// The answer cites a page (OpenAI URL citations, Anthropic web
    /// citations), with the cited words when the provider gives them
    /// (Anthropic's `cited_text`) and the cited span of the answer
    /// (OpenAI's `start_index`..`end_index`, in characters).
    Cited {
        url: String,
        title: String,
        quote: Option<String>,
        range: Option<(u32, u32)>,
    },
    /// Web search could not be used for this answer.
    Unavailable { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebKind {
    Search,
    Page,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSource {
    pub title: String,
    pub url: String,
}

/// Receives [`WebEvent`]s as they happen (chat shows them with the answer).
pub trait WebObserver: Send + Sync {
    fn observe(&self, event: WebEvent);
}

/// Lets the model search the web with its provider's hosted search.
#[derive(Clone, Default)]
pub struct WebSearch {
    pub observer: Option<Arc<dyn WebObserver>>,
    /// The request exists to search (ReMa's retrieval step): the provider
    /// is told to search where it supports that (`tool_choice: required`),
    /// and a provider that refuses web search fails the request instead of
    /// answering without it.
    pub required: bool,
    /// Search only these sites (and their subdomains), where the provider
    /// supports it (OpenAI `filters.allowed_domains`, Anthropic
    /// `allowed_domains`). Empty: no restriction.
    pub allowed_domains: Vec<String>,
    /// Where the request is about, for localized results (approximate;
    /// never the device's location).
    pub location: Option<ApproxLocation>,
    /// Tools offered instead when the provider refuses its own search for
    /// this request (an organization that turned web search off): ReMa's
    /// career search tools, so the answer still rests on current sources.
    /// They replace the request's tools and include them.
    pub fallback: Option<ToolBox>,
}

/// Most sites OpenAI's web search accepts in `filters.allowed_domains`.
pub const OPENAI_MAX_DOMAINS: usize = 100;

/// The queries of a search action: OpenAI and Codex list them in
/// `queries` (the single `query` is the older field).
pub fn joined_queries(action: &Value) -> Option<String> {
    let queries: Vec<&str> = action
        .get("queries")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .collect();
    (!queries.is_empty()).then(|| queries.join("; "))
}

/// An approximate place for search localization, from the request itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApproxLocation {
    pub city: Option<String>,
    pub region: Option<String>,
    /// ISO 3166-1 alpha-2 ("AT").
    pub country: Option<String>,
    /// IANA time zone ("Europe/Vienna").
    pub timezone: Option<String>,
}

impl ApproxLocation {
    /// The provider's `user_location` object (both use the same shape).
    pub fn to_json(&self) -> serde_json::Value {
        let mut out = serde_json::json!({ "type": "approximate" });
        for (key, value) in [
            ("city", &self.city),
            ("region", &self.region),
            ("country", &self.country),
            ("timezone", &self.timezone),
        ] {
            if let Some(value) = value {
                out[key] = serde_json::json!(value);
            }
        }
        out
    }
}

impl WebSearch {
    fn report(&self, event: WebEvent) {
        if let Some(observer) = &self.observer {
            observer.observe(event);
        }
    }
}

impl fmt::Debug for WebSearch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebSearch").finish_non_exhaustive()
    }
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

/// Why web search stopped in the middle of an answer.
pub const PRIVATE_WEB_OFF: &str = "this answer read your mail, calendar or applications, so web \
access is off for the rest of this chat. Start a new chat for answers from the web.";

/// Most model calls in one answer when tools are used. After this many
/// rounds the model answers once more with tools switched off.
pub const MAX_TOOL_ROUNDS: usize = 8;
/// Most times one answer resumes a provider's paused tool loop.
pub const MAX_CONTINUATIONS: usize = 4;
/// Ids ReMa gives tool calls a provider sent without one. They are unique
/// within an answer and never sent back as the provider's own ids.
pub const SYNTHETIC_CALL_PREFIX: &str = "rema_call_";

/// Whether a tool call id was made up by ReMa (the provider sent none).
pub fn is_synthetic_call_id(id: &str) -> bool {
    id.starts_with(SYNTHETIC_CALL_PREFIX)
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub system: Option<String>,
    pub turns: Vec<Turn>,
    /// Output cap reported by the provider for this model, if known.
    pub max_output_tokens: Option<u32>,
    /// Tools the model may call (none unless the user selected MCP servers).
    pub tools: Option<ToolBox>,
    /// The model may search the web (chat and scheduled prompts; never for
    /// ReMa's own extraction requests).
    pub web: Option<WebSearch>,
    /// Tool use so far in this answer, after `turns`.
    pub rounds: Vec<ToolRound>,
    /// The model must answer now, without calling tools (the last round of
    /// a long tool loop). The tools stay declared: earlier rounds used them.
    pub tools_off: bool,
    /// Set by the tools once one returned the user's private data (mail,
    /// calendar, applications): the provider's web search is switched off
    /// for the rest of the answer, so nothing read there can reach it.
    pub private: Option<Arc<AtomicBool>>,
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
    /// An OpenAI-compatible server that runs a web search tool itself
    /// (Unsloth Studio): web requests enable only that tool there.
    pub server_web_search: bool,
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
            // Connector grants never belong to a model provider.
            Some(Credential::RefreshToken { .. }) | None => request,
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
    /// Searches and page visits reported by the provider.
    pub web: Vec<WebEvent>,
    /// The provider paused its server-side tool loop; send the step back to
    /// continue.
    pub paused: bool,
}

/// State an adapter keeps across the events of one stream.
#[derive(Debug, Default)]
pub struct StreamState {
    /// The step as the provider must receive it back in a tool loop:
    /// Anthropic's content blocks (rebuilt from the stream), Gemini's parts,
    /// OpenAI's encrypted reasoning items.
    pub blocks: Vec<Value>,
    /// Partial JSON of blocks whose input streams in pieces, by index.
    pub partial_json: HashMap<usize, String>,
    /// Server-side web tool calls in progress (Unsloth): id → (kind, target).
    pub web_calls: HashMap<String, (WebKind, String)>,
    /// Inline reasoning removed from a local model's text.
    pub think: openai::ThinkFilter,
}

/// How one model call ended.
struct Step {
    finish: Finish,
    calls: Vec<ToolCall>,
    paused: bool,
    content: Option<Value>,
}

type ParseFn = fn(&SseEvent, &mut StreamState) -> AppResult<StreamPiece>;

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

    /// The calls of the answer's `round`-th model step. A call without a
    /// provider id gets one that no other call of the answer has.
    fn finish(self, round: usize) -> Vec<ToolCall> {
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
                        .unwrap_or_else(|| format!("{SYNTHETIC_CALL_PREFIX}{round}_{n}")),
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

    /// Whether an OpenAI-compatible server runs a web search tool itself
    /// (Unsloth Studio). Asked once per server and remembered.
    fn serves_web_search<'a>(&'a self, _endpoint: &'a Endpoint) -> BoxFuture<'a, bool> {
        Box::pin(async { false })
    }

    /// A runtime's own web search policy, read from its metadata without
    /// searching: Codex's mode for the signed-in account (`live`,
    /// `indexed`, `cached`) or why it may not search. `None` where the
    /// runtime has no such policy.
    fn web_search_policy<'a>(
        &'a self,
        _endpoint: &'a Endpoint,
    ) -> BoxFuture<'a, Option<Result<String, String>>> {
        Box::pin(async { None })
    }
}

/// The real implementation: HTTPS APIs, or the Codex runtime for a
/// ChatGPT account.
pub struct ProviderLanguageModel {
    http: reqwest::Client,
    codex: Option<Arc<CodexRuntime>>,
    /// Hands out the Claude Console access token (short-lived, cached by
    /// the runtime): taken again for every request, so an answer that waits
    /// long for an approval never sends an expired one.
    console: Option<Arc<dyn AccountRuntime>>,
}

impl ProviderLanguageModel {
    pub fn new(codex: Option<Arc<CodexRuntime>>) -> Self {
        Self {
            http: http::client(),
            codex,
            console: None,
        }
    }

    /// The Claude Console runtime that renews Console access tokens.
    pub fn with_console(mut self, console: Arc<dyn AccountRuntime>) -> Self {
        self.console = Some(console);
        self
    }

    /// The endpoint with a current Claude Console token (other connections
    /// as they are).
    async fn current(&self, endpoint: &Endpoint) -> AppResult<Option<Endpoint>> {
        if endpoint.connection != ConnectionMethod::ClaudeConsole {
            return Ok(None);
        }
        let Some(console) = &self.console else {
            return Ok(None);
        };
        Ok(console.credential().await?.map(|credential| Endpoint {
            credential: Some(credential),
            ..endpoint.clone()
        }))
    }

    fn codex(&self) -> AppResult<&CodexRuntime> {
        self.codex
            .as_deref()
            .ok_or_else(|| AppError::configuration("ChatGPT sign-in is not available"))
    }
}

impl LanguageModel for ProviderLanguageModel {
    fn serves_web_search<'a>(&'a self, endpoint: &'a Endpoint) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            endpoint.kind == ProviderKind::OpenaiCompatible
                && openai::serves_web_search(&self.http, endpoint).await
        })
    }

    fn web_search_policy<'a>(
        &'a self,
        endpoint: &'a Endpoint,
    ) -> BoxFuture<'a, Option<Result<String, String>>> {
        Box::pin(async move {
            if endpoint.connection != ConnectionMethod::ChatgptAccount {
                return None;
            }
            let codex = self.codex.as_deref()?;
            match codex.web_search_policy().await {
                Ok(policy) => Some(policy.map(str::to_string)),
                // Not running or not signed in: nothing learned yet.
                Err(_) => None,
            }
        })
    }

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
            // Call the model; run the tools it asks for (or resume a paused
            // server-side tool loop) and call it again, until it answers.
            let mut request = request.clone();
            // Gemini before 3 cannot use Google Search in a request that also
            // declares functions: such a request gets ReMa's search tools
            // instead (the answer still rests on current sources).
            if endpoint.kind == ProviderKind::Gemini
                && !gemini::combines_search_with_functions(model_id)
                && !request.tool_specs().is_empty()
            {
                if let Some(web) = request.web.take() {
                    if let Some(fallback) = web.fallback {
                        request.tools = Some(fallback);
                    }
                }
            }
            let mut separate = false;
            let mut continuations = 0;
            loop {
                let mut round_text = String::new();
                let result = {
                    let mut forward = |text: &str| {
                        // A blank line between the text of successive steps.
                        if separate && round_text.is_empty() && !text.trim().is_empty() {
                            on_delta("\n\n");
                        }
                        round_text.push_str(text);
                        on_delta(text);
                    };
                    self.stream_once(endpoint, model_id, &request, &cancel, &mut forward)
                        .await
                };
                let step = match result {
                    // Anthropic asks this model to call web search directly
                    // (no programmatic tool calling): once, then remembered.
                    Err(error)
                        if endpoint.kind == ProviderKind::Anthropic
                            && request.web.is_some()
                            && request.rounds.is_empty()
                            && round_text.is_empty()
                            && anthropic::asks_for_direct_callers(&error)
                            && anthropic::learn_direct(model_id) =>
                    {
                        continue;
                    }
                    // The provider refused its web tools for this model or
                    // account: answer with ReMa's career search tools when
                    // the request has them, else without web search, rather
                    // than not at all.
                    Err(error)
                        if request.web.as_ref().is_some_and(|w| !w.required)
                            && request.rounds.is_empty()
                            && round_text.is_empty()
                            && rejects_web_tools(&error) =>
                    {
                        if let Some(web) = request.web.take() {
                            web.report(WebEvent::Unavailable {
                                reason: error.to_string(),
                            });
                            if let Some(fallback) = web.fallback {
                                request.tools = Some(fallback);
                            }
                        }
                        continue;
                    }
                    other => other?,
                };
                separate |= !round_text.trim().is_empty();
                if step.paused && step.finish == Finish::Complete {
                    if continuations == MAX_CONTINUATIONS {
                        return Err(AppError::provider(
                            "The model kept searching without answering. Try a narrower request.",
                        ));
                    }
                    continuations += 1;
                    request.rounds.push(ToolRound {
                        text: round_text,
                        content: step.content,
                        paused: true,
                        ..ToolRound::default()
                    });
                    continue;
                }
                let Some(tools) = request.tools.as_ref() else {
                    return Ok(step.finish);
                };
                // The final round answers; a call it makes anyway is not run.
                if step.calls.is_empty() || step.finish != Finish::Complete || request.tools_off {
                    return Ok(step.finish);
                }
                let mut outputs = Vec::with_capacity(step.calls.len());
                for call in &step.calls {
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
                    calls: step.calls,
                    outputs,
                    content: step.content,
                    paused: false,
                });
                // Private data was just read: no web search from here on
                // (URLs and text in an email are untrusted).
                if request
                    .private
                    .as_ref()
                    .is_some_and(|p| p.load(Ordering::SeqCst))
                {
                    if let Some(web) = request.web.take() {
                        web.report(WebEvent::Unavailable {
                            reason: PRIVATE_WEB_OFF.into(),
                        });
                    }
                }
                // Enough tool use: the next call answers from what the tools
                // returned, rather than the answer being lost.
                if request.rounds.iter().filter(|r| !r.paused).count() >= MAX_TOOL_ROUNDS {
                    request.tools_off = true;
                }
            }
        })
    }
}

impl ProviderLanguageModel {
    /// One HTTPS call to the model: streams its text and reports how it
    /// finished and the tools it called.
    async fn stream_once(
        &self,
        endpoint: &Endpoint,
        model_id: &str,
        request: &ChatRequest,
        cancel: &CancellationToken,
        on_delta: DeltaSink<'_>,
    ) -> AppResult<Step> {
        let renewed = self.current(endpoint).await?;
        let endpoint = renewed.as_ref().unwrap_or(endpoint);
        let secrets = endpoint.secrets();
        let (http_request, parse): (RequestBuilder, ParseFn) = match endpoint.kind {
            ProviderKind::Openai => (
                openai_responses::chat_request(&self.http, endpoint, model_id, request),
                openai_responses::parse_event,
            ),
            ProviderKind::OpenaiCompatible => (
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
            http::send_retrying(http_request, cancel, &endpoint.name, &secrets).await?
        else {
            return Ok(Step {
                finish: Finish::Cancelled,
                calls: Vec::new(),
                paused: false,
                content: None,
            });
        };
        let web = request.web.as_ref();
        // Only adapters that must send a step back verbatim keep its content
        // (Anthropic's blocks, Gemini's parts, OpenAI's reasoning items).
        let round = request.rounds.len();
        drive_stream(response, cancel, on_delta, parse, web, round).await
    }
}

/// A 400 that names the provider's web tools: the model or account cannot
/// use them.
fn rejects_web_tools(error: &AppError) -> bool {
    let AppError::Provider(message) = error else {
        return false;
    };
    let message = message.to_lowercase();
    message.contains("(400)")
        && [
            "web_search",
            "web search",
            "web_fetch",
            "web fetch",
            "google_search",
            "googlesearch",
            "grounding",
            "server tool",
            "tool use with",
            "tools.",
            "unsupported tool",
            "tool type",
            // Gemini: "Please enable tool_config.include_server_side_tool_
            // invocations to use Built-in tools with Function calling".
            "include_server_side_tool_invocations",
            "built-in tools",
        ]
        .iter()
        .any(|needle| message.contains(needle))
}

/// Reads a provider stream, forwarding text and web activity and collecting
/// tool calls.
async fn drive_stream(
    response: reqwest::Response,
    cancel: &CancellationToken,
    on_delta: DeltaSink<'_>,
    parse: ParseFn,
    web: Option<&WebSearch>,
    round: usize,
) -> AppResult<Step> {
    let mut finish = Finish::Complete;
    let mut paused = false;
    let mut calls = CallCollector::default();
    let mut state = StreamState::default();
    let end = read_sse(response, cancel, |event| {
        let piece = parse(&event, &mut state)?;
        if let Some(text) = piece.text.as_deref().filter(|t| !t.is_empty()) {
            on_delta(text);
        }
        if let Some(reason) = piece.finish {
            finish = reason;
        }
        paused |= piece.paused;
        for delta in piece.tools {
            calls.push(delta);
        }
        if let Some(web) = web {
            for event in piece.web {
                web.report(event);
            }
        }
        Ok(if piece.done {
            Flow::Stop
        } else {
            Flow::Continue
        })
    })
    .await?;
    Ok(match end {
        StreamEnd::Cancelled => Step {
            finish: Finish::Cancelled,
            calls: Vec::new(),
            paused: false,
            content: None,
        },
        StreamEnd::Completed => Step {
            finish,
            calls: calls.finish(round),
            paused,
            content: (!state.blocks.is_empty()).then_some(Value::Array(state.blocks)),
        },
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

    #[test]
    fn recognizes_providers_refusing_their_web_tools() {
        // Gemini 3: built-in search next to functions without the flag.
        assert!(rejects_web_tools(&AppError::provider(
            "Gemini returned an error (400): Please enable \
             tool_config.include_server_side_tool_invocations to use Built-in tools with \
             Function calling"
        )));
        assert!(rejects_web_tools(&AppError::provider(
            "Anthropic returned an error (400): web search is not enabled for this organization"
        )));
        assert!(!rejects_web_tools(&AppError::provider(
            "OpenAI returned an error (400): max_output_tokens too large"
        )));
    }
}

#[cfg(test)]
mod tests_http;

#[cfg(test)]
pub mod fake;
