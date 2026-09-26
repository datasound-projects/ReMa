//! Scripted `LanguageModel` for service tests.

use std::{sync::Mutex, time::Duration};

use tokio_util::sync::CancellationToken;

use super::{
    BoxFuture, ChatRequest, DeltaSink, Endpoint, FetchedModel, Finish, LanguageModel, ToolCall,
    ToolExecutor, ToolOutput, ToolRound, WebEvent,
};
use crate::error::{AppError, AppResult};

/// An executor for requests that must not call tools.
pub struct NoTools;

impl ToolExecutor for NoTools {
    fn execute<'a>(&'a self, _call: &'a ToolCall) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async { ToolOutput::error("no tools") })
    }
}

pub struct FakeLanguageModel {
    pub models: Vec<FetchedModel>,
    /// Text chunks streamed for every chat request.
    pub chunks: Vec<String>,
    pub delay: Duration,
    /// When set, streaming fails with this provider error after the chunks.
    pub fail_with: Option<String>,
    /// Report `fail_with` as the account having no credits left.
    pub fail_billing: bool,
    /// Requests received (model id, request).
    pub requests: Mutex<Vec<(String, ChatRequest)>>,
    /// Tool calls to make, in order, before streaming `chunks` (only when
    /// the request offers tools). Each call's output is recorded.
    pub tool_calls: Vec<ToolCall>,
    /// What the tools returned, as a finished answer would have seen it.
    pub tool_outputs: Mutex<Vec<ToolOutput>>,
    /// Web activity reported (when the request allows web search) before
    /// the answer streams.
    pub web_events: Vec<WebEvent>,
}

impl FakeLanguageModel {
    pub fn replying(chunks: &[&str]) -> Self {
        Self {
            models: vec![
                model("model-a", true),
                model("model-b", true),
                model("model-c", false),
            ],
            chunks: chunks.iter().map(|c| c.to_string()).collect(),
            delay: Duration::ZERO,
            fail_with: None,
            fail_billing: false,
            requests: Mutex::default(),
            tool_calls: Vec::new(),
            tool_outputs: Mutex::default(),
            web_events: Vec::new(),
        }
    }

    /// Reports this web activity (when web search is allowed) first.
    pub fn searching(mut self, events: Vec<WebEvent>) -> Self {
        self.web_events = events;
        self
    }

    /// Calls these tools (when offered) before answering.
    pub fn calling(mut self, calls: Vec<ToolCall>) -> Self {
        self.tool_calls = calls;
        self
    }
}

pub fn model(id: &str, recommended: bool) -> FetchedModel {
    FetchedModel {
        id: id.into(),
        display_name: id.to_uppercase(),
        max_output_tokens: Some(4096),
        recommended,
    }
}

impl LanguageModel for FakeLanguageModel {
    fn list_models<'a>(
        &'a self,
        endpoint: &'a Endpoint,
    ) -> BoxFuture<'a, AppResult<Vec<FetchedModel>>> {
        Box::pin(async move {
            match &endpoint.credential {
                Some(crate::secrets::Credential::ApiKey { key }) if key == "bad-key" => {
                    Err(AppError::authentication("rejected"))
                }
                _ => Ok(self.models.clone()),
            }
        })
    }

    fn stream_chat<'a>(
        &'a self,
        _endpoint: &'a Endpoint,
        model_id: &'a str,
        request: &'a ChatRequest,
        cancel: CancellationToken,
        on_delta: DeltaSink<'a>,
    ) -> BoxFuture<'a, AppResult<Finish>> {
        Box::pin(async move {
            self.requests
                .lock()
                .unwrap()
                .push((model_id.to_string(), request.clone()));
            // Like a provider that calls every offered tool first.
            if let Some(tools) = &request.tools {
                let mut round = ToolRound::default();
                for call in &self.tool_calls {
                    if cancel.is_cancelled() {
                        return Ok(Finish::Cancelled);
                    }
                    let output = tools.executor.execute(call).await;
                    self.tool_outputs.lock().unwrap().push(output.clone());
                    round.calls.push(call.clone());
                    round.outputs.push(output);
                }
                if cancel.is_cancelled() {
                    return Ok(Finish::Cancelled);
                }
                if !round.calls.is_empty() {
                    let mut with_round = request.clone();
                    with_round.rounds.push(round);
                    self.requests
                        .lock()
                        .unwrap()
                        .push((model_id.to_string(), with_round));
                }
            }
            if let Some(observer) = request.web.as_ref().and_then(|w| w.observer.as_ref()) {
                for event in &self.web_events {
                    observer.observe(event.clone());
                }
            }
            for chunk in &self.chunks {
                tokio::select! {
                    _ = cancel.cancelled() => return Ok(Finish::Cancelled),
                    _ = tokio::time::sleep(self.delay) => on_delta(chunk),
                }
            }
            match &self.fail_with {
                Some(message) if self.fail_billing => Err(AppError::Billing(message.clone())),
                Some(message) => Err(AppError::provider(message.clone())),
                None => Ok(Finish::Complete),
            }
        })
    }
}
