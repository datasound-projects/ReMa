//! Anthropic Messages API.
//!
//! With web search on, the request declares Anthropic's server tools
//! (`web_search`, and `web_fetch` on models that have the current
//! versions). They run on Anthropic's side; the stream reports what they
//! did, and the response's content blocks are kept so a `pause_turn` can
//! be resumed exactly as the API expects.

use std::collections::HashSet;

use reqwest::RequestBuilder;
use serde_json::{json, Value};

use super::{
    http::{error_message, join_url, send_json},
    sse::SseEvent,
    ChatRequest, Endpoint, FetchedModel, Finish, StreamPiece, StreamState, ToolDelta, ToolRound,
    WebEvent, WebKind, WebSearch, WebSource,
};
use crate::{
    error::{AppError, AppResult},
    models::chat::MessageRole,
    secrets::Credential,
};

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
const API_VERSION: &str = "2023-06-01";
/// Required with OAuth access tokens (a Claude Console sign-in).
const OAUTH_BETA: &str = "oauth-2025-04-20";
/// Lets a request ask the API to drop thinking written with an earlier
/// tool list instead of rejecting it (Claude 5 models bind each thinking
/// block to the system prompt, tools and messages before it).
const BINDING_BETA: &str = "thinking-binding-controls-2026-08-01";

/// Output cap for streaming requests when the model allows at least this much.
const STREAMING_MAX_TOKENS: u32 = 64_000;
/// Safe for every current model when the provider did not report a limit.
const FALLBACK_MAX_TOKENS: u32 = 8_192;
/// Added to a request's output cap for models that think by default:
/// `max_tokens` caps thinking and answer together, so a cap sized for the
/// answer alone (a JSON reply of 2,000 tokens) would cut the answer off.
const THINKING_HEADROOM: u32 = 16_000;
const RECOMMENDED_COUNT: usize = 4;
/// Most searches and page fetches in one answer.
const MAX_WEB_USES: u32 = 8;
/// Most of one fetched page given to the model (the API sets no limit).
const MAX_FETCH_TOKENS: u32 = 20_000;

fn authorize(endpoint: &Endpoint, request: RequestBuilder, betas: &[&str]) -> RequestBuilder {
    let request = request.header("anthropic-version", API_VERSION);
    let mut beta = Vec::new();
    let request = match &endpoint.credential {
        Some(Credential::ApiKey { key }) => request.header("x-api-key", key),
        Some(Credential::OAuth { access_token, .. }) => {
            beta.push(OAUTH_BETA);
            request.bearer_auth(access_token)
        }
        // Connector grants never belong to a model provider.
        Some(Credential::RefreshToken { .. }) | None => request,
    };
    beta.extend_from_slice(betas);
    if beta.is_empty() {
        request
    } else {
        request.header("anthropic-beta", beta.join(","))
    }
}

pub async fn list_models(
    http: &reqwest::Client,
    endpoint: &Endpoint,
) -> AppResult<Vec<FetchedModel>> {
    let secrets = endpoint.secrets();
    let mut entries = Vec::new();
    let mut after: Option<String> = None;
    // The API pages newest first; a few pages cover every model.
    for _ in 0..10 {
        let mut request = http
            .get(join_url(&endpoint.base_url, "models"))
            .query(&[("limit", "1000")]);
        if let Some(after_id) = &after {
            request = request.query(&[("after_id", after_id.as_str())]);
        }
        let body = send_json(authorize(endpoint, request, &[]), &endpoint.name, &secrets).await?;
        entries.extend(
            body.get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        match (
            body.get("has_more").and_then(Value::as_bool),
            body.get("last_id").and_then(Value::as_str),
        ) {
            (Some(true), Some(last)) => after = Some(last.to_string()),
            _ => break,
        }
    }
    Ok(parse_models(&entries))
}

pub fn parse_models(entries: &[Value]) -> Vec<FetchedModel> {
    entries
        .iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            Some(FetchedModel {
                display_name: m
                    .get("display_name")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string(),
                max_output_tokens: m
                    .get("max_tokens")
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok()),
                recommended: false,
                id,
            })
        })
        .enumerate()
        .map(|(index, model)| FetchedModel {
            recommended: index < RECOMMENDED_COUNT,
            ..model
        })
        .collect()
}

/// Anthropic's web tool versions. One place to update when Anthropic adds
/// a version (checked against platform.claude.com: web search tool, web
/// fetch tool, programmatic tool calling; 2026-09-27).
pub mod web_tool_versions {
    /// Newest search version: dynamic filtering and response inclusion.
    pub const SEARCH_CURRENT: &str = "web_search_20260318";
    /// Newest fetch version (same additions).
    pub const FETCH_CURRENT: &str = "web_fetch_20260318";
    /// Basic search, for models without dynamic filtering.
    pub const SEARCH_BASIC: &str = "web_search_20250305";
}

