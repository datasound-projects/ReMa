//! Gmail (read-only) behind [`MailProvider`].
//!
//! Synchronization follows Gmail's sync guide: the first sync records the
//! mailbox `historyId` and then reads recent messages with a narrow, dated
//! search; later syncs read only `users.history.list` changes since the
//! stored `historyId`. An expired history id (HTTP 404) leads to a
//! controlled resynchronization bounded by the last successful sync. Only
//! metadata is read while syncing; bodies are read for likely job emails.

use std::collections::HashSet;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

use crate::{
    connectors::{
        api::ApiClient,
        mail::{body_text, decode_entities, MailMessage, MailProvider, MailQuery, SyncBatch},
    },
    error::{AppError, AppResult},
    llm::BoxFuture,
    models::connectors::ProviderId,
};

/// Most messages read by one search.
pub const MAX_SEARCH_RESULTS: usize = 200;
/// More new messages than this in one incremental sync → a bounded resync.
const MAX_INCREMENTAL: usize = 500;
/// Labels whose messages are never job mail for the user to track.
const SKIPPED_LABELS: [&str; 5] = ["SENT", "DRAFT", "SPAM", "TRASH", "CHAT"];

/// Words job-application emails almost always contain (English and German).
/// Narrows the first sync server-side; later syncs see all new mail and
/// filter locally.
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
        "after:{} -in:chats -in:sent -in:drafts -in:spam -in:trash -category:promotions -category:social {{{}}}",
        after_ms.div_euclid(1000),
        JOB_KEYWORDS.join(" ")
    )
}

/// A Gmail search string from a [`MailQuery`] built by Rust.
pub fn search_query(query: &MailQuery) -> String {
    let mut parts = Vec::new();
    if let Some(after) = query.after {
        parts.push(format!("after:{}", after.div_euclid(1000)));
    }
    if let Some(before) = query.before {
        parts.push(format!("before:{}", before.div_euclid(1000)));
    }
    if let Some(from) = query.from.as_deref() {
        parts.push(format!("from:{}", sanitize(from)));
    }
    if let Some(text) = query.text.as_deref() {
        let text = sanitize(text);
        if !text.is_empty() {
            parts.push(text);
        }
    }
    parts.push("-in:chats -in:drafts -in:spam -in:trash".into());
    parts.join(" ")
}

/// Keeps words and simple punctuation: no Gmail operators from outside.
fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '@' | '.' | '-' | '_' | '\''))
        .collect::<String>()
        .split_whitespace()
        .filter(|w| !w.contains(':'))
        .take(12)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Gmail behind the mail interface.
pub struct Gmail {
    pub api: ApiClient,
    pub account_id: String,
    /// For links to the message in Gmail on the web.
    pub email: Option<String>,
}

impl Gmail {
    async fn profile_history_id(&self) -> AppResult<String> {
        let profile = self.api.get("users/me/profile", &[]).await?;
        profile
            .get("historyId")
            .and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            })
            .ok_or_else(|| AppError::provider("Gmail did not return a history id."))
    }

    async fn list(&self, query: &str, limit: usize) -> AppResult<Vec<(String, String)>> {
        let mut refs = Vec::new();
        let mut page: Option<String> = None;
        while refs.len() < limit {
            let mut params = vec![
                ("q", query.to_string()),
                ("maxResults", (limit - refs.len()).min(100).to_string()),
            ];
            if let Some(token) = &page {
                params.push(("pageToken", token.clone()));
            }
            let body = self.api.get("users/me/messages", &params).await?;
            refs.extend(message_refs(&body));
            match body.get("nextPageToken").and_then(Value::as_str) {
                Some(token) if !token.is_empty() => page = Some(token.to_string()),
                _ => break,
            }
        }
        refs.truncate(limit);
        Ok(refs)
    }

    async fn metadata(&self, id: &str) -> AppResult<MailMessage> {
        let body = self
            .api
            .get(
                &format!("users/me/messages/{}", encode(id)),
                &[
                    ("format", "metadata".into()),
                    ("metadataHeaders", "From".into()),
                    ("metadataHeaders", "To".into()),
                    ("metadataHeaders", "Subject".into()),
                ],
            )
            .await?;
        self.parse(&body)
    }

    fn parse(&self, message: &Value) -> AppResult<MailMessage> {
        let mut parsed = parse_message(message)?;
        parsed.account_id = self.account_id.clone();
        parsed.provider_web_link = Some(match &self.email {
            Some(email) => format!(
                "https://mail.google.com/mail/?authuser={}#all/{}",
                encode(email),
                parsed.message_id
            ),
            None => format!("https://mail.google.com/mail/#all/{}", parsed.message_id),
        });
        Ok(parsed)
    }

    /// First sync, or recovery after an expired history id.
    async fn full_sync(&self, since: i64) -> AppResult<SyncBatch> {
        // The history id first: nothing received from now on can be missed.
        let history_id = self.profile_history_id().await?;
        let refs = self
            .list(&job_search_query(since), MAX_SEARCH_RESULTS)
            .await?;
        let mut messages = Vec::new();
        for (id, _) in refs {
            messages.push(self.metadata(&id).await?);
        }
        messages.sort_by_key(|m| m.received_at);
        Ok(SyncBatch {
            messages,
            cursor: history_id,
            resynced: false,
        })
    }
}

