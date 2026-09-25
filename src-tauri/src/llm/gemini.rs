//! Google Gemini API (Generative Language API).

use reqwest::RequestBuilder;
use serde_json::{json, Value};

use super::{
    http::{error_message, join_url, send_json},
    sse::SseEvent,
    ChatRequest, Endpoint, FetchedModel, Finish, StreamPiece,
};
use crate::{
    error::{AppError, AppResult},
    models::chat::MessageRole,
    secrets::Credential,
};

pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const RECOMMENDED_COUNT: usize = 3;

fn authorize(endpoint: &Endpoint, request: RequestBuilder) -> RequestBuilder {
    match &endpoint.credential {
        // Sent as a header so the key never appears in URLs or logs.
        Some(Credential::ApiKey { key }) => request.header("x-goog-api-key", key),
        _ => endpoint.bearer(request),
    }
}

pub async fn list_models(
    http: &reqwest::Client,
    endpoint: &Endpoint,
) -> AppResult<Vec<FetchedModel>> {
    let secrets = endpoint.secrets();
    let mut entries = Vec::new();
    let mut page_token: Option<String> = None;
    for _ in 0..10 {
        let mut request = http
            .get(join_url(&endpoint.base_url, "models"))
            .query(&[("pageSize", "1000")]);
        if let Some(token) = &page_token {
            request = request.query(&[("pageToken", token.as_str())]);
        }
        let body = send_json(authorize(endpoint, request), &endpoint.name, &secrets).await?;
        entries.extend(
            body.get("models")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        match body.get("nextPageToken").and_then(Value::as_str) {
            Some(token) if !token.is_empty() => page_token = Some(token.to_string()),
            _ => break,
        }
    }
    Ok(parse_models(&entries))
}

pub fn parse_models(entries: &[Value]) -> Vec<FetchedModel> {
    let mut models: Vec<FetchedModel> = entries
        .iter()
        .filter(|m| {
            m.get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .is_some_and(|methods| methods.iter().any(|v| v == "generateContent"))
        })
        .filter_map(|m| {
            let name = m.get("name")?.as_str()?;
            let id = name.strip_prefix("models/").unwrap_or(name).to_string();
            if ["embedding", "aqa", "imagen", "tts", "image", "live"]
                .iter()
                .any(|p| id.contains(p))
            {
                return None;
            }
            Some(FetchedModel {
                display_name: m
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string(),
                max_output_tokens: m
                    .get("outputTokenLimit")
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok()),
                recommended: false,
                id,
            })
        })
        .collect();

    // Stable Gemini models first, highest version first.
    let stable = |id: &str| {
        // Pinned revisions end in `-001`, `-002`, …
        let pinned = id
            .rsplit('-')
            .next()
            .is_some_and(|s| s.len() == 3 && s.chars().all(|c| c.is_ascii_digit()));
        id.starts_with("gemini-")
            && !pinned
            && !["preview", "exp", "latest"].iter().any(|p| id.contains(p))
    };
    models.sort_by(|a, b| stable(&b.id).cmp(&stable(&a.id)).then(b.id.cmp(&a.id)));
    for model in models.iter_mut().take(RECOMMENDED_COUNT) {
        model.recommended = stable(&model.id);
    }
    models
}

pub fn request_body(request: &ChatRequest) -> Value {
    let contents: Vec<Value> = request
        .normalized_turns()
        .into_iter()
        .map(|turn| {
            let role = match turn.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "model",
            };
            json!({ "role": role, "parts": [{ "text": turn.content }] })
        })
        .collect();
    let mut body = json!({ "contents": contents });
    if let Some(system) = &request.system {
        body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
    }
    body
}

pub fn chat_request(
    http: &reqwest::Client,
    endpoint: &Endpoint,
    model_id: &str,
    request: &ChatRequest,
) -> RequestBuilder {
    let url = join_url(
        &endpoint.base_url,
        &format!("models/{model_id}:streamGenerateContent"),
    );
    authorize(
        endpoint,
        http.post(url)
            .query(&[("alt", "sse")])
            .json(&request_body(request)),
    )
}