/// Which web tools a model gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebToolChoice {
    pub search: &'static str,
    pub fetch: Option<&'static str>,
    /// `allowed_callers: ["direct"]`: the model calls search itself rather
    /// than from code execution. The `_20260209` and later versions default
    /// to a code execution caller (dynamic filtering), which provisions a
    /// container and is not eligible for zero data retention; ReMa asks
    /// for direct calls on every version that takes the field (server
    /// tools documentation, "ZDR and allowed_callers").
    pub direct: bool,
}

/// Family, major and minor version of a Claude model id
/// (`claude-sonnet-4-6`, `claude-opus-4-5-20251101` → minor 5, a dated
/// snapshot is not a minor version).
fn model_version(model_id: &str) -> Option<(String, u32, u32)> {
    let id = model_id.to_ascii_lowercase();
    let rest = id.strip_prefix("claude-")?;
    let mut parts = rest.split(['-', '.', '@']);
    let family = parts.next()?.to_string();
    let major: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor: u32 = parts
        .next()
        .filter(|p| p.len() <= 2)
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    Some((family, major, minor))
}

/// A 400 saying a thinking block is bound to a different conversation
/// (preserved thinking: the system prompt, tools or earlier messages
/// changed since it was written). The documented recovery is to ask the
/// API to drop such blocks once, or else to leave every thinking block out.
pub fn is_prefix_mismatch(error: &AppError) -> bool {
    matches!(error, AppError::Provider(message)
        if message.contains("(400)")
            && (message.contains("Invalid `signature`")
                || message.contains("bound to a different conversation")
                || message.contains("prefix_mismatch_behavior")))
}

/// Leaves every thinking block out of the steps sent back (the documented
/// fallback when the API cannot be asked to drop the invalid ones).
pub fn strip_thinking(rounds: &mut [ToolRound]) {
    for round in rounds {
        if let Some(Value::Array(blocks)) = round.content.as_mut() {
            blocks.retain(|b| {
                !matches!(
                    b.get("type").and_then(Value::as_str),
                    Some("thinking" | "redacted_thinking")
                )
            });
        }
    }
}

/// Whether the model thinks when a request does not mention thinking: the
/// Claude 5 generation (Opus 5 and 5.5, Sonnet 5, Fable, Mythos). ReMa
/// leaves thinking at the model's default, so their output caps need room
/// for it.
pub fn thinks_by_default(model_id: &str) -> bool {
    match model_version(model_id) {
        Some((family, major, _)) => match family.as_str() {
            "opus" | "sonnet" | "haiku" => major >= 5,
            "fable" | "mythos" => true,
            _ => false,
        },
        None => false,
    }
}

/// The request's `max_tokens`: the caller's cap (bounded for streaming), plus
/// room for thinking on models that think by default.
fn max_tokens(model_id: &str, requested: Option<u32>) -> u32 {
    let cap = requested.map_or(FALLBACK_MAX_TOKENS, |limit| limit.min(STREAMING_MAX_TOKENS));
    if thinks_by_default(model_id) {
        cap.saturating_add(THINKING_HEADROOM)
            .min(STREAMING_MAX_TOKENS)
            .max(cap)
    } else {
        cap
    }
}

/// The web tools for a model. The `_20260318` versions (response
/// inclusion control) are available on Claude 4.6 and later and the Mythos
/// models; every other model gets basic search, whose only caller is
/// direct anyway.
pub fn web_tool_choice(model_id: &str) -> WebToolChoice {
    use web_tool_versions::*;
    let basic = WebToolChoice {
        search: SEARCH_BASIC,
        fetch: None,
        direct: false,
    };
    let Some((family, major, minor)) = model_version(model_id) else {
        return basic;
    };
    let dynamic = match family.as_str() {
        "opus" | "sonnet" => major >= 5 || (major == 4 && minor >= 6),
        "fable" | "haiku" => major >= 5,
        "mythos" => true,
        _ => false,
    };
    if !dynamic {
        return basic;
    }
    WebToolChoice {
        search: SEARCH_CURRENT,
        fetch: Some(FETCH_CURRENT),
        direct: true,
    }
}

fn web_tools(model_id: &str, web: &WebSearch) -> Vec<Value> {
    let choice = web_tool_choice(model_id);
    let mut search = json!({
        "type": choice.search,
        "name": "web_search",
        "max_uses": MAX_WEB_USES,
    });
    // Career searches stay on career sources; the place is approximate.
    if !web.allowed_domains.is_empty() {
        search["allowed_domains"] = json!(web.allowed_domains);
    }
    if let Some(location) = &web.location {
        search["user_location"] = location.to_json();
    }
    let mut tools = vec![search];
    if let Some(fetch) = choice.fetch {
        tools.push(json!({
            "type": fetch,
            "name": "web_fetch",
            "max_uses": MAX_WEB_USES,
            // A long page or PDF must not fill the context by itself.
            "max_content_tokens": MAX_FETCH_TOKENS,
        }));
    }
    for tool in &mut tools {
        if choice.direct {
            tool["allowed_callers"] = json!(["direct"]);
        }
        // ReMa verifies a search by its result blocks: keep them all.
        if tool["type"]
            .as_str()
            .is_some_and(|t| t.ends_with("20260318"))
        {
            tool["response_inclusion"] = json!("full");
        }
    }
    tools
}

