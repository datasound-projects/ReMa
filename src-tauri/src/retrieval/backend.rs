//! Optional search services (Settings → Career Search → Advanced): Brave
//! Search API, Tavily or a SearXNG instance. They only add results to
//! ReMa's own career search; nothing depends on one being set up, and a
//! misconfigured one is skipped rather than failing a search.
//!
//! Keys stay in the OS credential store and are never put in prompts, logs
//! or error messages. Searches are plain HTTPS API calls: no browser, no
//! scraping of search result pages.

use std::time::Duration;

use reqwest::{header, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use tokio_util::sync::CancellationToken;

use super::{intent::JobQuery, listings::is_search_page, Candidate, Found, Progress};
use crate::{
    analytics::normalize,
    db::providers as settings,
    error::{AppError, AppResult},
    llm::{http::scrub, WebEvent, WebKind, WebSource},
    state::AppState,
};

pub const KIND_KEY: &str = "websearch.kind";
pub const URL_KEY: &str = "websearch.url";
/// Credential-store account of the service's API key.
pub const SECRET: &str = "websearch.key";

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RETRY_WAIT: Duration = Duration::from_secs(5);
const RESULTS: usize = 20;

/// A search service ReMa can call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Brave,
    Tavily,
    Searxng,
}

impl ServiceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Brave => "brave",
            Self::Tavily => "tavily",
            Self::Searxng => "searxng",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "brave" => Some(Self::Brave),
            "tavily" => Some(Self::Tavily),
            "searxng" => Some(Self::Searxng),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Brave => "Brave Search",
            Self::Tavily => "Tavily",
            Self::Searxng => "SearXNG",
        }
    }

    pub fn needs_key(self) -> bool {
        !matches!(self, Self::Searxng)
    }

    /// The official API address. Debug builds accept a local mock
    /// (`REMA_BRAVE_URL`, `REMA_TAVILY_URL`) for end-to-end tests.
    fn default_url(self) -> Option<String> {
        let (official, var) = match self {
            Self::Brave => (
                "https://api.search.brave.com/res/v1/web/search",
                "REMA_BRAVE_URL",
            ),
            Self::Tavily => ("https://api.tavily.com/search", "REMA_TAVILY_URL"),
            Self::Searxng => return None,
        };
        #[cfg(debug_assertions)]
        if let Ok(url) = std::env::var(var) {
            return Some(url);
        }
        let _ = var;
        Some(official.to_string())
    }
}

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: Option<String>,
    /// "3 days ago", "2026-09-20T…".
    pub age: Option<String>,
}

/// A configured search service.
pub struct Service {
    pub kind: ServiceKind,
    endpoint: String,
    key: Option<String>,
    http: reqwest::Client,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("ReMa/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// The service set up in Settings, if any.
pub async fn configured(state: &AppState) -> AppResult<Option<Service>> {
    let (kind, url) = state.db.call(|c| {
        Ok((
            settings::get_setting(c, KIND_KEY)?,
            settings::get_setting(c, URL_KEY)?,
        ))
    })?;
    let Some(kind) = kind.as_deref().and_then(ServiceKind::parse) else {
        return Ok(None);
    };
    let key = if kind.needs_key() {
        match state.vault.get_text(SECRET).await? {
            Some(key) if !key.trim().is_empty() => Some(key),
            _ => {
                return Err(AppError::configuration(format!(
                    "{} has no API key; add it under Settings → Career Search → Advanced",
                    kind.name()
                )))
            }
        }
    } else {
        None
    };
    Service::new(kind, url.as_deref(), key).map(Some)
}

impl Service {
    pub fn new(kind: ServiceKind, url: Option<&str>, key: Option<String>) -> AppResult<Self> {
        let endpoint = match kind {
            ServiceKind::Searxng => {
                let base = url
                    .map(str::trim)
                    .filter(|u| !u.is_empty())
                    .ok_or_else(|| AppError::configuration("Enter the SearXNG address"))?;
                let parsed = reqwest::Url::parse(base).map_err(|_| {
                    AppError::validation("The SearXNG address must start with http:// or https://")
                })?;
                if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                    return Err(AppError::validation(
                        "The SearXNG address must start with http:// or https://",
                    ));
                }
                format!("{}/search", base.trim_end_matches('/'))
            }
            other => other.default_url().unwrap_or_default(),
        };
        Ok(Self {
            kind,
            endpoint,
            key,
            http: client(),
        })
    }

    pub fn name(&self) -> &'static str {
        self.kind.name()
    }

