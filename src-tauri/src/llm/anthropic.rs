//! Anthropic Messages API.

use reqwest::RequestBuilder;
use serde_json::{json, Value};

use super::{
    http::{error_message, join_url, send_json},
    sse::SseEvent,
    ChatRequest, Endpoint, FetchedModel, Finish, StreamPiece, ToolDelta,
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
    // Tool use so far: the assistant's text and calls, then the results.
    for round in &request.rounds {
        let mut content = Vec::new();
        if !round.text.trim().is_empty() {
            content.push(json!({ "type": "text", "text": round.text }));
        }
        for call in &round.calls {
            let input = match &call.arguments {
                Value::Object(_) => call.arguments.clone(),
                _ => json!({}),
            };
            content.push(
                json!({ "type": "tool_use", "id": call.id, "name": call.name, "input": input }),
            );
        }
        messages.push(json!({ "role": "assistant", "content": content }));
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
    let tools: Vec<Value> = request
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

pub fn parse_event(event: &SseEvent) -> AppResult<StreamPiece> {
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
        "content_block_start"
            if data.pointer("/content_block/type").and_then(Value::as_str) == Some("tool_use") =>
        {
            StreamPiece::tool(ToolDelta {
                index,
                id: data
                    .pointer("/content_block/id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                name: data
                    .pointer("/content_block/name")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                ..ToolDelta::default()
            })
        }
        "content_block_delta" => match data.pointer("/delta/type").and_then(Value::as_str) {
            // Thinking deltas are not shown; only the answer text streams.
            Some("text_delta") => StreamPiece::text(
                data.pointer("/delta/text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            Some("input_json_delta") => StreamPiece::tool(ToolDelta {
                index,
                arguments: data
                    .pointer("/delta/partial_json")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                ..ToolDelta::default()
            }),
            _ => StreamPiece::default(),
        },
        "message_delta" => StreamPiece {
            finish: match data.pointer("/delta/stop_reason").and_then(Value::as_str) {
                Some("max_tokens") => Some(Finish::MaxTokens),
                Some("refusal") => Some(Finish::Refused),
                Some(_) => Some(Finish::Complete),
                None => None,
            },
            ..StreamPiece::default()
        },
        "message_stop" => StreamPiece::done(),
        "error" => {
            let message = error_message(&data).unwrap_or_else(|| "unknown error".into());
            return Err(AppError::provider(format!("Anthropic: {message}")));
        }
        _ => StreamPiece::default(), // message_start, content_block_start/stop, ping
    })
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
        let text = parse_event(&event(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
        ))
        .unwrap();
        assert_eq!(text.text.as_deref(), Some("Hello"));

        let thinking = parse_event(&event(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#,
        ))
        .unwrap();
        assert_eq!(thinking.text, None);

        let stop = parse_event(&event(
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":12}}"#,
        ))
        .unwrap();
        assert_eq!(stop.finish, Some(Finish::MaxTokens));

        assert!(
            parse_event(&event("message_stop", r#"{"type":"message_stop"}"#))
                .unwrap()
                .done
        );
        assert!(
            parse_event(&event("ping", r#"{"type":"ping"}"#)).unwrap() == StreamPiece::default()
        );

        let error = parse_event(&event(
            "error",
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        ))
        .unwrap_err();
        assert!(error.to_string().contains("Overloaded"));
    }

    #[test]
    fn streams_and_sends_tool_use() {
        use crate::llm::{ToolCall, ToolOutput, ToolRound};
        let start = parse_event(&event(
            "content_block_start",
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"mcp_jobs_search","input":{}}}"#,
        ))
        .unwrap();
        assert_eq!(start.tools[0].index, Some(1));
        assert_eq!(start.tools[0].name.as_deref(), Some("mcp_jobs_search"));
        let part = parse_event(&event(
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
