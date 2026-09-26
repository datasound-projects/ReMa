//! Anthropic Messages API.
//!
//! With web search on, the request declares Anthropic's server tools
//! (`web_search`, and `web_fetch` on models that have the current
//! versions). They run on Anthropic's side; the stream reports what they
//! did, and the response's content blocks are kept so a `pause_turn` can
//! be resumed exactly as the API expects.

use reqwest::RequestBuilder;
use serde_json::{json, Value};

use super::{
    http::{error_message, join_url, send_json},
    sse::SseEvent,
    ChatRequest, Endpoint, FetchedModel, Finish, StreamPiece, StreamState, ToolDelta, WebEvent,
    WebKind, WebSource,
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

/// Output cap for streaming requests when the model allows at least this much.
const STREAMING_MAX_TOKENS: u32 = 64_000;
/// Safe for every current model when the provider did not report a limit.
const FALLBACK_MAX_TOKENS: u32 = 8_192;
const RECOMMENDED_COUNT: usize = 4;
/// Most searches and page fetches in one answer.
const MAX_WEB_USES: u32 = 8;

fn authorize(endpoint: &Endpoint, request: RequestBuilder) -> RequestBuilder {
    let request = request.header("anthropic-version", API_VERSION);
    match &endpoint.credential {
        Some(Credential::ApiKey { key }) => request.header("x-api-key", key),
        Some(Credential::OAuth { access_token, .. }) => request
            .bearer_auth(access_token)
            .header("anthropic-beta", OAUTH_BETA),
        None => request,
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
        let body = send_json(authorize(endpoint, request), &endpoint.name, &secrets).await?;
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

/// Whether the model has the current web tools (`_20260209`, with dynamic
/// filtering): Claude Opus/Sonnet 4.6 and later, and every 5-series model.
/// Older models get the basic `web_search_20250305` only.
fn has_current_web_tools(model_id: &str) -> bool {
    let id = model_id.to_ascii_lowercase();
    let Some(rest) = id.strip_prefix("claude-") else {
        return false;
    };
    let mut parts = rest.split(['-', '.', '@']);
    let family = parts.next().unwrap_or_default();
    let major: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    // A dated snapshot (`20250929`) is not a minor version.
    let minor: u32 = parts
        .next()
        .filter(|p| p.len() <= 2)
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    match family {
        "opus" | "sonnet" => major >= 5 || (major == 4 && minor >= 6),
        "fable" => major >= 5,
        "haiku" => major >= 5,
        _ => false,
    }
}

fn web_tools(model_id: &str) -> Vec<Value> {
    if has_current_web_tools(model_id) {
        vec![
            json!({ "type": "web_search_20260209", "name": "web_search", "max_uses": MAX_WEB_USES }),
            json!({ "type": "web_fetch_20260209", "name": "web_fetch", "max_uses": MAX_WEB_USES }),
        ]
    } else {
        vec![
            json!({ "type": "web_search_20250305", "name": "web_search", "max_uses": MAX_WEB_USES }),
        ]
    }
}

pub fn request_body(model_id: &str, request: &ChatRequest) -> Value {
    let max_tokens = request
        .max_output_tokens
        .map_or(FALLBACK_MAX_TOKENS, |limit| limit.min(STREAMING_MAX_TOKENS));
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
    for round in &request.rounds {
        let content = match &round.content {
            Some(Value::Array(blocks)) => {
                blocks.iter().filter(|b| is_sendable(b)).cloned().collect()
            }
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
    if request.web.is_some() {
        tools.extend(web_tools(model_id));
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    body
}

pub fn chat_request(
    http: &reqwest::Client,
    endpoint: &Endpoint,
    model_id: &str,
    request: &ChatRequest,
) -> RequestBuilder {
    authorize(
        endpoint,
        http.post(join_url(&endpoint.base_url, "messages"))
            .json(&request_body(model_id, request)),
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
                    if let (Some(block), Some(citation)) = (block, data.pointer("/delta/citation"))
                    {
                        if !block["citations"].is_array() {
                            block["citations"] = json!([]);
                        }
                        if let Some(list) = block["citations"].as_array_mut() {
                            list.push(citation.clone());
                        }
                    }
                    StreamPiece::default()
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
        for id in [
            "claude-opus-5",
            "claude-fable-5-1",
            "claude-sonnet-5",
            "claude-opus-4-8",
            "claude-opus-4-6",
            "claude-sonnet-4-6",
        ] {
            assert!(has_current_web_tools(id), "{id}");
        }
        for id in [
            "claude-haiku-4-5-20251001",
            "claude-sonnet-4-5",
            "claude-opus-4-1",
            "claude-sonnet-4-20250514",
            "claude-3-7-sonnet-latest",
            "llama3",
        ] {
            assert!(!has_current_web_tools(id), "{id}");
        }
        let request = ChatRequest {
            web: Some(crate::llm::WebSearch::default()),
            ..ChatRequest::default()
        };
        assert_eq!(
            request_body("claude-haiku-4-5", &request)["tools"],
            json!([{ "type": "web_search_20250305", "name": "web_search", "max_uses": 8 }])
        );
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
}