    fn secrets(&self) -> Vec<&str> {
        self.key.as_deref().into_iter().collect()
    }

    fn request(
        &self,
        query: &str,
        days: Option<u32>,
        domain: Option<&str>,
    ) -> reqwest::RequestBuilder {
        // Brave and SearXNG take the `site:` operator; Tavily a domain list.
        let scoped;
        let query = match (self.kind, domain) {
            (ServiceKind::Brave | ServiceKind::Searxng, Some(domain)) => {
                scoped = format!("{query} site:{domain}");
                scoped.as_str()
            }
            _ => query,
        };
        let range = days.map(|d| match d {
            0..=1 => "day",
            2..=7 => "week",
            8..=31 => "month",
            _ => "year",
        });
        match self.kind {
            ServiceKind::Brave => {
                let mut params = vec![("q", query.to_string()), ("count", RESULTS.to_string())];
                if let Some(range) = range {
                    let freshness = match range {
                        "day" => "pd",
                        "week" => "pw",
                        "month" => "pm",
                        _ => "py",
                    };
                    params.push(("freshness", freshness.to_string()));
                }
                self.http
                    .get(&self.endpoint)
                    .query(&params)
                    .header(header::ACCEPT, "application/json")
                    .header("X-Subscription-Token", self.key.clone().unwrap_or_default())
            }
            ServiceKind::Tavily => {
                let mut body = json!({
                    "query": query,
                    "max_results": RESULTS.min(20),
                    "search_depth": "basic",
                    "include_answer": false,
                });
                if let Some(range) = range {
                    body["time_range"] = json!(range);
                }
                if let Some(domain) = domain {
                    body["include_domains"] = json!([domain]);
                }
                self.http
                    .post(&self.endpoint)
                    .bearer_auth(self.key.clone().unwrap_or_default())
                    .json(&body)
            }
            ServiceKind::Searxng => {
                let mut params = vec![("q", query.to_string()), ("format", "json".to_string())];
                if let Some(range) = range {
                    params.push(("time_range", range.to_string()));
                }
                self.http
                    .get(&self.endpoint)
                    .query(&params)
                    .header(header::ACCEPT, "application/json")
            }
        }
    }

