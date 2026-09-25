//! Scripted `LanguageModel` for service tests.

use std::{sync::Mutex, time::Duration};

use tokio_util::sync::CancellationToken;

use super::{BoxFuture, ChatRequest, DeltaSink, Endpoint, FetchedModel, Finish, LanguageModel};
use crate::error::{AppError, AppResult};

pub struct FakeLanguageModel {
    pub models: Vec<FetchedModel>,
    /// Text chunks streamed for every chat request.
    pub chunks: Vec<String>,
    pub delay: Duration,
    /// When set, streaming fails with this provider error after the chunks.
    pub fail_with: Option<String>,
    /// Requests received (model id, request).
    pub requests: Mutex<Vec<(String, ChatRequest)>>,
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
            requests: Mutex::default(),
        }
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
            for chunk in &self.chunks {
                tokio::select! {
                    _ = cancel.cancelled() => return Ok(Finish::Cancelled),
                    _ = tokio::time::sleep(self.delay) => on_delta(chunk),
                }
            }
            match &self.fail_with {
                Some(message) => Err(AppError::provider(message.clone())),
                None => Ok(Finish::Complete),
            }
        })
    }
}