pub fn request_body(model_id: &str, request: &ChatRequest) -> Value {
    let max_tokens = max_tokens(model_id, request.max_output_tokens);
    let mut messages: Vec<Value> = request
        .normalized_turns()
        .into_iter()
        .map(|turn| {
            let role = match turn.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
            };
            json!({ "role": role, "content": turn.content })
        })
        .collect();
    // Steps so far in this answer: the assistant's content (as Anthropic
    // sent it when known), then the results of ReMa-run tools. A paused
    // step has no results; the next step continues the same message.
    let resolved = resolved_server_calls(&request.rounds);
    for round in &request.rounds {
        let content = match &round.content {
            Some(Value::Array(blocks)) => blocks
                .iter()
                .filter(|b| is_sendable(b))
                // Web tools switched off: a call they would still run is
                // left out (the API rejects a pending call whose tool is no
                // longer declared).
                .filter(|b| request.web.is_some() || !is_unresolved(b, &resolved))
                .cloned()
                .collect(),
            _ => {
                let mut content = Vec::new();
                if !round.text.trim().is_empty() {
                    content.push(json!({ "type": "text", "text": round.text }));
                }
                for call in &round.calls {
                    let input = match &call.arguments {
                        Value::Object(_) => call.arguments.clone(),
                        _ => json!({}),
                    };
                    content.push(json!({
                        "type": "tool_use", "id": call.id, "name": call.name, "input": input,
                    }));
                }
                content
            }
        };
        match messages.last_mut() {
            Some(last) if last["role"] == "assistant" && last["content"].is_array() => {
                if let Some(existing) = last["content"].as_array_mut() {
                    existing.extend(content);
                }
            }
            _ => messages.push(json!({ "role": "assistant", "content": content })),
        }
        if round.paused || round.calls.is_empty() {
            continue;
        }
        let results: Vec<Value> = round
            .calls
            .iter()
            .zip(&round.outputs)
            .map(|(call, output)| {
                json!({
                    "type": "tool_result",
                    "tool_use_id": call.id,
                    "content": output.content,
                    "is_error": output.is_error,
                })
            })
            .collect();
        messages.push(json!({ "role": "user", "content": results }));
    }
    let mut body = json!({
        "model": model_id,
        "max_tokens": max_tokens,
        "stream": true,
        "messages": messages,
    });
    if let Some(system) = &request.system {
        body["system"] = json!(system);
    }
    let mut tools: Vec<Value> = request
        .tool_specs()
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema,
            })
        })
        .collect();
    if let Some(web) = &request.web {
        tools.extend(web_tools(model_id, web));
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
        // The last round of a long tool loop answers without tools (the
        // tools stay declared: earlier rounds used them).
        if request.tools_off {
            body["tool_choice"] = json!({ "type": "none" });
        }
    }
    // The tool list changed mid-answer: the thinking of earlier steps was
    // written with the old one. The API drops it (the steps' calls and
    // results stay) rather than refusing the request.
    if drops_stale_thinking(model_id, request) {
        body["thinking"] = json!({
            "type": "adaptive",
            "block_binding": { "prefix_mismatch_behavior": "drop_block" },
        });
    }
    body
}

/// Web tools were switched off mid-answer on a model that thinks by
/// default, so earlier steps carry thinking bound to the old tool list; or
/// the API refused a request over such a block and the answer is retried
/// once with the documented setting.
fn drops_stale_thinking(model_id: &str, request: &ChatRequest) -> bool {
    request.drop_stale_thinking || (request.web_dropped && thinks_by_default(model_id))
}

/// Ids of the server tool calls whose result is in this answer (a deferred
/// call's result opens the step after it).
fn resolved_server_calls(rounds: &[ToolRound]) -> HashSet<&str> {
    rounds
        .iter()
        .filter_map(|r| r.content.as_ref()?.as_array())
        .flatten()
        .filter(|b| {
            b.get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.ends_with("_tool_result"))
        })
        .filter_map(|b| b.get("tool_use_id").and_then(Value::as_str))
        .collect()
}

/// A server tool call without its result: Claude made it next to a ReMa
/// tool, and the API runs it with the next request, while the tool is
/// still declared.
fn is_unresolved(block: &Value, resolved: &HashSet<&str>) -> bool {
    block.get("type").and_then(Value::as_str) == Some("server_tool_use")
        && block
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| !resolved.contains(id))
}