fn encode(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
        .collect()
}

impl MailProvider for Gmail {
    fn provider(&self) -> ProviderId {
        ProviderId::Google
    }

    fn sync_changes<'a>(
        &'a self,
        cursor: Option<&'a str>,
        since: i64,
    ) -> BoxFuture<'a, AppResult<SyncBatch>> {
        Box::pin(async move {
            let Some(start) = cursor else {
                return self.full_sync(since).await;
            };
            let mut ids: Vec<String> = Vec::new();
            let mut seen = HashSet::new();
            let mut page: Option<String> = None;
            let mut latest = start.to_string();
            loop {
                let mut params = vec![
                    ("startHistoryId", start.to_string()),
                    ("historyTypes", "messageAdded".into()),
                    ("maxResults", "500".into()),
                ];
                if let Some(token) = &page {
                    params.push(("pageToken", token.clone()));
                }
                let response = self
                    .api
                    .send(reqwest::Method::GET, "users/me/history", |r| {
                        r.query(&params)
                    })
                    .await?;
                if response.status == 404 {
                    // The history id expired: resync from the last success.
                    let mut batch = self.full_sync(since).await?;
                    batch.resynced = true;
                    return Ok(batch);
                }
                let body = self.api.expect_ok(response)?;
                if let Some(id) = body.get("historyId").and_then(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .or_else(|| v.as_u64().map(|n| n.to_string()))
                }) {
                    latest = id;
                }
                for (id, labels) in added_messages(&body) {
                    let skipped = labels.iter().any(|l| SKIPPED_LABELS.contains(&l.as_str()));
                    if !skipped && seen.insert(id.clone()) {
                        ids.push(id);
                    }
                }
                match body.get("nextPageToken").and_then(Value::as_str) {
                    Some(token) if !token.is_empty() => page = Some(token.to_string()),
                    _ => break,
                }
                if ids.len() > MAX_INCREMENTAL {
                    let mut batch = self.full_sync(since).await?;
                    batch.resynced = true;
                    return Ok(batch);
                }
            }
            let mut messages = Vec::new();
            for id in ids {
                match self.metadata(&id).await {
                    Ok(message) => messages.push(message),
                    // Deleted between the change and now.
                    Err(AppError::NotFound(_)) => {}
                    Err(error) => return Err(error),
                }
            }
            messages.sort_by_key(|m| m.received_at);
            Ok(SyncBatch {
                messages,
                cursor: latest,
                resynced: false,
            })
        })
    }

    fn search<'a>(
        &'a self,
        query: &'a MailQuery,
        limit: usize,
    ) -> BoxFuture<'a, AppResult<Vec<MailMessage>>> {
        Box::pin(async move {
            let refs = self.list(&search_query(query), limit.min(50)).await?;
            let mut out = Vec::new();
            for (id, _) in refs {
                out.push(self.metadata(&id).await?);
            }
            out.sort_by_key(|m| std::cmp::Reverse(m.received_at));
            Ok(out)
        })
    }

    fn get_message<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<MailMessage>> {
        Box::pin(async move {
            let body = self
                .api
                .get(
                    &format!("users/me/messages/{}", encode(id)),
                    &[("format", "full".into())],
                )
                .await?;
            let mut message = self.parse(&body)?;
            message.body_text = Some(message_text(&body));
            Ok(message)
        })
    }

    fn get_thread<'a>(&'a self, thread_id: &'a str) -> BoxFuture<'a, AppResult<Vec<MailMessage>>> {
        Box::pin(async move {
            let body = self
                .api
                .get(
                    &format!("users/me/threads/{}", encode(thread_id)),
                    &[
                        ("format", "metadata".into()),
                        ("metadataHeaders", "From".into()),
                        ("metadataHeaders", "To".into()),
                        ("metadataHeaders", "Subject".into()),
                    ],
                )
                .await?;
            let mut messages: Vec<MailMessage> = body
                .get("messages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|m| self.parse(m))
                .collect::<AppResult<_>>()?;
            messages.sort_by_key(|m| m.received_at);
            Ok(messages)
        })
    }
}

