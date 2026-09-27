//! OpenAI Responses API — used for OpenAI's own API (an API key).
//!
//! Requests are stateless (`store: false`): earlier turns and tool steps are
//! sent as input items each time. With web search on, OpenAI's hosted
//! `web_search` tool is offered; OpenAI runs it and the stream reports the
//! searches.

use reqwest::RequestBuilder;
use serde_json::{json, Value};

use super::{
    http::{error_message, join_url},
    sse::SseEvent,
    ChatRequest, Endpoint, Finish, StreamPiece, StreamState, ToolDelta, WebEvent, WebKind,
    WebSource,
};
use crate::{
    error::{AppError, AppResult},
    models::chat::MessageRole,
};

pub fn request_body(model_id: &str, request: &ChatRequest) -> Value {
    let mut input: Vec<Value> = request
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
    // Tool use so far: the model's reasoning (encrypted, as OpenAI returned
    // it: the model keeps its train of thought across the stateless steps),
    // its text and calls, then their outputs.
    for round in &request.rounds {
        if let Some(Value::Array(items)) = &round.content {
            input.extend(items.iter().cloned());
        }
        if !round.text.trim().is_empty() {
            input.push(json!({ "role": "assistant", "content": round.text }));
        }
        for call in &round.calls {
            input.push(json!({
                "type": "function_call",
                "call_id": call.id,
                "name": call.name,
                "arguments": arguments_text(&call.arguments),
            }));
        }
        for (call, output) in round.calls.iter().zip(&round.outputs) {
            input.push(json!({
                "type": "function_call_output",
                "call_id": call.id,
                "output": output.content,
            }));
        }
    }
    let mut body = json!({
        "model": model_id,
        "input": input,
        "stream": true,
        "store": false,
    });
    if let Some(system) = &request.system {
        body["instructions"] = json!(system);
    }
    let mut tools: Vec<Value> = request
        .tool_specs()
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.input_schema,
                // MCP schemas are not written for strict mode.
                "strict": false,
            })
        })
        .collect();
    if let Some(web) = &request.web {
        // Live results, not only OpenAI's cached index.
        let mut tool = json!({ "type": "web_search", "external_web_access": true });
        if !web.allowed_domains.is_empty() {
            let domains: Vec<&String> = web
                .allowed_domains
                .iter()
                .take(super::OPENAI_MAX_DOMAINS)
                .collect();
            tool["filters"] = json!({ "allowed_domains": domains });
        }
        if let Some(location) = &web.location {
            tool["user_location"] = location.to_json();
        }
        tools.push(tool);
        body["include"] = json!(["web_search_call.action.sources"]);
        if web.required && request.tool_specs().is_empty() {
            // The retrieval step: the model must search before it answers.
            body["tool_choice"] = json!("required");
        }
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
        // The last round of a long tool loop answers without tools.
        if request.tools_off {
            body["tool_choice"] = json!("none");
        }
    }
    body
}

/// A reasoning item that can be sent back in a stateless request: one that
/// carries its encrypted content (without it, OpenAI would look the item up
/// by id, and nothing is stored).
fn reasoning_item(item: &Value) -> Option<Value> {
    (item.get("type").and_then(Value::as_str) == Some("reasoning")
        && item
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_some_and(|c| !c.is_empty()))
    .then(|| {
        let mut item = item.clone();
        if let Some(fields) = item.as_object_mut() {
            fields.remove("status");
        }
        item
    })
}

/// A `url_citation` annotation on the answer text, with the span it cites.
fn citation(annotation: &Value) -> Option<WebEvent> {
    if annotation.get("type").and_then(Value::as_str) != Some("url_citation") {
        return None;
    }
    let url = annotation.get("url").and_then(Value::as_str)?.to_string();
    let title = annotation
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(&url)
        .to_string();
    let index = |key: &str| {
        annotation
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
    };
    let range = index("start_index").zip(index("end_index"));
    Some(WebEvent::Cited {
        url,
        title,
        quote: None,
        range,
    })
}

/// Arguments as the JSON text the API expects.
fn arguments_text(arguments: &Value) -> String {
    match arguments {
        Value::String(raw) => raw.clone(),
        other => other.to_string(),
    }
}

