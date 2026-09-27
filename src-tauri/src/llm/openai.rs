//! OpenAI Chat Completions API — used for every OpenAI-compatible endpoint
//! (Ollama, LM Studio, vLLM, …). OpenAI itself is reached through the
//! Responses API (`openai_responses`); model listing is shared.

use std::time::Duration;

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
    models::{chat::MessageRole, provider::ProviderKind},
};

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// How many of the newest chat models are enabled on first connect.
const RECOMMENDED_COUNT: usize = 4;

pub async fn list_models(
    http: &reqwest::Client,
    endpoint: &Endpoint,
) -> AppResult<Vec<FetchedModel>> {
    let request = endpoint.bearer(http.get(join_url(&endpoint.base_url, "models")));
    let body = send_json(request, &endpoint.name, &endpoint.secrets()).await?;
    Ok(parse_models(&body, endpoint.kind == ProviderKind::Openai))
}

/// `official` applies OpenAI's naming to keep only chat-capable models.
pub fn parse_models(body: &Value, official: bool) -> Vec<FetchedModel> {
    let mut entries: Vec<(String, i64)> = body
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            let created = m.get("created").and_then(Value::as_i64).unwrap_or(0);
            Some((id, created))
        })
        .filter(|(id, _)| !official || is_chat_model(id))
        .collect();

    if official {
        // Aliases before dated snapshots, newest first.
        entries.sort_by(|(a, ca), (b, cb)| {
            is_snapshot(a)
                .cmp(&is_snapshot(b))
                .then(cb.cmp(ca))
                .then(a.cmp(b))
        });
    } else {
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
    }

    entries
        .into_iter()
        .enumerate()
        .map(|(index, (id, _))| FetchedModel {
            display_name: if official {
                pretty_name(&id)
            } else {
                id.clone()
            },
            recommended: official && index < RECOMMENDED_COUNT && !is_snapshot(&id),
            max_output_tokens: None,
            id,
        })
        .collect()
}

/// Models usable through Chat Completions (excludes audio, image,
/// embedding and Responses-API-only families).
fn is_chat_model(id: &str) -> bool {
    let family = id.starts_with("gpt-")
        || id.starts_with("chatgpt-")
        || (id.starts_with('o') && id[1..].starts_with(|c: char| c.is_ascii_digit()));
    const EXCLUDED: [&str; 12] = [
        "audio",
        "realtime",
        "transcribe",
        "tts",
        "image",
        "search",
        "embedding",
        "moderation",
        "instruct",
        "codex",
        "-pro",
        "deep-research",
    ];
    family && !EXCLUDED.iter().any(|part| id.contains(part))
}

/// `gpt-4o-2024-08-06`, `gpt-4-0613`
fn is_snapshot(id: &str) -> bool {
    let parts: Vec<&str> = id.rsplitn(4, '-').collect();
    let digits = |s: &str, n: usize| s.len() == n && s.chars().all(|c| c.is_ascii_digit());
    (parts.len() >= 3 && digits(parts[0], 2) && digits(parts[1], 2) && digits(parts[2], 4))
        || parts.first().is_some_and(|p| digits(p, 4))
}

fn pretty_name(id: &str) -> String {
    match id.strip_prefix("gpt-") {
        Some(rest) => format!("GPT-{rest}"),
        None => id.to_string(),
    }
}

pub fn request_body(model_id: &str, request: &ChatRequest) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = &request.system {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for turn in request.normalized_turns() {
        let role = match turn.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
        };
        messages.push(json!({ "role": role, "content": turn.content }));
    }
    // Tool use so far: the assistant's calls, then one message per result.
    for round in &request.rounds {
        let calls: Vec<Value> = round
            .calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": { "name": call.name, "arguments": arguments_text(&call.arguments) },
                })
            })
            .collect();
        let content = if round.text.trim().is_empty() {
            Value::Null
        } else {
            json!(round.text)
        };
        messages.push(json!({ "role": "assistant", "content": content, "tool_calls": calls }));
        for (call, output) in round.calls.iter().zip(&round.outputs) {
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": output.content,
            }));
        }
    }
    let mut body = json!({ "model": model_id, "messages": messages, "stream": true });
    let tools: Vec<Value> = request
        .tool_specs()
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                },
            })
        })
        .collect();
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    body
}

/// Arguments as the JSON text the API expects.
fn arguments_text(arguments: &Value) -> String {
    match arguments {
        Value::String(raw) => raw.clone(),
        other => other.to_string(),
    }
}

/// Unsloth Studio lists the models it serves as owned by it.
pub const UNSLOTH_OWNER: &str = "unsloth-studio";
/// Asks Unsloth Studio to stream its tool calls with the answer.
const UNSLOTH_EVENTS_HEADER: &str = "X-Unsloth-Events";