pub fn message_refs(body: &Value) -> Vec<(String, String)> {
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some((
                m.get("id")?.as_str()?.to_string(),
                m.get("threadId")?.as_str()?.to_string(),
            ))
        })
        .collect()
}

/// `(message id, labels)` added according to a history page.
pub fn added_messages(body: &Value) -> Vec<(String, Vec<String>)> {
    body.get("history")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|h| {
            h.get("messagesAdded")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|added| {
            let message = added.get("message")?;
            let labels = message
                .get("labelIds")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            Some((message.get("id")?.as_str()?.to_string(), labels))
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

/// A Gmail message resource (metadata or full) → [`MailMessage`].
pub fn parse_message(message: &Value) -> AppResult<MailMessage> {
    let field = |name: &str| {
        message
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let id = field("id").ok_or_else(|| AppError::provider("Gmail message without id"))?;
    let thread_id = field("threadId").unwrap_or_else(|| id.clone());
    let to = header(message, "To");
    Ok(MailMessage {
        provider: Some(ProviderId::Google),
        account_id: String::new(),
        conversation_id: thread_id.clone(),
        thread_id,
        received_at: field("internalDate")
            .and_then(|d| d.parse().ok())
            .unwrap_or_default(),
        sender: header(message, "From"),
        recipients: to
            .split(',')
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty())
            .collect(),
        subject: header(message, "Subject"),
        snippet: decode_entities(&field("snippet").unwrap_or_default()),
        body_text: None,
        provider_labels: message
            .get("labelIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        provider_web_link: None,
        message_id: id,
    })
}

fn decode_part(data: &str) -> Option<String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(data.trim().trim_end_matches('='))
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Collects text/plain and text/html parts, depth first (no attachments).
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

/// The readable text of a full-format message (HTML-only mail included).
pub fn message_text(message: &Value) -> String {
    let (mut plain, mut html) = (Vec::new(), Vec::new());
    if let Some(payload) = message.get("payload") {
        collect_parts(payload, &mut plain, &mut html);
    }
    body_text(&plain, &html)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{connectors::api::StaticToken, test_support::MockServer};
    use serde_json::json;

    fn b64(text: &str) -> String {
        URL_SAFE_NO_PAD.encode(text)
    }

    fn gmail(server: &MockServer) -> Gmail {
        Gmail {
            api: ApiClient::new(
                reqwest::Client::new(),
                Arc::new(StaticToken("tok".into())),
                "Gmail",
                "Gmail",
                &format!("{}/gmail/v1", server.base_url),
            ),
            account_id: "sub-1".into(),
            email: Some("me@example.com".into()),
        }
    }

    fn meta(id: &str, thread: &str, date: i64, subject: &str) -> String {
        json!({
            "id": id, "threadId": thread, "internalDate": date.to_string(), "snippet": "hi",
            "labelIds": ["INBOX"],
            "payload": {"headers": [{"name": "From", "value": "Acme <jobs@acme.io>"},
                                    {"name": "Subject", "value": subject}]}
        })
        .to_string()
    }

    #[test]
    fn builds_narrow_dated_queries_without_foreign_operators() {
        let query = job_search_query(1_790_000_000_123);
        assert!(query.starts_with("after:1790000000 "));
        assert!(query.contains("-category:promotions"));
        let q = search_query(&MailQuery {
            text: Some("interview in:anywhere label:x".into()),
            from: Some("acme.io".into()),
            after: Some(1_000),
            before: None,
        });
        assert!(q.contains("from:acme.io") && q.contains("interview"));
        assert!(!q.contains("in:anywhere") && !q.contains("label:x"));
    }

    #[test]
    fn parses_metadata_and_multipart_bodies() {
        let message = json!({
            "id": "m1", "threadId": "t1", "internalDate": "1790000000000",
            "snippet": "We&#39;d like to invite you",
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [{"name": "From", "value": "Acme <jobs@acme.io>"}, {"name": "subject", "value": "Interview"},
                            {"name": "To", "value": "a@x.io, b@x.io"}],
                "parts": [
                    {"mimeType": "text/plain", "body": {"data": b64("Hello plain")}},
                    {"mimeType": "text/html", "body": {"data": b64(
                        "<p>Hello&nbsp;Ana,</p><p>Join <a href=\"https://meet.example.com/abc\">here</a></p>"
                    )}},
                    {"mimeType": "application/pdf", "filename": "cv.pdf", "body": {"attachmentId": "a"}}
                ]
            }
        });
        let parsed = parse_message(&message).unwrap();
        assert_eq!(
            (parsed.message_id.as_str(), parsed.thread_id.as_str()),
            ("m1", "t1")
        );
        assert_eq!(parsed.received_at, 1_790_000_000_000);
        assert_eq!(parsed.snippet, "We'd like to invite you");
        assert_eq!(parsed.recipients, ["a@x.io", "b@x.io"]);
        let text = message_text(&message);
        assert!(text.contains("Hello Ana,"));
        assert!(text.contains("here (https://meet.example.com/abc)"));
    }

    #[tokio::test]
    async fn first_sync_records_the_history_id_then_reads_recent_mail() {
        let server = MockServer::start(|req| {
            let t = req.target.as_str();
            if t.starts_with("/gmail/v1/users/me/profile") {
                return Some((
                    200,
                    r#"{"emailAddress":"me@example.com","historyId":"900"}"#.into(),
                ));
            }
            if t.starts_with("/gmail/v1/users/me/messages?") {
                assert!(
                    t.contains("after%3A"),
                    "the first sync is bounded by date: {t}"
                );
                return Some((
                    200,
                    r#"{"messages":[{"id":"b","threadId":"tb"},{"id":"a","threadId":"ta"}]}"#
                        .into(),
                ));
            }
            if t.starts_with("/gmail/v1/users/me/messages/a?") {
                return Some((200, meta("a", "ta", 1_000, "Application received")));
            }
            if t.starts_with("/gmail/v1/users/me/messages/b?") {
                return Some((200, meta("b", "tb", 2_000, "Interview")));
            }
            None
        })
        .await;
        let batch = gmail(&server).sync_changes(None, 0).await.unwrap();
        assert_eq!(batch.cursor, "900");
        let ids: Vec<_> = batch
            .messages
            .iter()
            .map(|m| m.message_id.as_str())
            .collect();
        assert_eq!(ids, ["a", "b"], "oldest first");
        assert!(batch.messages[0]
            .provider_web_link
            .as_deref()
            .unwrap()
            .contains("#all/a"));
        // Profile before the listing: nothing can slip between them.
        let targets: Vec<String> = server.requests().iter().map(|r| r.target.clone()).collect();
        assert!(targets[0].contains("/profile"));
    }

    #[tokio::test]
    async fn later_syncs_read_history_only_and_skip_duplicates_and_sent_mail() {
        let server = MockServer::start(|req| {
            let t = req.target.as_str();
            if t.starts_with("/gmail/v1/users/me/history?") {
                assert!(t.contains("startHistoryId=900"));
                return Some((200, json!({
                    "historyId": "950",
                    "history": [
                        {"messagesAdded": [{"message": {"id": "c", "threadId": "tc", "labelIds": ["INBOX"]}}]},
                        {"messagesAdded": [{"message": {"id": "c", "threadId": "tc", "labelIds": ["INBOX"]}}]},
                        {"messagesAdded": [{"message": {"id": "s", "threadId": "ts", "labelIds": ["SENT"]}}]}
                    ]
                }).to_string()));
            }
            if t.starts_with("/gmail/v1/users/me/messages/c?") {
                return Some((200, meta("c", "tc", 3_000, "Next steps")));
            }
            None
        })
        .await;
        let batch = gmail(&server).sync_changes(Some("900"), 0).await.unwrap();
        assert_eq!(batch.cursor, "950");
        assert_eq!(batch.messages.len(), 1, "duplicate and sent mail skipped");
        assert!(!batch.resynced);
        assert!(
            !server
                .requests()
                .iter()
                .any(|r| r.target.contains("/messages?")),
            "no mailbox listing on incremental syncs"
        );
    }

    #[tokio::test]
    async fn an_expired_history_id_leads_to_a_bounded_resync() {
        let server = MockServer::start(|req| {
            let t = req.target.as_str();
            if t.starts_with("/gmail/v1/users/me/history?") {
                return Some((
                    404,
                    r#"{"error":{"message":"Requested entity was not found."}}"#.into(),
                ));
            }
            if t.starts_with("/gmail/v1/users/me/profile") {
                return Some((200, r#"{"historyId":"2000"}"#.into()));
            }
            if t.starts_with("/gmail/v1/users/me/messages?") {
                assert!(
                    t.contains("after%3A1790000000"),
                    "bounded by the last success: {t}"
                );
                return Some((200, r#"{"messages":[]}"#.into()));
            }
            None
        })
        .await;
        let batch = gmail(&server)
            .sync_changes(Some("1"), 1_790_000_000_000)
            .await
            .unwrap();
        assert!(batch.resynced);
        assert_eq!(batch.cursor, "2000");
    }
}