    fn parse(&self, body: &Value) -> Vec<Hit> {
        let (list, snippet_key, age_keys): (&str, &str, &[&str]) = match self.kind {
            ServiceKind::Brave => ("/web/results", "description", &["age", "page_age"]),
            ServiceKind::Tavily => ("/results", "content", &["published_date"]),
            ServiceKind::Searxng => ("/results", "content", &["publishedDate"]),
        };
        body.pointer(list)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let url = normalize::web_url(item.get("url")?.as_str()?)?;
                let title = item
                    .get("title")
                    .and_then(Value::as_str)
                    .map(|t| normalize::clip(t, 200))
                    .unwrap_or_default();
                let text = |key: &str| {
                    item.get(key)
                        .and_then(Value::as_str)
                        .map(|t| normalize::clip(t, 400))
                        .filter(|t| !t.is_empty())
                };
                Some(Hit {
                    title,
                    url,
                    snippet: text(snippet_key),
                    age: age_keys.iter().find_map(|k| text(k)),
                })
            })
            .collect()
    }

    fn error(&self, status: StatusCode, body: &str) -> String {
        let detail = scrub(
            &normalize::clip(
                &serde_json::from_str::<Value>(body)
                    .ok()
                    .and_then(|v| crate::llm::http::error_message(&v))
                    .unwrap_or_default(),
                200,
            ),
            &self.secrets(),
        );
        match (status, self.kind) {
            (StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN, ServiceKind::Searxng) => {
                "the instance refused the request; enable the JSON format (search.formats: json) \
                 in its settings"
                    .to_string()
            }
            (StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN, _) => {
                "the API key was rejected; check it under Settings → Career Search → Advanced"
                    .to_string()
            }
            (StatusCode::TOO_MANY_REQUESTS, _) => "the rate limit or quota is reached".to_string(),
            _ if detail.is_empty() => format!("the service answered {}", status.as_u16()),
            _ => format!("the service answered {}: {detail}", status.as_u16()),
        }
    }

    /// One search, retried once after a rate limit, cancellable.
    pub async fn search(
        &self,
        query: &str,
        days: Option<u32>,
        cancel: &CancellationToken,
    ) -> Result<Vec<Hit>, String> {
        self.search_in(query, days, None, cancel).await
    }

    /// A search limited to one site (`domain`), when given.
    pub async fn search_in(
        &self,
        query: &str,
        days: Option<u32>,
        domain: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Vec<Hit>, String> {
        for attempt in 0..2 {
            let response = tokio::select! {
                _ = cancel.cancelled() => return Err("the search was stopped".into()),
                response = self.request(query, days, domain).send() => response,
            };
            // Callers name the service, so these messages don't.
            let response = response.map_err(|e| {
                if e.is_timeout() {
                    "no answer in time".to_string()
                } else {
                    "could not connect; check the internet connection".to_string()
                }
            })?;
            let status = response.status();
            if status == StatusCode::TOO_MANY_REQUESTS && attempt == 0 {
                let wait = response
                    .headers()
                    .get(header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(Duration::from_secs(2))
                    .min(MAX_RETRY_WAIT);
                tokio::select! {
                    _ = cancel.cancelled() => return Err("the search was stopped".into()),
                    _ = tokio::time::sleep(wait) => {}
                }
                continue;
            }
            let body = response
                .text()
                .await
                .map_err(|_| "the answer was incomplete".to_string())?;
            if !status.is_success() {
                return Err(self.error(status, &body));
            }
            let json: Value = serde_json::from_str(&body)
                .map_err(|_| "the answer could not be read".to_string())?;
            return Ok(self.parse(&json));
        }
        Err("the rate limit or quota is reached".into())
    }

    /// Searches for the request's postings and reports each search.
    pub async fn search_jobs(
        &self,
        query: &JobQuery,
        progress: &dyn Progress,
        cancel: &CancellationToken,
    ) -> Result<Found, String> {
        let observer = progress.web();
        let mut candidates: Vec<Candidate> = query
            .verify_urls
            .iter()
            .map(|url| Candidate {
                url: url.clone(),
                listed: true,
                reported: true,
                ..Candidate::default()
            })
            .collect();
        let mut searches = 0;
        let mut first_error = None;
        let queries = if query.verify_urls.is_empty() {
            job_queries(query)
        } else {
            Vec::new()
        };
        for (n, text) in queries.iter().enumerate() {
            let id = format!("{}:{n}", self.kind.as_str());
            if let Some(o) = &observer {
                o.observe(WebEvent::Started {
                    id: id.clone(),
                    kind: WebKind::Search,
                    target: text.clone(),
                });
            }
            match self.search(text, query.posted_within_days, cancel).await {
                Ok(hits) => {
                    searches += 1;
                    if let Some(o) = &observer {
                        o.observe(WebEvent::Finished {
                            id,
                            kind: WebKind::Search,
                            target: text.clone(),
                            sources: hits
                                .iter()
                                .map(|h| WebSource {
                                    title: h.title.clone(),
                                    url: h.url.clone(),
                                })
                                .collect(),
                            error: None,
                        });
                    }
                    candidates.extend(hits.into_iter().filter(|h| !is_search_page(&h.url)).map(
                        |h| Candidate {
                            url: h.url,
                            title: (!h.title.is_empty()).then_some(h.title),
                            posted: h.age,
                            snippet: h.snippet,
                            reported: true,
                            ..Candidate::default()
                        },
                    ));
                }
                Err(error) => {
                    if let Some(o) = &observer {
                        o.observe(WebEvent::Finished {
                            id,
                            kind: WebKind::Search,
                            target: text.clone(),
                            sources: Vec::new(),
                            error: Some(error.clone()),
                        });
                    }
                    if cancel.is_cancelled() {
                        return Err(error);
                    }
                    first_error.get_or_insert(error);
                }
            }
        }
        if searches == 0 && query.verify_urls.is_empty() {
            return Err(first_error.unwrap_or_else(|| "no search ran".into()));
        }
        Ok(Found {
            engine: self.name().to_string(),
            searches,
            candidates,
            lists_sources: true,
        })
    }
}

/// Searches for a request: the role with the place, and a posting-focused
/// variant.
pub fn job_queries(query: &JobQuery) -> Vec<String> {
    let role = query
        .role
        .clone()
        .unwrap_or_else(|| normalize::clip(&query.text, 80));
    let place = match (&query.location, query.remote) {
        (Some(location), true) => format!("{location} OR remote"),
        (Some(location), false) => location.clone(),
        (None, true) => "remote".to_string(),
        (None, false) => String::new(),
    };
    let mut out = vec![
        format!("{role} jobs {place}"),
        format!("\"{role}\" {place} job posting apply"),
    ];
    if let Some(company) = &query.company {
        out.insert(0, format!("{company} careers {role} {place}"));
    }
    out.iter_mut()
        .for_each(|q| *q = q.split_whitespace().collect::<Vec<_>>().join(" "));
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_job_queries() {
        let query = super::super::detect("Find current AI Engineer jobs in Vienna").unwrap();
        assert_eq!(
            job_queries(&query),
            vec![
                "AI Engineer jobs Vienna",
                "\"AI Engineer\" Vienna job posting apply"
            ]
        );
    }

    #[test]
    fn reads_each_services_results() {
        let brave = Service::new(ServiceKind::Brave, None, Some("k".into())).unwrap();
        let hits = brave.parse(&json!({ "web": { "results": [
            { "title": "AI Engineer – Nordlicht", "url": "https://careers.nordlicht.example/jobs/ai-4411", "description": "Vienna, hybrid.", "age": "3 days ago" },
            { "title": "bad", "url": "javascript:alert(1)" },
        ]}}));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].age.as_deref(), Some("3 days ago"));

        let tavily = Service::new(ServiceKind::Tavily, None, Some("k".into())).unwrap();
        let hits = tavily.parse(&json!({ "results": [
            { "title": "ML Engineer", "url": "https://jobs.example.com/ml-1", "content": "Apply now", "published_date": "2026-09-20" },
        ]}));
        assert_eq!(hits[0].snippet.as_deref(), Some("Apply now"));

        let searx =
            Service::new(ServiceKind::Searxng, Some("http://localhost:8888/"), None).unwrap();
        assert_eq!(searx.endpoint, "http://localhost:8888/search");
        assert!(Service::new(ServiceKind::Searxng, Some("ftp://x"), None).is_err());
        assert!(Service::new(ServiceKind::Searxng, None, None).is_err());
    }

    #[test]
    fn never_shows_the_key_in_errors() {
        let brave = Service::new(ServiceKind::Brave, None, Some("secret-key-123".into())).unwrap();
        let message = brave.error(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"bad token secret-key-123"}}"#,
        );
        assert!(!message.contains("secret-key-123"));
        assert!(brave
            .error(StatusCode::UNAUTHORIZED, "")
            .contains("API key was rejected"));
    }
}