pub fn chat_request(
    http: &reqwest::Client,
    endpoint: &Endpoint,
    model_id: &str,
    request: &ChatRequest,
) -> RequestBuilder {
    endpoint.bearer(
        http.post(join_url(&endpoint.base_url, "responses"))
            .json(&request_body(model_id, request)),
    )
}

/// A `web_search_call` item as web activity.
fn web_search(item: &Value, finished: bool) -> WebEvent {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let action = item.get("action").unwrap_or(&Value::Null);
    let kind = match action.get("type").and_then(Value::as_str) {
        Some("open_page" | "find_in_page") => WebKind::Page,
        _ => WebKind::Search,
    };
    // A page's address, or the queries of a search (`queries`; the single
    // `query` is the older field).
    let target = if kind == WebKind::Page {
        action
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        action
            .get("query")
            .and_then(Value::as_str)
            .filter(|q| !q.is_empty())
            .map(str::to_string)
            .or_else(|| super::joined_queries(action))
            .unwrap_or_default()
    };
    if !finished {
        return WebEvent::Started { id, kind, target };
    }
    let mut sources: Vec<WebSource> = action
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|source| {
            let url = source.get("url").and_then(Value::as_str)?.to_string();
            let title = source
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(&url)
                .to_string();
            Some(WebSource { title, url })
        })
        .collect();
    if kind == WebKind::Page && sources.is_empty() && !target.is_empty() {
        sources.push(WebSource {
            title: target.clone(),
            url: target.clone(),
        });
    }
    let error = match item.get("status").and_then(Value::as_str) {
        Some("failed") => Some("The search failed.".to_string()),
        Some("incomplete") => Some("The search did not finish.".to_string()),
        _ => None,
    };
    WebEvent::Finished {
        id,
        kind,
        target,
        sources,
        error,
    }
}

