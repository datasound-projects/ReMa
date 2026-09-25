//! OpenAI Chat Completions API — used for OpenAI and for every
//! OpenAI-compatible endpoint.

use reqwest::RequestBuilder;
use serde_json::{json, Value};

use super::{
    http::{error_message, join_url, send_json},
    sse::SseEvent,
    ChatRequest, Endpoint, FetchedModel, Finish, StreamPiece,
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
    json!({ "model": model_id, "messages": messages, "stream": true })
}

pub fn chat_request(
    http: &reqwest::Client,
    endpoint: &Endpoint,
    model_id: &str,
    request: &ChatRequest,
) -> RequestBuilder {
    endpoint.bearer(
        http.post(join_url(&endpoint.base_url, "chat/completions"))
            .json(&request_body(model_id, request)),
    )
}

pub fn parse_event(event: &SseEvent) -> AppResult<StreamPiece> {
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
    Ok(StreamPiece {
        text,
        finish,
        done: false,
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
            max_output_tokens: None,
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
        let piece = parse_event(&event(
            r#"{"choices":[{"delta":{"content":"Hel"},"finish_reason":null}]}"#,
        ))
        .unwrap();
        assert_eq!(piece.text.as_deref(), Some("Hel"));
        assert_eq!(piece.finish, None);

        let piece = parse_event(&event(
            r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#,
        ))
        .unwrap();
        assert_eq!(piece.finish, Some(Finish::MaxTokens));

        assert!(parse_event(&event("[DONE]")).unwrap().done);
        assert!(parse_event(&event(r#"{"error":{"message":"model not loaded"}}"#)).is_err());
    }
}