pub fn parse_event(event: &SseEvent) -> AppResult<StreamPiece> {
    let data: Value = serde_json::from_str(event.data.trim())
        .map_err(|_| AppError::provider("Gemini sent an unreadable stream event"))?;
    if data.get("error").is_some() {
        let message = error_message(&data).unwrap_or_else(|| "unknown error".into());
        return Err(AppError::provider(format!("Gemini: {message}")));
    }
    if data.pointer("/promptFeedback/blockReason").is_some() {
        return Ok(StreamPiece {
            finish: Some(Finish::Refused),
            done: true,
            ..StreamPiece::default()
        });
    }
    let candidate = data.pointer("/candidates/0");
    let text: String = candidate
        .and_then(|c| c.pointer("/content/parts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        // Thought summaries are not part of the answer.
        .filter(|part| part.get("thought").and_then(Value::as_bool) != Some(true))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect();
    let finish = match candidate
        .and_then(|c| c.get("finishReason"))
        .and_then(Value::as_str)
    {
        Some("MAX_TOKENS") => Some(Finish::MaxTokens),
        Some("SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "RECITATION" | "SPII") => {
            Some(Finish::Refused)
        }
        Some(_) => Some(Finish::Complete),
        None => None,
    };
    Ok(StreamPiece {
        text: (!text.is_empty()).then_some(text),
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
    fn builds_generate_content_requests() {
        let request = ChatRequest {
            system: Some("Be brief.".into()),
            turns: vec![
                Turn {
                    role: MessageRole::User,
                    content: "Hi".into(),
                },
                Turn {
                    role: MessageRole::Assistant,
                    content: "Hello".into(),
                },
                Turn {
                    role: MessageRole::User,
                    content: "Bye".into(),
                },
            ],
            max_output_tokens: None,
        };
        assert_eq!(
            request_body(&request),
            json!({
                "contents": [
                    { "role": "user", "parts": [{ "text": "Hi" }] },
                    { "role": "model", "parts": [{ "text": "Hello" }] },
                    { "role": "user", "parts": [{ "text": "Bye" }] },
                ],
                "systemInstruction": { "parts": [{ "text": "Be brief." }] },
            })
        );
    }

    #[test]
    fn keeps_text_generation_models() {
        let entries = vec![
            json!({ "name": "models/gemini-2.5-flash", "displayName": "Gemini 2.5 Flash",
                    "supportedGenerationMethods": ["generateContent", "countTokens"], "outputTokenLimit": 65536 }),
            json!({ "name": "models/gemini-3-pro-preview", "displayName": "Gemini 3 Pro Preview",
                    "supportedGenerationMethods": ["generateContent"] }),
            json!({ "name": "models/text-embedding-004", "supportedGenerationMethods": ["embedContent"] }),
        ];
        let models = parse_models(&entries);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gemini-2.5-flash");
        assert!(models[0].recommended);
        assert_eq!(models[0].max_output_tokens, Some(65_536));
        assert!(
            !models[1].recommended,
            "previews are not enabled by default"
        );
    }

    #[test]
    fn parses_stream_chunks() {
        let piece = parse_event(&event(
            r#"{"candidates":[{"content":{"parts":[{"text":"thinking…","thought":true},{"text":"Hi"}],"role":"model"}}]}"#,
        ))
        .unwrap();
        assert_eq!(piece.text.as_deref(), Some("Hi"));

        let piece = parse_event(&event(
            r#"{"candidates":[{"content":{"parts":[{"text":"!"}]},"finishReason":"MAX_TOKENS"}]}"#,
        ))
        .unwrap();
        assert_eq!(piece.finish, Some(Finish::MaxTokens));

        let blocked =
            parse_event(&event(r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#)).unwrap();
        assert_eq!(blocked.finish, Some(Finish::Refused));

        assert!(parse_event(&event(
            r#"{"error":{"code":400,"message":"API key not valid"}}"#
        ))
        .is_err());
    }
}
