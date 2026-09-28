//! Proof that a model's own web search runs: one live request, judged only
//! by what the provider reported (its search events, the pages they
//! returned, its citations) and by whether a returned page really loads —
//! never by what the answer says it did.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio_util::sync::CancellationToken;

use crate::{
    error::AppResult,
    llm::{ChatRequest, Endpoint, LanguageModel, Turn, WebEvent, WebKind, WebObserver, WebSearch},
    models::chat::MessageRole,
    rema_mcp::fetch::{Accept, FetchError, Fetcher},
};

/// A question only a search answers well: something published recently,
/// on a page the answer can cite.
pub const QUESTION: &str = "What is the newest stable Rust release announced on the official \
Rust blog (blog.rust-lang.org)? Give the version number, the date it was announced and the \
page it was announced on.";

/// What the provider reported while answering [`QUESTION`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchProof {
    /// Searches and page visits the provider reported as finished.
    pub searches: usize,
    /// Those that reported an error.
    pub failed: usize,
    /// Distinct pages the provider's searches returned or cited.
    pub sources: Vec<String>,
    /// Citations the provider attached to the answer.
    pub citations: usize,
    /// The first returned page ReMa could open itself, and how it answered.
    pub opened: Option<(String, String)>,
    /// Characters of answer text.
    pub answer_chars: usize,
}

impl SearchProof {
    /// Whether the evidence shows a real search, and what it shows.
    pub fn verdict(&self) -> Result<String, String> {
        if self.searches == 0 {
            return Err(
                "The model answered without its provider reporting a web search, so ReMa \
                 cannot confirm its search works."
                    .into(),
            );
        }
        if self.sources.is_empty() {
            return Err(format!(
                "The provider reported {} search(es) but returned no pages.",
                self.searches
            ));
        }
        let Some((url, how)) = &self.opened else {
            return Err(format!(
                "The provider returned {} page(s), but none of the first three could be opened.",
                self.sources.len()
            ));
        };
        Ok(format!(
            "The provider reported {} search(es) returning {} page(s), with {} citation(s); \
             ReMa opened {url} ({how}).",
            self.searches,
            self.sources.len(),
            self.citations
        ))
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<WebEvent>>);

impl WebObserver for Recorder {
    fn observe(&self, event: WebEvent) {
        self.0.lock().unwrap().push(event);
    }
}

/// Asks `model` [`QUESTION`] with its provider's own web search required,
/// and opens up to three of the pages it returned.
pub async fn prove(
    llm: &dyn LanguageModel,
    fetcher: &Fetcher,
    endpoint: &Endpoint,
    model: &str,
    cancel: CancellationToken,
) -> AppResult<SearchProof> {
    let recorder = Arc::new(Recorder::default());
    let request = ChatRequest {
        turns: vec![Turn {
            role: MessageRole::User,
            content: QUESTION.into(),
        }],
        max_output_tokens: Some(2_000),
        web: Some(WebSearch {
            observer: Some(recorder.clone()),
            required: true,
            ..WebSearch::default()
        }),
        ..ChatRequest::default()
    };
    let mut answer = String::new();
    let mut sink = |delta: &str| answer.push_str(delta);
    llm.stream_chat(endpoint, model, &request, cancel.clone(), &mut sink)
        .await?;
    let events = recorder.0.lock().unwrap().clone();
    let mut proof = summarize(&events);
    proof.answer_chars = answer.chars().count();
    let deadline = Instant::now() + Duration::from_secs(30);
    for url in proof.sources.iter().take(3) {
        let how = match fetcher
            .get(url, Accept::Html, 256 * 1024, None, deadline, &cancel)
            .await
        {
            Ok(response) => format!("loaded from {}", host_of(&response.final_url)),
            // The page exists; the site only declines automated readers.
            Err(FetchError::Refused(code)) => format!("exists; the site declines robots ({code})"),
            Err(_) => continue,
        };
        proof.opened = Some((url.clone(), how));
        break;
    }
    Ok(proof)
}

/// Counts what the provider reported.
pub fn summarize(events: &[WebEvent]) -> SearchProof {
    let mut proof = SearchProof::default();
    let mut sources = BTreeSet::new();
    let mut keep = |url: &str| {
        if url.starts_with("https://") || url.starts_with("http://") {
            sources.insert(url.to_string());
        }
    };
    for event in events {
        match event {
            WebEvent::Finished {
                kind,
                sources: found,
                error,
                target,
                ..
            } => {
                proof.searches += 1;
                if error.is_some() {
                    proof.failed += 1;
                    continue;
                }
                for source in found {
                    keep(&source.url);
                }
                if *kind == WebKind::Page {
                    keep(target);
                }
            }
            WebEvent::Cited { url, .. } => {
                proof.citations += 1;
                keep(url);
            }
            WebEvent::Started { .. } | WebEvent::Unavailable { .. } => {}
        }
    }
    proof.sources = sources.into_iter().collect();
    proof
}

fn host_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::WebSource;

    fn finished(kind: WebKind, target: &str, urls: &[&str], error: Option<&str>) -> WebEvent {
        WebEvent::Finished {
            id: "1".into(),
            kind,
            target: target.into(),
            sources: urls
                .iter()
                .map(|u| WebSource {
                    title: (*u).into(),
                    url: (*u).into(),
                })
                .collect(),
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn only_what_the_provider_reported_counts() {
        // No search event: whatever the answer claims, nothing is proven.
        assert!(summarize(&[]).verdict().is_err());
        let proof = summarize(&[
            finished(
                WebKind::Search,
                "rust release",
                &[
                    "https://blog.rust-lang.org/2026/09/18/Rust-1.95.0/",
                    "not a url",
                ],
                None,
            ),
            finished(WebKind::Search, "x", &[], Some("unavailable")),
            finished(WebKind::Page, "https://www.rust-lang.org/", &[], None),
            WebEvent::Cited {
                url: "https://blog.rust-lang.org/2026/09/18/Rust-1.95.0/".into(),
                title: "Rust 1.95.0".into(),
                quote: None,
                range: None,
            },
        ]);
        assert_eq!(proof.searches, 3);
        assert_eq!(proof.failed, 1);
        assert_eq!(proof.citations, 1);
        assert_eq!(
            proof.sources,
            [
                "https://blog.rust-lang.org/2026/09/18/Rust-1.95.0/",
                "https://www.rust-lang.org/"
            ]
        );
        // Pages that could not be opened prove nothing either.
        assert!(proof.verdict().unwrap_err().contains("could be opened"));
        let opened = SearchProof {
            opened: Some(("https://www.rust-lang.org/".into(), "loaded".into())),
            ..proof
        };
        assert!(opened
            .verdict()
            .unwrap()
            .contains("ReMa opened https://www.rust-lang.org/"));
    }
}