pub fn parse_event(event: &SseEvent, state: &mut StreamState) -> AppResult<StreamPiece> {
    let data: Value = serde_json::from_str(event.data.trim())
        .map_err(|_| AppError::provider("OpenAI sent an unreadable stream event"))?;
    let kind = data
        .get("type")
        .and_then(Value::as_str)
        .or(event.event.as_deref())
        .unwrap_or_default();
    let index = data
        .get("output_index")
        .and_then(Value::as_u64)
        .map(|i| i as usize);
    let item = data.get("item").unwrap_or(&Value::Null);
    let item_type = item.get("type").and_then(Value::as_str);
    Ok(match kind {
        "response.output_text.delta" | "response.refusal.delta" => StreamPiece {
            text: data
                .get("delta")
                .and_then(Value::as_str)
                .map(str::to_string),
            finish: (kind == "response.refusal.delta").then_some(Finish::Refused),
            ..StreamPiece::default()
        },
        "response.output_item.added" if item_type == Some("function_call") => {
            StreamPiece::tool(ToolDelta {
                index,
                id: item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                name: item.get("name").and_then(Value::as_str).map(str::to_string),
                arguments: item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                provider_data: None,
            })
        }
        "response.function_call_arguments.delta" => StreamPiece::tool(ToolDelta {
            index,
            arguments: data
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            ..ToolDelta::default()
        }),
        "response.output_item.done" if item_type == Some("reasoning") => {
            state.blocks.extend(reasoning_item(item));
            StreamPiece::default()
        }
        "response.output_item.added" | "response.output_item.done"
            if item_type == Some("web_search_call") =>
        {
            StreamPiece {
                web: vec![web_search(item, kind == "response.output_item.done")],
                ..StreamPiece::default()
            }
        }
        "response.output_text.annotation.added" => StreamPiece {
            web: data
                .get("annotation")
                .and_then(citation)
                .into_iter()
                .collect(),
            ..StreamPiece::default()
        },
        "response.completed" => StreamPiece {
            finish: Some(Finish::Complete),
            done: true,
            ..StreamPiece::default()
        },
        "response.incomplete" => StreamPiece {
            finish: Some(
                match data
                    .pointer("/response/incomplete_details/reason")
                    .and_then(Value::as_str)
                {
                    Some("content_filter") => Finish::Refused,
                    _ => Finish::MaxTokens,
                },
            ),
            done: true,
            ..StreamPiece::default()
        },
        "response.failed" => {
            let message = data
                .pointer("/response/error/message")
                .and_then(Value::as_str)
                .unwrap_or("the request failed");
            return Err(AppError::provider(format!("OpenAI: {message}")));
        }
        "error" => {
            let message = error_message(&data).unwrap_or_else(|| "unknown error".into());
            return Err(AppError::provider(format!("OpenAI: {message}")));
        }
        _ => StreamPiece::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ToolCall, ToolOutput, ToolRound, ToolSpec, Turn, WebSearch};

    fn event(data: &str) -> SseEvent {
        SseEvent {
            event: None,
            data: data.into(),
        }
    }

    fn parse(data: &str) -> StreamPiece {
        parse_event(&event(data), &mut StreamState::default()).unwrap()
    }

    #[test]
    fn builds_stateless_requests_with_web_search() {
        let mut request = ChatRequest {
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
                    content: "Find jobs".into(),
                },
            ],
            ..ChatRequest::default()
        };
        let body = request_body("gpt-6", &request);
        assert_eq!(body["instructions"], "Be brief.");
        assert_eq!(body["store"], false);
        assert_eq!(
            body["input"][1],
            json!({ "role": "assistant", "content": "Hello" })
        );
        assert!(body.get("tools").is_none(), "no tools unless asked");

        request.web = Some(WebSearch::default());
        let body = request_body("gpt-6", &request);
        assert_eq!(
            body["tools"],
            json!([{ "type": "web_search", "external_web_access": true }])
        );
        assert_eq!(body["include"][0], "web_search_call.action.sources");
        assert!(
            body.get("tool_choice").is_none(),
            "the model decides in chat"
        );

        // ReMa's retrieval step makes the search mandatory.
        request.web = Some(WebSearch {
            required: true,
            ..WebSearch::default()
        });
        assert_eq!(request_body("gpt-6", &request)["tool_choice"], "required");

        // A career search stays on career sites, localized to the request.
        request.web = Some(WebSearch {
            required: true,
            allowed_domains: vec!["karriere.at".into(), "greenhouse.io".into()],
            location: Some(crate::llm::ApproxLocation {
                city: Some("Vienna".into()),
                region: None,
                country: Some("AT".into()),
                timezone: Some("Europe/Vienna".into()),
            }),
            ..WebSearch::default()
        });
        let body = request_body("gpt-6", &request);
        assert_eq!(
            body["tools"][0],
            json!({
                "type": "web_search",
                "external_web_access": true,
                "filters": { "allowed_domains": ["karriere.at", "greenhouse.io"] },
                "user_location": { "type": "approximate", "city": "Vienna", "country": "AT", "timezone": "Europe/Vienna" }
            })
        );
        assert_eq!(body["tool_choice"], "required");

        // OpenAI accepts at most 100 sites.
        request.web = Some(WebSearch {
            allowed_domains: (0..130).map(|i| format!("site{i}.example")).collect(),
            ..WebSearch::default()
        });
        let body = request_body("gpt-6", &request);
        let domains = body["tools"][0]["filters"]["allowed_domains"]
            .as_array()
            .unwrap();
        assert_eq!(domains.len(), crate::llm::OPENAI_MAX_DOMAINS);
        assert_eq!(domains[0], "site0.example");
    }

    #[test]
    fn reports_url_citations() {
        let piece = parse(
            r#"{"type":"response.output_text.annotation.added","annotation":{"type":"url_citation","url":"https://jobs.example.com/1","title":"AI Engineer","start_index":0,"end_index":5}}"#,
        );
        assert_eq!(
            piece.web,
            vec![WebEvent::Cited {
                url: "https://jobs.example.com/1".into(),
                title: "AI Engineer".into(),
                quote: None,
                range: Some((0, 5)),
            }]
        );
    }

    #[test]
    fn reads_the_queries_of_a_search_and_its_outcome() {
        let done = parse(
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"web_search_call","id":"ws_2","status":"completed","action":{"type":"search","queries":["AI Engineer Wien","KI Ingenieur Wien"],"sources":[{"type":"url","url":"https://example.at/1"}]}}}"#,
        );
        assert!(
            matches!(&done.web[0], WebEvent::Finished { target, error: None, .. }
            if target == "AI Engineer Wien; KI Ingenieur Wien")
        );
        let cut = parse(
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"web_search_call","id":"ws_3","status":"incomplete","action":{"type":"search","queries":["x"]}}}"#,
        );
        assert!(
            matches!(&cut.web[0], WebEvent::Finished { error: Some(e), .. }
            if e.contains("did not finish"))
        );
    }

    #[test]
    fn sends_function_calls_and_outputs() {
        let request = ChatRequest {
            turns: vec![Turn {
                role: MessageRole::User,
                content: "Find jobs".into(),
            }],
            tools: Some(crate::llm::ToolBox {
                specs: vec![ToolSpec {
                    name: "mcp_jobs_search".into(),
                    description: "Search jobs".into(),
                    input_schema: json!({ "type": "object" }),
                }],
                executor: std::sync::Arc::new(crate::llm::fake::NoTools),
            }),
            rounds: vec![ToolRound {
                text: "Searching.".into(),
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
        let body = request_body("gpt-6", &request);
        assert_eq!(body["tools"][0]["name"], "mcp_jobs_search");
        assert_eq!(body["tools"][0]["strict"], false);
        assert_eq!(body["input"][1]["content"], "Searching.");
        assert_eq!(
            body["input"][2],
            json!({ "type": "function_call", "call_id": "call_1", "name": "mcp_jobs_search", "arguments": "{\"q\":\"rust\"}" })
        );
        assert_eq!(
            body["input"][3],
            json!({ "type": "function_call_output", "call_id": "call_1", "output": "3 jobs" })
        );
    }

    #[test]
    fn parses_text_calls_searches_and_endings() {
        let text = parse(
            r#"{"type":"response.output_text.delta","item_id":"m1","output_index":1,"delta":"Hel"}"#,
        );
        assert_eq!(text.text.as_deref(), Some("Hel"));

        let call = parse(
            r#"{"type":"response.output_item.added","output_index":2,"item":{"type":"function_call","id":"fc_1","call_id":"call_9","name":"mcp_jobs_search","arguments":""}}"#,
        );
        assert_eq!(call.tools[0].index, Some(2));
        assert_eq!(call.tools[0].id.as_deref(), Some("call_9"));
        let args = parse(
            r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":2,"delta":"{\"q\":1}"}"#,
        );
        assert_eq!(args.tools[0].arguments, r#"{"q":1}"#);

        let started = parse(
            r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"web_search_call","id":"ws_1","status":"in_progress"}}"#,
        );
        assert_eq!(
            started.web,
            vec![WebEvent::Started {
                id: "ws_1".into(),
                kind: WebKind::Search,
                target: String::new()
            }]
        );
        let done = parse(
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"web_search_call","id":"ws_1","status":"completed","action":{"type":"search","query":"AI jobs Vienna","sources":[{"type":"url","url":"https://example.com/job/1"}]}}}"#,
        );
        match &done.web[0] {
            WebEvent::Finished {
                target, sources, ..
            } => {
                assert_eq!(target, "AI jobs Vienna");
                assert_eq!(sources[0].url, "https://example.com/job/1");
            }
            other => panic!("unexpected {other:?}"),
        }

        let end = parse(r#"{"type":"response.completed","response":{"status":"completed"}}"#);
        assert!(end.done);
        assert_eq!(end.finish, Some(Finish::Complete));
        let cut = parse(
            r#"{"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}"#,
        );
        assert_eq!(cut.finish, Some(Finish::MaxTokens));
        assert!(parse_event(
            &event(r#"{"type":"response.failed","response":{"error":{"message":"boom"}}}"#),
            &mut StreamState::default()
        )
        .is_err());
        assert!(parse_event(
            &event(r#"{"type":"error","code":"x","message":"bad"}"#),
            &mut StreamState::default()
        )
        .is_err());
    }
}
