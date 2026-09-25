//! Gmail (read-only) as a deterministic tool.
//!
//! The model never calls Gmail itself. Rust builds the queries, date windows
//! and pagination, fetches only what a workflow asks for, and returns plain
//! structured data. Bodies are converted to text and truncated.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

use crate::{
    error::{AppError, AppResult},
    llm::{
        http::{error_message, scrub},
        BoxFuture,
    },
};

/// Longest message body passed on (characters).
pub const MAX_BODY_CHARS: usize = 12_000;
/// Most messages returned by one search.
pub const MAX_SEARCH_RESULTS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRef {
    pub id: String,
    pub thread_id: String,
}

/// Headers and snippet only — enough to decide relevance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageMeta {
    pub id: String,
    pub thread_id: String,
    /// When Gmail received it (epoch ms).
    pub received_at: i64,
    pub from: String,
    pub subject: String,
    pub snippet: String,
}

impl MessageMeta {
    /// `recruiting@company.com` → `company.com`.
    pub fn sender_domain(&self) -> Option<String> {
        let address = match (self.from.rfind('<'), self.from.rfind('>')) {
            (Some(start), Some(end)) if start < end => &self.from[start + 1..end],
            _ => self.from.as_str(),
        };
        address
            .rsplit_once('@')
            .map(|(_, domain)| domain.trim().trim_end_matches('>').to_ascii_lowercase())
            .filter(|d| d.contains('.'))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageContent {
    pub meta: MessageMeta,
    /// Plain text (HTML converted, links kept as `text (url)`), truncated.
    pub body: String,
}

/// Read-only Gmail operations used by ReMa workflows.
pub trait GmailApi: Send + Sync {
    /// Message ids matching a Gmail search query (newest first).
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, AppResult<Vec<MessageRef>>>;
    fn metadata<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<MessageMeta>>;
    fn message<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<MessageContent>>;
    /// Headers of every message in a thread, oldest first.
    fn thread<'a>(&'a self, thread_id: &'a str) -> BoxFuture<'a, AppResult<Vec<MessageMeta>>>;
}

/// Words that job-application emails almost always contain (English and
/// German). Used to narrow the search before anything reaches a model.
const JOB_KEYWORDS: &[&str] = &[
    "application",
    "applied",
    "applying",
    "candidate",
    "candidacy",
    "interview",
    "recruiter",
    "recruiting",
    "recruitment",
    "hiring",
    "position",
    "role",
    "vacancy",
    "assessment",
    "\"next steps\"",
    "offer",
    "bewerbung",
    "vorstellungsgespräch",
    "absage",
    "stelle",
    "kennenlernen",
];

/// Gmail search for job-related mail received after `after_ms`.
pub fn job_search_query(after_ms: i64) -> String {
    format!(
        "after:{} -in:chats -in:sent -in:drafts -category:promotions -category:social {{{}}}",
        after_ms.div_euclid(1000),
        JOB_KEYWORDS.join(" ")
    )
}

/// Messages received since `after_ms` that match `extra_query`.
pub async fn messages_since(
    api: &dyn GmailApi,
    after_ms: i64,
    extra_query: &str,
) -> AppResult<Vec<MessageRef>> {
    let query = format!("after:{} {extra_query}", after_ms.div_euclid(1000));
    api.search(query.trim(), MAX_SEARCH_RESULTS).await
}

/// The real client for the Gmail REST API.
pub struct HttpGmail {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl HttpGmail {
    pub fn new(http: reqwest::Client, base: &str, access_token: String) -> Self {
        Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token: access_token,
        }
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> AppResult<Value> {
        let response = self
            .http
            .get(format!("{}/users/me/{path}", self.base))
            .bearer_auth(&self.token)
            .query(query)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status.is_success() {
            return serde_json::from_str(&body)
                .map_err(|_| AppError::provider("Gmail sent an unreadable response."));
        }
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| error_message(&v))
            .map(|m| scrub(&m, &[&self.token]))
            .unwrap_or_default();
        Err(google_error("Gmail", status.as_u16(), &detail))
    }
}

/// Maps Google API errors without exposing tokens or content.
pub fn google_error(service: &str, status: u16, detail: &str) -> AppError {
    match status {
        401 => AppError::authentication(format!(
            "{service} access expired or was revoked. Reconnect Google in Settings."
        )),
        403 if detail.to_lowercase().contains("insufficient") => AppError::authentication(format!(
            "ReMa does not have permission for {service}. Reconnect Google in Settings."
        )),
        403 => AppError::provider(format!("{service} refused the request: {detail}")),
        404 => AppError::not_found(format!("{service}: item not found")),
        429 => AppError::provider(format!("{service} rate limit reached. Try again later.")),
        s if s >= 500 => AppError::provider(format!("{service} is unavailable right now ({s}).")),
        s => AppError::provider(format!("{service} returned an error ({s}): {detail}")),
    }
}

impl GmailApi for HttpGmail {
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, AppResult<Vec<MessageRef>>> {
        Box::pin(async move {
            let mut refs = Vec::new();
            let mut page: Option<String> = None;
            while refs.len() < limit {
                let page_size = (limit - refs.len()).min(100).to_string();
                let mut query_params = vec![("q", query), ("maxResults", page_size.as_str())];
                if let Some(token) = &page {
                    query_params.push(("pageToken", token.as_str()));
                }
                let body = self.get("messages", &query_params).await?;
                refs.extend(parse_message_refs(&body));
                match body.get("nextPageToken").and_then(Value::as_str) {
                    Some(token) if !token.is_empty() => page = Some(token.to_string()),
                    _ => break,
                }
            }
            refs.truncate(limit);
            Ok(refs)
        })
    }

    fn metadata<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<MessageMeta>> {
        Box::pin(async move {
            let body = self
                .get(
                    &format!("messages/{id}"),
                    &[
                        ("format", "metadata"),
                        ("metadataHeaders", "From"),
                        ("metadataHeaders", "Subject"),
                    ],
                )
                .await?;
            parse_meta(&body)
        })
    }

    fn message<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<MessageContent>> {
        Box::pin(async move {
            let body = self
                .get(&format!("messages/{id}"), &[("format", "full")])
                .await?;
            Ok(MessageContent {
                meta: parse_meta(&body)?,
                body: message_text(&body),
            })
        })
    }

    fn thread<'a>(&'a self, thread_id: &'a str) -> BoxFuture<'a, AppResult<Vec<MessageMeta>>> {
        Box::pin(async move {
            let body = self
                .get(
                    &format!("threads/{thread_id}"),
                    &[
                        ("format", "metadata"),
                        ("metadataHeaders", "From"),
                        ("metadataHeaders", "Subject"),
                    ],
                )
                .await?;
            body.get("messages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(parse_meta)
                .collect()
        })
    }
}

pub fn parse_message_refs(body: &Value) -> Vec<MessageRef> {
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(MessageRef {
                id: m.get("id")?.as_str()?.to_string(),
                thread_id: m.get("threadId")?.as_str()?.to_string(),
            })
        })
        .collect()
}

fn header(message: &Value, name: &str) -> String {
    message
        .pointer("/payload/headers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|h| {
            h.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .and_then(|h| h.get("value").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

pub fn parse_meta(message: &Value) -> AppResult<MessageMeta> {
    let field = |name: &str| {
        message
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let id = field("id").ok_or_else(|| AppError::provider("Gmail message without id"))?;
    Ok(MessageMeta {
        thread_id: field("threadId").unwrap_or_else(|| id.clone()),
        received_at: field("internalDate")
            .and_then(|d| d.parse().ok())
            .unwrap_or_default(),
        from: header(message, "From"),
        subject: header(message, "Subject"),
        snippet: decode_entities(&field("snippet").unwrap_or_default()),
        id,
    })
}

fn decode_part(data: &str) -> Option<String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(data.trim().trim_end_matches('='))
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Collects text/plain and text/html parts, depth first.
fn collect_parts(part: &Value, plain: &mut Vec<String>, html: &mut Vec<String>) {
    let mime = part
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let data = part.pointer("/body/data").and_then(Value::as_str);
    let is_attachment = part
        .get("filename")
        .and_then(Value::as_str)
        .is_some_and(|f| !f.is_empty());
    if let (Some(data), false) = (data, is_attachment) {
        match mime {
            "text/plain" => plain.extend(decode_part(data)),
            "text/html" => html.extend(decode_part(data)),
            _ => {}
        }
    }
    for child in part
        .get("parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        collect_parts(child, plain, html);
    }
}

/// The readable text of a full-format message.
pub fn message_text(message: &Value) -> String {
    let (mut plain, mut html) = (Vec::new(), Vec::new());
    if let Some(payload) = message.get("payload") {
        collect_parts(payload, &mut plain, &mut html);
    }
    // HTML keeps link targets (meeting URLs often live only in href).
    let text = if !html.is_empty() {
        html_to_text(&html.join("\n"))
    } else {
        plain.join("\n")
    };
    truncate(&normalize_whitespace(&text), MAX_BODY_CHARS)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max).collect();
        out.push_str("\n[…truncated]");
        out
    }
}

fn normalize_whitespace(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.trim().to_string()
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Very small HTML → text conversion: drops scripts/styles, turns block
/// elements into line breaks and links into `text (url)`.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    let mut pending_href: Option<String> = None;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let tag = &rest[start + 1..start + end];
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let closing = tag.starts_with('/');
        rest = &rest[start + end + 1..];

        match name.as_str() {
            "script" | "style" | "head" if !closing => {
                let close = format!("</{name}");
                match rest.to_ascii_lowercase().find(&close) {
                    Some(pos) => {
                        rest = &rest[pos..];
                        if let Some(gt) = rest.find('>') {
                            rest = &rest[gt + 1..];
                        }
                    }
                    None => rest = "",
                }
            }
            "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "h4" | "table" => {
                out.push('\n')
            }
            "td" | "th" => out.push(' '),
            "a" if !closing => pending_href = attribute(tag, "href"),
            "a" => {
                if let Some(href) = pending_href.take() {
                    if href.starts_with("http") && !out.contains(&href) {
                        out.push_str(&format!(" ({href})"));
                    }
                }
            }
            _ => {}
        }
    }
    out.push_str(rest);
    decode_entities(&out)
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let pos = lower.find(&format!("{name}="))?;
    let value = &tag[pos + name.len() + 1..];
    let (quote, value) = match value.chars().next()? {
        q @ ('"' | '\'') => (q, &value[1..]),
        _ => (' ', value),
    };
    let end = value.find(quote).unwrap_or(value.len());
    Some(decode_entities(&value[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockServer;
    use serde_json::json;

    fn b64(text: &str) -> String {
        URL_SAFE_NO_PAD.encode(text)
    }

    #[test]
    fn builds_a_narrow_dated_search_query() {
        let query = job_search_query(1_790_000_000_123);
        assert!(query.starts_with("after:1790000000 "));
        assert!(query.contains("-category:promotions"));
        assert!(query.contains("{application applied"));
        assert!(query.contains("\"next steps\""));
    }

    #[test]
    fn extracts_sender_domains() {
        let meta = |from: &str| MessageMeta {
            id: "1".into(),
            thread_id: "t".into(),
            received_at: 0,
            from: from.into(),
            subject: String::new(),
            snippet: String::new(),
        };
        assert_eq!(
            meta("Acme Talent <Jobs@Acme.io>")
                .sender_domain()
                .as_deref(),
            Some("acme.io")
        );
        assert_eq!(
            meta("hr@example.com").sender_domain().as_deref(),
            Some("example.com")
        );
        assert_eq!(meta("nobody").sender_domain(), None);
    }

    #[test]
    fn parses_metadata_and_multipart_bodies() {
        let message = json!({
            "id": "m1", "threadId": "t1", "internalDate": "1790000000000",
            "snippet": "We&#39;d like to invite you",
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [{"name": "From", "value": "Acme <jobs@acme.io>"}, {"name": "subject", "value": "Interview"}],
                "parts": [
                    {"mimeType": "text/plain", "body": {"data": b64("Hello plain")}},
                    {"mimeType": "text/html", "body": {"data": b64(
                        "<html><head><style>p{}</style></head><body><p>Hello&nbsp;Ana,</p>\
                         <p>Join <a href=\"https://meet.example.com/abc\">here</a></p><script>x()</script></body></html>"
                    )}},
                    {"mimeType": "application/pdf", "filename": "cv.pdf", "body": {"attachmentId": "a"}}
                ]
            }
        });
        let meta = parse_meta(&message).unwrap();
        assert_eq!((meta.id.as_str(), meta.thread_id.as_str()), ("m1", "t1"));
        assert_eq!(meta.received_at, 1_790_000_000_000);
        assert_eq!(meta.subject, "Interview");
        assert_eq!(meta.snippet, "We'd like to invite you");

        let text = message_text(&message);
        assert!(text.contains("Hello Ana,"));
        assert!(text.contains("here (https://meet.example.com/abc)"));
        assert!(!text.contains("x()") && !text.contains("p{}"));
    }

    #[test]
    fn truncates_long_bodies() {
        let long = "word ".repeat(10_000);
        let message =
            json!({"id": "m", "payload": {"mimeType": "text/plain", "body": {"data": b64(&long)}}});
        let text = message_text(&message);
        assert!(text.chars().count() <= MAX_BODY_CHARS + 20);
        assert!(text.ends_with("[…truncated]"));
    }

    #[tokio::test]
    async fn searches_with_pagination_and_bearer_token() {
        let server = MockServer::start(|req| {
            if !req.target.starts_with("/gmail/v1/users/me/messages") {
                return None;
            }
            if req.target.contains("pageToken=p2") {
                Some((200, r#"{"messages":[{"id":"c","threadId":"tc"}]}"#.into()))
            } else {
                Some((200, r#"{"messages":[{"id":"a","threadId":"ta"},{"id":"b","threadId":"tb"}],"nextPageToken":"p2"}"#.into()))
            }
        })
        .await;
        let gmail = HttpGmail::new(
            reqwest::Client::new(),
            &format!("{}/gmail/v1", server.base_url),
            "tok".into(),
        );
        let refs = gmail.search("after:1 interview", 50).await.unwrap();
        let ids: Vec<_> = refs.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        let first = &server.requests()[0];
        assert!(first.headers.contains("authorization: bearer tok"));
        assert!(
            first.target.contains("q=after%3A1+interview")
                || first.target.contains("q=after%3A1%20interview")
        );
    }

    #[tokio::test]
    async fn maps_expired_tokens_to_reconnect_errors() {
        let server = MockServer::start(|_| {
            Some((401, r#"{"error":{"message":"Invalid Credentials"}}"#.into()))
        })
        .await;
        let gmail = HttpGmail::new(reqwest::Client::new(), &server.base_url, "tok".into());
        let error = gmail.metadata("x").await.unwrap_err();
        assert!(matches!(error, AppError::Authentication(_)));
    }
}