/// Whether a model list comes from Unsloth Studio.
pub fn lists_unsloth_models(body: &Value) -> bool {
    body.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|m| m.get("owned_by").and_then(Value::as_str) == Some(UNSLOTH_OWNER))
}

/// Whether the server runs its own web search tool (Unsloth Studio).
pub async fn serves_web_search(http: &reqwest::Client, endpoint: &Endpoint) -> bool {
    let request = endpoint.bearer(
        http.get(join_url(&endpoint.base_url, "models"))
            .timeout(Duration::from_secs(4)),
    );
    send_json(request, &endpoint.name, &endpoint.secrets())
        .await
        .is_ok_and(|body| lists_unsloth_models(&body))
}

/// Turns on the server's own web search for this request — and only it:
/// Unsloth Studio enables every local tool (code execution included) when
/// `enabled_tools` is left out. Its approval prompts cannot be answered
/// through the API, so it is told not to pause for them.
pub fn enable_server_web_search(body: &mut Value) {
    body["enable_tools"] = json!(true);
    body["enabled_tools"] = json!(["web_search"]);
    body["permission_mode"] = json!("off");
}

pub fn chat_request(
    http: &reqwest::Client,
    endpoint: &Endpoint,
    model_id: &str,
    request: &ChatRequest,
) -> RequestBuilder {
    let mut body = request_body(model_id, request);
    let mut builder = http.post(join_url(&endpoint.base_url, "chat/completions"));
    // Its own tool loop is not mixed with ReMa's tools in one request.
    if endpoint.server_web_search && request.web.is_some() && request.tool_specs().is_empty() {
        enable_server_web_search(&mut body);
        builder = builder.header(UNSLOTH_EVENTS_HEADER, "1");
    }
    endpoint.bearer(builder.json(&body))
}

/// "Title: …\nURL: …\nSnippet: …" blocks of a server web search result.
fn result_sources(result: &str) -> Vec<WebSource> {
    let mut out = Vec::new();
    let mut title = String::new();
    for line in result.lines() {
        let line = line.trim();
        if let Some(t) = line.strip_prefix("Title:") {
            title = t.trim().to_string();
        } else if let Some(url) = line.strip_prefix("URL:") {
            let url = url.trim();
            if url.starts_with("http://") || url.starts_with("https://") {
                out.push(WebSource {
                    title: std::mem::take(&mut title),
                    url: url.to_string(),
                });
            }
        }
    }
    out
}