/// Searches and page visits Claude asked for in this answer that have not
/// run (id, kind, query or URL). Other providers' steps have none.
pub fn unresolved_web_calls(rounds: &[ToolRound]) -> Vec<(String, WebKind, String)> {
    let resolved = resolved_server_calls(rounds);
    rounds
        .iter()
        .filter_map(|r| r.content.as_ref()?.as_array())
        .flatten()
        .filter(|b| is_unresolved(b, &resolved))
        .filter(|b| web_kind(b.get("name").and_then(Value::as_str).unwrap_or("")).is_some())
        .map(|b| {
            let (kind, target) = web_call(b);
            let id = b.get("id").and_then(Value::as_str).unwrap_or_default();
            (id.to_string(), kind, target)
        })
        .collect()
}

pub fn chat_request(
    http: &reqwest::Client,
    endpoint: &Endpoint,
    model_id: &str,
    request: &ChatRequest,
) -> RequestBuilder {
    let betas: &[&str] = if drops_stale_thinking(model_id, request) {
        &[BINDING_BETA]
    } else {
        &[]
    };
    authorize(
        endpoint,
        http.post(join_url(&endpoint.base_url, "messages"))
            .json(&request_body(model_id, request)),
        betas,
    )
}

/// Blocks the API accepts back: not placeholders or empty text.
fn is_sendable(block: &Value) -> bool {
    match block.get("type").and_then(Value::as_str) {
        None => false,
        Some("text") => block
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|t| !t.is_empty()),
        Some(_) => true,
    }
}

/// Pages a web tool result lists, or its error.
fn web_result(block: &Value) -> (Vec<WebSource>, Option<String>) {
    let content = block.get("content").unwrap_or(&Value::Null);
    let source = |item: &Value| {
        let url = item.get("url").and_then(Value::as_str)?.to_string();
        let title = item
            .get("title")
            .or_else(|| item.pointer("/content/title"))
            .and_then(Value::as_str)
            .unwrap_or(&url)
            .to_string();
        Some(WebSource { title, url })
    };
    match content {
        Value::Array(items) => (items.iter().filter_map(source).collect(), None),
        Value::Object(_) => match content.get("error_code").and_then(Value::as_str) {
            Some(code) => (Vec::new(), Some(web_error(code))),
            None => (source(content).into_iter().collect(), None),
        },
        _ => (Vec::new(), None),
    }
}

fn web_error(code: &str) -> String {
    match code {
        "max_uses_exceeded" => "Search limit for this answer reached.".into(),
        "too_many_requests" => "Anthropic is rate limiting searches right now.".into(),
        "query_too_long" => "The search query was too long.".into(),
        "url_not_accessible" | "url_not_allowed" => "The page could not be opened.".into(),
        "unsupported_content_type" => "The page's content type is not supported.".into(),
        "unavailable" => "Search is unavailable right now.".into(),
        other => format!("Search failed ({other})."),
    }
}

fn web_kind(name: &str) -> Option<WebKind> {
    match name {
        "web_search" => Some(WebKind::Search),
        "web_fetch" => Some(WebKind::Page),
        _ => None,
    }
}

pub fn parse_event(event: &SseEvent, state: &mut StreamState) -> AppResult<StreamPiece> {
    let data: Value = serde_json::from_str(event.data.trim())
        .map_err(|_| AppError::provider("Anthropic sent an unreadable stream event"))?;
    let kind = data
        .get("type")
        .and_then(Value::as_str)
        .or(event.event.as_deref())
        .unwrap_or_default();
    let index = data
        .get("index")
        .and_then(Value::as_u64)
        .map(|i| i as usize);
    Ok(match kind {
        "content_block_start" => {
            let block = data.get("content_block").cloned().unwrap_or(Value::Null);
            let block_type = block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let mut piece = StreamPiece::default();
            match block_type {
                "tool_use" => {
                    piece.tools.push(ToolDelta {
                        index,
                        id: block.get("id").and_then(Value::as_str).map(str::to_string),
                        name: block
                            .get("name")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        ..ToolDelta::default()
                    });
                }
                "web_search_tool_result" | "web_fetch_tool_result" => {
                    let id = block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    // The call this answers: its kind and query or URL.
                    let call = state
                        .blocks
                        .iter()
                        .find(|b| b.get("id").and_then(Value::as_str) == Some(id.as_str()));
                    let (kind, target) = call.map_or(
                        (
                            if block_type == "web_fetch_tool_result" {
                                WebKind::Page
                            } else {
                                WebKind::Search
                            },
                            String::new(),
                        ),
                        web_call,
                    );
                    let (sources, error) = web_result(&block);
                    piece.web.push(WebEvent::Finished {
                        id,
                        kind,
                        target,
                        sources,
                        error,
                    });
                }
                _ => {}
            }
            if let Some(i) = index {
                if state.blocks.len() <= i {
                    state.blocks.resize(i + 1, Value::Null);
                }
                state.blocks[i] = block;
            }
            piece
        }
        "content_block_delta" => {
            let block = index.and_then(|i| state.blocks.get_mut(i));
            match data.pointer("/delta/type").and_then(Value::as_str) {
                Some("text_delta") => {
                    let text = data
                        .pointer("/delta/text")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if let Some(block) = block {
                        append(block, "text", text);
                    }
                    StreamPiece::text(text)
                }
                Some("input_json_delta") => {
                    let part = data
                        .pointer("/delta/partial_json")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if let Some(i) = index {
                        state.partial_json.entry(i).or_default().push_str(&part);
                    }
                    let is_tool_use = block
                        .map(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                        .unwrap_or(true);
                    if is_tool_use {
                        StreamPiece::tool(ToolDelta {
                            index,
                            arguments: part,
                            ..ToolDelta::default()
                        })
                    } else {
                        StreamPiece::default()
                    }
                }
                Some("citations_delta") => {
                    let citation = data.pointer("/delta/citation");
                    if let (Some(block), Some(citation)) = (block, citation) {
                        if !block["citations"].is_array() {
                            block["citations"] = json!([]);
                        }
                        if let Some(list) = block["citations"].as_array_mut() {
                            list.push(citation.clone());
                        }
                    }
                    // Web citations name the page they quote.
                    let url = citation.and_then(|c| c.get("url")).and_then(Value::as_str);
                    StreamPiece {
                        web: url
                            .map(|url| WebEvent::Cited {
                                url: url.to_string(),
                                title: citation
                                    .and_then(|c| c.get("title"))
                                    .and_then(Value::as_str)
                                    .unwrap_or(url)
                                    .to_string(),
                                quote: citation
                                    .and_then(|c| c.get("cited_text"))
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                range: None,
                            })
                            .into_iter()
                            .collect(),
                        ..StreamPiece::default()
                    }
                }
                // Thinking is kept for the record but not shown.
                Some("thinking_delta") => {
                    if let Some(block) = block {
                        let text = data.pointer("/delta/thinking").and_then(Value::as_str);
                        append(block, "thinking", text.unwrap_or_default());
                    }
                    StreamPiece::default()
                }
                Some("signature_delta") => {
                    if let Some(block) = block {
                        let signature = data.pointer("/delta/signature").and_then(Value::as_str);
                        append(block, "signature", signature.unwrap_or_default());
                    }
                    StreamPiece::default()
                }
                _ => StreamPiece::default(),
            }
        }
        "content_block_stop" => {
            let mut piece = StreamPiece::default();
            if let Some(i) = index {
                if let Some(json) = state.partial_json.remove(&i) {
                    if let Some(block) = state.blocks.get_mut(i) {
                        let input = serde_json::from_str::<Value>(json.trim())
                            .ok()
                            .filter(Value::is_object)
                            .unwrap_or_else(|| json!({}));
                        block["input"] = input;
                    }
                }
                if let Some(block) = state.blocks.get(i) {
                    if block.get("type").and_then(Value::as_str) == Some("server_tool_use") {
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                        if web_kind(name).is_some() {
                            let (kind, target) = web_call(block);
                            piece.web.push(WebEvent::Started {
                                id: block
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                kind,
                                target,
                            });
                        }
                    }
                }
            }
            piece
        }
        "message_delta" => match data.pointer("/delta/stop_reason").and_then(Value::as_str) {
            Some("pause_turn") => StreamPiece {
                finish: Some(Finish::Complete),
                paused: true,
                ..StreamPiece::default()
            },
            Some("max_tokens") => StreamPiece {
                finish: Some(Finish::MaxTokens),
                ..StreamPiece::default()
            },
            Some("refusal") => StreamPiece {
                finish: Some(Finish::Refused),
                ..StreamPiece::default()
            },
            Some(_) => StreamPiece {
                finish: Some(Finish::Complete),
                ..StreamPiece::default()
            },
            None => StreamPiece::default(),
        },
        "message_stop" => StreamPiece::done(),
        "error" => {
            let message = error_message(&data).unwrap_or_else(|| "unknown error".into());
            return Err(AppError::provider(format!("Anthropic: {message}")));
        }
        _ => StreamPiece::default(), // message_start, ping
    })
}

/// The kind and query or URL of a `server_tool_use` block.
fn web_call(block: &Value) -> (WebKind, String) {
    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
    let kind = web_kind(name).unwrap_or(WebKind::Search);
    let key = if kind == WebKind::Page {
        "url"
    } else {
        "query"
    };
    let target = block
        .pointer(&format!("/input/{key}"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    (kind, target)
}

fn append(block: &mut Value, key: &str, text: &str) {
    let current = block.get(key).and_then(Value::as_str).unwrap_or_default();
    block[key] = json!(format!("{current}{text}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Turn;

    fn event(name: &str, data: &str) -> SseEvent {
        SseEvent {
            event: Some(name.into()),
            data: data.into(),
        }
    }

    fn parse_event_(event: &SseEvent) -> AppResult<StreamPiece> {
        parse_event(event, &mut StreamState::default())
    }

    #[test]
    fn builds_messages_requests_with_output_cap() {
        let request = ChatRequest {
            system: Some("Be brief.".into()),
            turns: vec![Turn {
                role: MessageRole::User,
                content: "Hi".into(),
            }],
            max_output_tokens: Some(128_000),
            ..ChatRequest::default()
        };
        assert_eq!(
            request_body("claude-opus-5", &request),
            json!({
                "model": "claude-opus-5",
                "max_tokens": 64_000,
                "stream": true,
                "system": "Be brief.",
                "messages": [{ "role": "user", "content": "Hi" }],
            })
        );

        let unknown_limit = ChatRequest {
            max_output_tokens: None,
            ..request
        };
        assert_eq!(request_body("m", &unknown_limit)["max_tokens"], 8_192);
    }

    #[test]
    fn picks_the_web_tool_versions_each_model_has() {
        // Dynamic filtering: Claude 4.6 and later, the 5-series, Mythos.
        for id in [
            "claude-opus-5-5",
            "claude-opus-5",
            "claude-fable-5-1",
            "claude-sonnet-5",
            "claude-opus-4-8",
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-mythos-5-1",
        ] {
            let choice = web_tool_choice(id);
            assert_eq!(choice.search, "web_search_20260318", "{id}");
            assert_eq!(choice.fetch, Some("web_fetch_20260318"), "{id}");
            assert!(choice.direct, "{id}: searches are called directly");
        }
        for id in [
            "claude-haiku-4-5-20251001",
            "claude-sonnet-4-5",
            "claude-opus-4-5-20251101",
            "claude-opus-4-1",
            "claude-sonnet-4-20250514",
            "claude-3-7-sonnet-latest",
            "llama3",
        ] {
            let choice = web_tool_choice(id);
            assert_eq!(choice.search, "web_search_20250305", "{id}");
            assert_eq!(choice.fetch, None, "{id}");
        }
        // Every version that takes `allowed_callers` is asked for direct
        // calls: no code execution container for dynamic filtering.
        assert!(web_tool_choice("claude-haiku-5").direct);
        let request = ChatRequest {
            web: Some(crate::llm::WebSearch::default()),
            ..ChatRequest::default()
        };
        assert_eq!(
            request_body("claude-haiku-4-5", &request)["tools"],
            json!([{ "type": "web_search_20250305", "name": "web_search", "max_uses": 8 }])
        );
        assert_eq!(
            request_body("claude-haiku-5", &request)["tools"][0],
            json!({
                "type": "web_search_20260318", "name": "web_search", "max_uses": 8,
                "allowed_callers": ["direct"], "response_inclusion": "full"
            })
        );
        // The documented refusal of a thinking block written for another
        // prefix, and the fallback that leaves thinking out.
        let refused = AppError::provider(
            "Anthropic: messages.1.content.0: Invalid `signature` in `thinking` block. The \
             block is bound to a different conversation. (400)",
        );
        assert!(is_prefix_mismatch(&refused));
        assert!(!is_prefix_mismatch(&AppError::provider(
            "Anthropic: web search is not enabled (400)"
        )));
        let mut rounds = vec![ToolRound {
            content: Some(json!([
                { "type": "thinking", "thinking": "…", "signature": "sig" },
                { "type": "redacted_thinking", "data": "…" },
                { "type": "text", "text": "Checking." },
            ])),
            ..ToolRound::default()
        }];
        strip_thinking(&mut rounds);
        assert_eq!(
            rounds[0].content,
            Some(json!([{ "type": "text", "text": "Checking." }]))
        );

        // Career searches: the registry's sites and the request's place.
        let request = ChatRequest {
            web: Some(crate::llm::WebSearch {
                required: true,
                allowed_domains: vec!["karriere.at".into(), "linkedin.com".into()],
                location: Some(crate::llm::ApproxLocation {
                    city: Some("Vienna".into()),
                    region: None,
                    country: Some("AT".into()),
                    timezone: Some("Europe/Vienna".into()),
                }),
                ..crate::llm::WebSearch::default()
            }),
            ..ChatRequest::default()
        };
        let tools = &request_body("claude-sonnet-5", &request)["tools"];
        assert_eq!(
            tools[0],
            json!({
                "type": "web_search_20260318", "name": "web_search", "max_uses": 8,
                "allowed_domains": ["karriere.at", "linkedin.com"],
                "user_location": { "type": "approximate", "city": "Vienna", "country": "AT", "timezone": "Europe/Vienna" },
                "allowed_callers": ["direct"], "response_inclusion": "full"
            })
        );
        assert_eq!(
            tools[1],
            json!({
                "type": "web_fetch_20260318", "name": "web_fetch", "max_uses": 8,
                "max_content_tokens": 20_000, "allowed_callers": ["direct"],
                "response_inclusion": "full"
            })
        );
        // Without a place in the request, no location is sent.
        let request = ChatRequest {
            web: Some(crate::llm::WebSearch::default()),
            ..ChatRequest::default()
        };
        assert!(request_body("claude-sonnet-5", &request)["tools"][0]
            .get("user_location")
            .is_none());
    }

    #[test]
    fn leaves_room_for_thinking_on_models_that_think_by_default() {
        let capped = |model: &str, cap: u32| {
            request_body(
                model,
                &ChatRequest {
                    max_output_tokens: Some(cap),
                    ..ChatRequest::default()
                },
            )["max_tokens"]
                .as_u64()
                .unwrap()
        };
        // A 2,000-token JSON reply still fits after thinking.
        for model in [
            "claude-opus-5-5",
            "claude-opus-5",
            "claude-sonnet-5",
            "claude-fable-5-1",
        ] {
            assert!(thinks_by_default(model), "{model}");
            assert_eq!(capped(model, 2_000), 18_000, "{model}");
            assert_eq!(capped(model, 128_000), 64_000, "{model}");
        }
        // Models that think only when asked keep the caller's cap.
        for model in [
            "claude-opus-4-8",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
            "llama3",
        ] {
            assert!(!thinks_by_default(model), "{model}");
            assert_eq!(capped(model, 2_000), 2_000, "{model}");
        }
    }

    #[test]
    fn a_fetched_page_is_bounded_and_the_last_round_uses_no_tools() {
        let request = ChatRequest {
            web: Some(crate::llm::WebSearch::default()),
            tools_off: true,
            ..ChatRequest::default()
        };
        let body = request_body("claude-sonnet-5", &request);
        assert_eq!(body["tools"][1]["max_content_tokens"], 20_000);
        assert_eq!(body["tool_choice"], json!({ "type": "none" }));
        let normal = request_body(
            "claude-sonnet-5",
            &ChatRequest {
                tools_off: false,
                ..request
            },
        );
        assert!(normal.get("tool_choice").is_none());
    }

    #[test]
    fn parses_models_newest_first() {
        let entries = vec![
            json!({ "id": "claude-opus-5", "display_name": "Claude Opus 5", "max_tokens": 128000 }),
            json!({ "id": "claude-haiku-4-5", "display_name": "Claude Haiku 4.5" }),
        ];
        let models = parse_models(&entries);
        assert_eq!(models[0].display_name, "Claude Opus 5");
        assert_eq!(models[0].max_output_tokens, Some(128_000));
        assert!(models[1].recommended);
    }

    #[test]
    fn parses_stream_events() {
        let text = parse_event_(&event(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
        ))
        .unwrap();
        assert_eq!(text.text.as_deref(), Some("Hello"));

        let thinking = parse_event_(&event(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#,
        ))
        .unwrap();
        assert_eq!(thinking.text, None);

        let stop = parse_event_(&event(
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":12}}"#,
        ))
        .unwrap();
        assert_eq!(stop.finish, Some(Finish::MaxTokens));

        assert!(
            parse_event_(&event("message_stop", r#"{"type":"message_stop"}"#))
                .unwrap()
                .done
        );
        assert!(
            parse_event_(&event("ping", r#"{"type":"ping"}"#)).unwrap() == StreamPiece::default()
        );

        let error = parse_event_(&event(
            "error",
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        ))
        .unwrap_err();
        assert!(error.to_string().contains("Overloaded"));
    }

    #[test]
    fn streams_and_sends_tool_use() {
        use crate::llm::{ToolCall, ToolOutput, ToolRound};
        let start = parse_event_(&event(
            "content_block_start",
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"mcp_jobs_search","input":{}}}"#,
        ))
        .unwrap();
        assert_eq!(start.tools[0].index, Some(1));
        assert_eq!(start.tools[0].name.as_deref(), Some("mcp_jobs_search"));
        let part = parse_event_(&event(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"q\": \"rust\"}"}}"#,
        ))
        .unwrap();
        assert_eq!(part.tools[0].arguments, r#"{"q": "rust"}"#);

        let request = ChatRequest {
            turns: vec![Turn {
                role: MessageRole::User,
                content: "Find jobs".into(),
            }],
            rounds: vec![ToolRound {
                text: "Searching.".into(),
                calls: vec![ToolCall {
                    id: "toolu_1".into(),
                    name: "mcp_jobs_search".into(),
                    arguments: json!({ "q": "rust" }),
                    provider_data: None,
                }],
                outputs: vec![ToolOutput {
                    content: "failed".into(),
                    is_error: true,
                }],
                ..ToolRound::default()
            }],
            ..ChatRequest::default()
        };
        let body = request_body("claude-opus-5", &request);
        assert_eq!(body["messages"][1]["content"][0]["text"], "Searching.");
        assert_eq!(body["messages"][1]["content"][1]["type"], "tool_use");
        assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(body["messages"][2]["content"][0]["is_error"], true);
        assert!(body.get("tools").is_none(), "no tools unless offered");
    }

    /// A step in which Claude thought, searched once (done), asked for a
    /// second search (not run yet) and called a ReMa tool.
    fn mixed_step() -> crate::llm::ToolRound {
        use crate::llm::{ToolCall, ToolOutput, ToolRound};
        ToolRound {
            content: Some(json!([
                { "type": "thinking", "thinking": "Check mail.", "signature": "sig" },
                { "type": "server_tool_use", "id": "srvtoolu_done", "name": "web_search",
                  "input": { "query": "Acme GmbH" } },
                { "type": "web_search_tool_result", "tool_use_id": "srvtoolu_done", "content": [] },
                { "type": "server_tool_use", "id": "srvtoolu_open", "name": "web_search",
                  "input": { "query": "Acme GmbH interview" } },
                { "type": "tool_use", "id": "toolu_1", "name": "mail_search", "input": {} },
            ])),
            calls: vec![ToolCall {
                id: "toolu_1".into(),
                name: "mail_search".into(),
                arguments: json!({}),
                provider_data: None,
            }],
            outputs: vec![ToolOutput {
                content: "One email from Acme.".into(),
                is_error: false,
            }],
            ..ToolRound::default()
        }
    }

    fn sent_types(body: &Value) -> Vec<&str> {
        body["messages"][1]["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn a_search_that_has_not_run_is_left_out_once_the_web_is_off() {
        let request = ChatRequest {
            turns: vec![Turn {
                role: MessageRole::User,
                content: "Did Acme write?".into(),
            }],
            rounds: vec![mixed_step()],
            web_dropped: true,
            ..ChatRequest::default()
        };
        let body = request_body("claude-opus-5-5", &request);
        assert_eq!(
            sent_types(&body),
            [
                "thinking",
                "server_tool_use",
                "web_search_tool_result",
                "tool_use"
            ]
        );
        assert_eq!(body["messages"][1]["content"][1]["id"], "srvtoolu_done");
        assert_eq!(
            unresolved_web_calls(&request.rounds),
            [(
                "srvtoolu_open".to_string(),
                WebKind::Search,
                "Acme GmbH interview".to_string()
            )]
        );
        // The earlier thinking was written with the web tools declared: the
        // API is asked to drop it rather than refuse the request.
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(
            body["thinking"]["block_binding"]["prefix_mismatch_behavior"],
            "drop_block"
        );
    }

    #[test]
    fn a_search_that_has_not_run_is_kept_while_the_web_is_on() {
        let request = ChatRequest {
            turns: vec![Turn {
                role: MessageRole::User,
                content: "Did Acme write?".into(),
            }],
            rounds: vec![mixed_step()],
            web: Some(WebSearch::default()),
            ..ChatRequest::default()
        };
        let body = request_body("claude-opus-5-5", &request);
        assert_eq!(sent_types(&body).len(), 5);
        assert!(body.get("thinking").is_none(), "the model's default");
    }

    #[test]
    fn a_claude_sign_in_sends_every_beta_in_one_header() {
        use crate::llm::{ConnectionMethod, ProviderKind};
        let endpoint = Endpoint {
            kind: ProviderKind::Anthropic,
            name: "Anthropic".into(),
            connection: ConnectionMethod::ApiKey,
            base_url: DEFAULT_BASE_URL.into(),
            credential: Some(Credential::OAuth {
                access_token: "at".into(),
                refresh_token: None,
                expires_at: None,
            }),
            server_web_search: false,
        };
        let http = reqwest::Client::new();
        let built = authorize(&endpoint, http.post(DEFAULT_BASE_URL), &[BINDING_BETA])
            .build()
            .unwrap();
        let betas: Vec<_> = built.headers().get_all("anthropic-beta").iter().collect();
        assert_eq!(betas.len(), 1);
        assert_eq!(
            betas[0],
            "oauth-2025-04-20,thinking-binding-controls-2026-08-01"
        );
        let plain = authorize(&endpoint, http.post(DEFAULT_BASE_URL), &[])
            .build()
            .unwrap();
        assert_eq!(plain.headers()["anthropic-beta"], OAUTH_BETA);
    }

    #[test]
    fn models_without_default_thinking_need_no_thinking_controls() {
        let request = ChatRequest {
            turns: vec![Turn {
                role: MessageRole::User,
                content: "Did Acme write?".into(),
            }],
            rounds: vec![mixed_step()],
            web_dropped: true,
            ..ChatRequest::default()
        };
        let body = request_body("claude-sonnet-4-6", &request);
        assert!(body.get("thinking").is_none());
        assert!(!drops_stale_thinking("claude-sonnet-4-6", &request));
        assert!(drops_stale_thinking("claude-opus-5-5", &request));
    }
}