/// Unsloth Studio's tool frames as web activity.
fn server_tool_event(kind: &str, chunk: &Value, state: &mut StreamState) -> Vec<WebEvent> {
    let id = chunk
        .get("tool_call_id")
        .and_then(Value::as_str)
        .unwrap_or("web")
        .to_string();
    let name = chunk.get("tool_name").and_then(Value::as_str);
    match kind {
        "tool_start" if name == Some("web_search") => {
            let arguments = chunk.get("arguments").cloned().unwrap_or(Value::Null);
            let text = |key: &str| {
                arguments
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            };
            let (web_kind, target) = match text("url") {
                Some(url) => (WebKind::Page, url),
                None => (WebKind::Search, text("query").unwrap_or_default()),
            };
            state
                .web_calls
                .insert(id.clone(), (web_kind, target.clone()));
            vec![WebEvent::Started {
                id,
                kind: web_kind,
                target,
            }]
        }
        "tool_end" => {
            // The end names the tool only sometimes; its start is known.
            let Some((web_kind, target)) = state.web_calls.remove(&id) else {
                return Vec::new();
            };
            let result = chunk
                .get("result")
                .map(|r| match r {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            let failed = result.starts_with("Failed to fetch URL")
                || result.starts_with("Search failed")
                || result.starts_with("Error");
            let sources = match (web_kind, failed) {
                (_, true) => Vec::new(),
                (WebKind::Search, false) => result_sources(&result),
                (WebKind::Page, false) => vec![WebSource {
                    title: target.clone(),
                    url: target.clone(),
                }],
            };
            vec![WebEvent::Finished {
                id,
                kind: web_kind,
                target,
                sources,
                error: failed.then(|| result.chars().take(200).collect()),
            }]
        }
        _ => Vec::new(),
    }
}

pub fn parse_event(event: &SseEvent, state: &mut StreamState) -> AppResult<StreamPiece> {
    let data = event.data.trim();
    if data == "[DONE]" {
        return Ok(StreamPiece::done());
    }
    let chunk: Value = serde_json::from_str(data)
        .map_err(|_| AppError::provider("the model server sent an unreadable stream event"))?;
    if chunk.get("error").is_some() {
        let message = error_message(&chunk).unwrap_or_else(|| "unknown error".into());
        return Err(AppError::provider(message));
    }
    // Unsloth Studio's own frames (tool calls, status) have a type and no
    // choices.
    if let Some(kind) = chunk.get("type").and_then(Value::as_str) {
        if chunk.get("choices").is_none() {
            return Ok(StreamPiece {
                web: server_tool_event(kind, &chunk, state),
                ..StreamPiece::default()
            });
        }
    }
    let choice = chunk.pointer("/choices/0");
    let text = choice
        .and_then(|c| c.pointer("/delta/content"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let finish = match choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(Value::as_str)
    {
        Some("length") => Some(Finish::MaxTokens),
        Some("content_filter") => Some(Finish::Refused),
        Some(_) => Some(Finish::Complete),
        None => None,
    };
    let tools = choice
        .and_then(|c| c.pointer("/delta/tool_calls"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|call| ToolDelta {
            index: Some(call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize),
            id: call.get("id").and_then(Value::as_str).map(str::to_string),
            name: call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .map(str::to_string),
            arguments: call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            provider_data: None,
        })
        .collect();
    Ok(StreamPiece {
        text,
        finish,
        tools,
        ..StreamPiece::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Turn;

    fn event(data: &str) -> SseEvent {
        SseEvent {
            event: None,
            data: data.into(),
        }
    }

    fn parse(event: &SseEvent) -> AppResult<StreamPiece> {
        parse_event(event, &mut StreamState::default())
    }

    #[test]
    fn keeps_only_chat_models_for_openai() {
        let body = json!({ "data": [
            { "id": "gpt-5", "created": 30 },
            { "id": "gpt-4o-2024-08-06", "created": 40 },
            { "id": "o3", "created": 20 },
            { "id": "text-embedding-3-large", "created": 50 },
            { "id": "gpt-4o-realtime-preview", "created": 50 },
            { "id": "o1-pro", "created": 50 },
            { "id": "dall-e-3", "created": 50 },
        ]});
        let models = parse_models(&body, true);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["gpt-5", "o3", "gpt-4o-2024-08-06"]);
        assert_eq!(models[0].display_name, "GPT-5");
        assert!(models[0].recommended && models[1].recommended);
        assert!(
            !models[2].recommended,
            "snapshots are not enabled by default"
        );
    }

    #[test]
    fn lists_everything_for_compatible_servers() {
        let body = json!({ "object": "list", "data": [{ "id": "llama3:8b" }, { "id": "embed" }] });
        let models = parse_models(&body, false);
        assert_eq!(models.len(), 2);
        assert!(models.iter().all(|m| !m.recommended));
    }

    #[test]
    fn builds_chat_completion_requests() {
        let request = ChatRequest {
            system: Some("Be brief.".into()),
            turns: vec![Turn {
                role: MessageRole::User,
                content: "Hi".into(),
            }],
            ..ChatRequest::default()
        };
        assert_eq!(
            request_body("gpt-5", &request),
            json!({
                "model": "gpt-5",
                "stream": true,
                "messages": [
                    { "role": "system", "content": "Be brief." },
                    { "role": "user", "content": "Hi" },
                ],
            })
        );
    }

    #[test]
    fn parses_stream_chunks() {
        let piece = parse(&event(
            r#"{"choices":[{"delta":{"content":"Hel"},"finish_reason":null}]}"#,
        ))
        .unwrap();
        assert_eq!(piece.text.as_deref(), Some("Hel"));
        assert_eq!(piece.finish, None);

        let piece = parse(&event(
            r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#,
        ))
        .unwrap();
        assert_eq!(piece.finish, Some(Finish::MaxTokens));

        assert!(parse(&event("[DONE]")).unwrap().done);
        assert!(parse(&event(r#"{"error":{"message":"model not loaded"}}"#)).is_err());
    }

    #[test]
    fn sends_tools_and_tool_results() {
        use crate::llm::{ToolCall, ToolOutput, ToolRound, ToolSpec};
        let request = ChatRequest {
            turns: vec![Turn {
                role: MessageRole::User,
                content: "Find jobs".into(),
            }],
            rounds: vec![ToolRound {
                text: String::new(),
                calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "mcp_jobs_search".into(),
                    arguments: json!({ "q": "rust" }),
                    provider_data: None,
                }],
                outputs: vec![ToolOutput {
                    content: "3 jobs".into(),
                    is_error: false,
                }],
                ..ToolRound::default()
            }],
            ..ChatRequest::default()
        };
        let mut request = request;
        request.tools = Some(crate::llm::ToolBox {
            specs: vec![ToolSpec {
                name: "mcp_jobs_search".into(),
                description: "Search jobs".into(),
                input_schema: json!({ "type": "object" }),
            }],
            executor: std::sync::Arc::new(crate::llm::fake::NoTools),
        });
        let body = request_body("gpt-5", &request);
        assert_eq!(body["tools"][0]["function"]["name"], "mcp_jobs_search");
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["function"]["arguments"],
            r#"{"q":"rust"}"#
        );
        assert_eq!(body["messages"][1]["content"], Value::Null);
        assert_eq!(
            body["messages"][2],
            json!({ "role": "tool", "tool_call_id": "call_1", "content": "3 jobs" })
        );
    }

    #[test]
    fn unsloth_runs_only_its_web_search_and_reports_it() {
        use crate::llm::{ToolSpec, WebSearch};
        assert!(lists_unsloth_models(
            &json!({ "data": [{ "id": "qwen", "owned_by": UNSLOTH_OWNER }] })
        ));
        assert!(!lists_unsloth_models(
            &json!({ "data": [{ "id": "llama3", "owned_by": "library" }] })
        ));

        let http = reqwest::Client::new();
        let endpoint = Endpoint {
            kind: ProviderKind::OpenaiCompatible,
            name: "Unsloth".into(),
            connection: crate::models::provider::ConnectionMethod::ApiKey,
            base_url: "http://127.0.0.1:8888/v1".into(),
            credential: None,
            server_web_search: true,
        };
        let mut request = ChatRequest {
            turns: vec![Turn {
                role: MessageRole::User,
                content: "AI jobs".into(),
            }],
            web: Some(WebSearch {
                required: true,
                ..WebSearch::default()
            }),
            ..ChatRequest::default()
        };
        let built = chat_request(&http, &endpoint, "qwen", &request)
            .build()
            .unwrap();
        assert_eq!(built.headers().get("x-unsloth-events").unwrap(), "1");
        let body: Value =
            serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["enable_tools"], json!(true));
        assert_eq!(
            body["enabled_tools"],
            json!(["web_search"]),
            "never python, terminal or files"
        );
        assert_eq!(body["permission_mode"], json!("off"));

        // With ReMa's own tools in the request, the server's loop stays off.
        request.tools = Some(crate::llm::ToolBox {
            specs: vec![ToolSpec {
                name: "x".into(),
                description: String::new(),
                input_schema: json!({"type":"object"}),
            }],
            executor: std::sync::Arc::new(crate::llm::fake::NoTools),
        });
        let built = chat_request(&http, &endpoint, "qwen", &request)
            .build()
            .unwrap();
        let body: Value =
            serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).unwrap();
        assert!(body.get("enable_tools").is_none());

        let mut state = StreamState::default();
        let start = parse_event(
            &event(r#"{"type":"tool_start","tool_name":"web_search","tool_call_id":"c1","arguments":{"query":"AI Engineer Vienna jobs"}}"#),
            &mut state,
        )
        .unwrap();
        assert_eq!(
            start.web,
            vec![WebEvent::Started {
                id: "c1".into(),
                kind: WebKind::Search,
                target: "AI Engineer Vienna jobs".into()
            }]
        );
        let end = parse_event(
            &event(r#"{"type":"tool_end","tool_call_id":"c1","result":"Title: AI Engineer - Nordlicht\nURL: https://boards.greenhouse.io/nordlicht/jobs/1\nSnippet: Vienna\n\n---\n\nIMPORTANT: These are only short snippets."}"#),
            &mut state,
        )
        .unwrap();
        let WebEvent::Finished { sources, error, .. } = &end.web[0] else {
            panic!()
        };
        assert_eq!(error, &None);
        assert_eq!(
            sources[0].url,
            "https://boards.greenhouse.io/nordlicht/jobs/1"
        );
        assert_eq!(sources[0].title, "AI Engineer - Nordlicht");
        // Status frames are not text.
        let status = parse_event(
            &event(r#"{"type":"tool_status","content":"Searching…"}"#),
            &mut state,
        )
        .unwrap();
        assert_eq!(status.text, None);
        assert!(status.web.is_empty());
    }

    #[test]
    fn parses_streamed_tool_calls() {
        let first = parse(&event(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"mcp_jobs_search","arguments":""}}]}}]}"#,
        ))
        .unwrap();
        assert_eq!(first.tools[0].id.as_deref(), Some("call_1"));
        let more = parse(&event(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"q\":"}}]}}]}"#,
        ))
        .unwrap();
        assert_eq!(more.tools[0].arguments, r#"{"q":"#);
        assert_eq!(more.tools[0].index, Some(0));
    }
}
