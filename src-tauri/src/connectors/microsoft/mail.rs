//! Outlook mail (Microsoft Graph, delegated `Mail.Read`) behind
//! [`MailProvider`].
//!
//! Synchronization uses Graph delta queries on the Inbox: the first round
//! (bounded by `receivedDateTime ge <since>`) pages through
//! `@odata.nextLink` until Graph returns an `@odata.deltaLink`, which is
//! stored and followed next time to get only changes. An expired or invalid
//! delta token (HTTP 410, `syncStateNotFound` / `resyncRequired`) leads to a
//! controlled resynchronization. Links returned by Graph are followed only
//! on the Graph origin.

use std::collections::HashSet;

use serde_json::Value;

use crate::{
    connectors::{
        api::ApiClient,
        mail::{
            body_text, decode_entities, truncate, MailMessage, MailProvider, MailQuery, SyncBatch,
        },
    },
    error::{AppError, AppResult},
    llm::BoxFuture,
    models::connectors::ProviderId,
};

/// The fields ReMa reads (never attachments).
const SELECT: &str =
    "id,conversationId,receivedDateTime,from,toRecipients,subject,bodyPreview,webLink,categories,isDraft";
/// Most pages followed in one sync (50 messages each).
const MAX_PAGES: usize = 40;
const PAGE_SIZE: &str = "odata.maxpagesize=50";

pub struct OutlookMail {
    pub api: ApiClient,
    pub account_id: String,
}

fn iso(millis: i64) -> String {
    jiff::Timestamp::from_millisecond(millis)
        .map(|t| t.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

fn address(value: &Value) -> Option<String> {
    let email = value.get("emailAddress")?;
    let address = email
        .get("address")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = email
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match (name.is_empty(), address.is_empty()) {
        (_, true) if name.is_empty() => None,
        (true, _) => Some(address.to_string()),
        (false, true) => Some(name.to_string()),
        (false, false) => Some(format!("{name} <{address}>")),
    }
}

/// A Graph message resource → [`MailMessage`].
pub fn parse_message(value: &Value, account_id: &str) -> Option<MailMessage> {
    if value.get("@removed").is_some() {
        return None;
    }
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let id = text("id")?;
    let conversation = text("conversationId").unwrap_or_else(|| id.clone());
    let received_at = text("receivedDateTime")
        .and_then(|t| t.parse::<jiff::Timestamp>().ok())
        .map(|t| t.as_millisecond())
        .unwrap_or_default();
    let body_text = value.get("body").map(|body| {
        let content = body
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match body.get("contentType").and_then(Value::as_str) {
            Some(kind) if kind.eq_ignore_ascii_case("html") => {
                body_text(&[], &[content.to_string()])
            }
            _ => body_text(&[content.to_string()], &[]),
        }
    });
    Some(MailMessage {
        provider: Some(ProviderId::Microsoft),
        account_id: account_id.to_string(),
        message_id: id,
        thread_id: conversation.clone(),
        conversation_id: conversation,
        received_at,
        sender: value.get("from").and_then(address).unwrap_or_default(),
        recipients: value
            .get("toRecipients")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(address)
            .collect(),
        subject: text("subject").unwrap_or_default(),
        snippet: truncate(
            &decode_entities(&text("bodyPreview").unwrap_or_default()),
            300,
        ),
        body_text,
        provider_labels: value
            .get("categories")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        provider_web_link: text("webLink"),
    })
}

/// A KQL `$search` from a [`MailQuery`] built by Rust: words, `from:` and
/// `received>=`, quoted as Graph requires.
pub fn kql(query: &MailQuery) -> String {
    let clean = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '@' | '.' | '-' | '_'))
            .collect::<String>()
            .split_whitespace()
            .filter(|w| !matches!(w.to_ascii_uppercase().as_str(), "AND" | "OR" | "NOT"))
            .take(12)
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut parts = Vec::new();
    if let Some(text) = query.text.as_deref() {
        let text = clean(text);
        if !text.is_empty() {
            parts.push(text);
        }
    }
    if let Some(from) = query.from.as_deref() {
        let from = clean(from);
        if !from.is_empty() {
            parts.push(format!("from:{from}"));
        }
    }
    if let Some(after) = query.after {
        parts.push(format!("received>={}", &iso(after)[..10]));
    }
    if let Some(before) = query.before {
        parts.push(format!("received<{}", &iso(before)[..10]));
    }
    format!("\"{}\"", parts.join(" "))
}

fn resync_required(status: u16, body: &str) -> bool {
    status == 410
        || (status == 400
            && [
                "syncStateNotFound",
                "resyncRequired",
                "SyncStateInvalid",
                "SyncStateNotFound",
            ]
            .iter()
            .any(|code| body.contains(code)))
}

impl OutlookMail {
    /// Follows delta pages from `start` until the delta link. `Ok(None)`: the
    /// delta token is no longer valid.
    async fn delta(&self, start: &str) -> AppResult<Option<SyncBatch>> {
        let mut url = start.to_string();
        let mut messages = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..MAX_PAGES {
            let response = self
                .api
                .send(reqwest::Method::GET, &url, |r| {
                    r.header("Prefer", PAGE_SIZE)
                })
                .await?;
            if resync_required(response.status, &response.body) {
                return Ok(None);
            }
            let body = self.api.expect_ok(response)?;
            for item in body
                .get("value")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if item.get("isDraft").and_then(Value::as_bool) == Some(true) {
                    continue;
                }
                if let Some(message) = parse_message(item, &self.account_id) {
                    if seen.insert(message.message_id.clone()) {
                        messages.push(message);
                    }
                }
            }
            if let Some(delta) = body.get("@odata.deltaLink").and_then(Value::as_str) {
                messages.sort_by_key(|m: &MailMessage| m.received_at);
                return Ok(Some(SyncBatch {
                    messages,
                    cursor: delta.to_string(),
                    resynced: false,
                }));
            }
            match body.get("@odata.nextLink").and_then(Value::as_str) {
                Some(next) => url = next.to_string(),
                None => break,
            }
        }
        Err(AppError::provider(
            "Outlook returned too many changes at once; the next sync continues.",
        ))
    }

    fn initial_url(&self, since: i64) -> String {
        format!(
            "me/mailFolders/inbox/messages/delta?changeType=created&$select={SELECT}&$filter=receivedDateTime+ge+{}",
            iso(since)
        )
    }
}

impl MailProvider for OutlookMail {
    fn provider(&self) -> ProviderId {
        ProviderId::Microsoft
    }

    fn sync_changes<'a>(
        &'a self,
        cursor: Option<&'a str>,
        since: i64,
    ) -> BoxFuture<'a, AppResult<SyncBatch>> {
        Box::pin(async move {
            if let Some(link) = cursor {
                // A stored link that is not Graph's (another site) is never
                // followed; like an expired token, it restarts the sync.
                if self.api.url(link).is_ok() {
                    if let Some(batch) = self.delta(link).await? {
                        return Ok(batch);
                    }
                }
                // Invalid or expired delta token: a bounded full round.
                let mut batch = self.delta(&self.initial_url(since)).await?.ok_or_else(|| {
                    AppError::provider("Outlook could not start a new synchronization.")
                })?;
                batch.resynced = true;
                return Ok(batch);
            }
            self.delta(&self.initial_url(since))
                .await?
                .ok_or_else(|| AppError::provider("Outlook could not start a synchronization."))
        })
    }

    fn list_range<'a>(
        &'a self,
        after: i64,
        before: i64,
    ) -> BoxFuture<'a, AppResult<Vec<MailMessage>>> {
        Box::pin(async move {
            let mut url = format!(
                "me/mailFolders/inbox/messages?$select={SELECT}&$orderby=receivedDateTime&$top=50&$filter=receivedDateTime+ge+{}+and+receivedDateTime+lt+{}",
                iso(after),
                iso(before)
            );
            let mut messages = Vec::new();
            let mut seen = HashSet::new();
            for _ in 0..MAX_PAGES {
                let body = self.api.get(&url, &[]).await?;
                for item in body
                    .get("value")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if item.get("isDraft").and_then(Value::as_bool) == Some(true) {
                        continue;
                    }
                    if let Some(message) = parse_message(item, &self.account_id) {
                        if seen.insert(message.message_id.clone()) {
                            messages.push(message);
                        }
                    }
                }
                match body.get("@odata.nextLink").and_then(Value::as_str) {
                    Some(next) => url = next.to_string(),
                    None => break,
                }
            }
            messages.sort_by_key(|m| m.received_at);
            Ok(messages)
        })
    }

    fn search<'a>(
        &'a self,
        query: &'a MailQuery,
        limit: usize,
    ) -> BoxFuture<'a, AppResult<Vec<MailMessage>>> {
        Box::pin(async move {
            let top = limit.clamp(1, 50).to_string();
            let body = self
                .api
                .get(
                    "me/messages",
                    &[
                        ("$search", kql(query)),
                        ("$top", top),
                        ("$select", SELECT.into()),
                    ],
                )
                .await?;
            let mut messages: Vec<MailMessage> = body
                .get("value")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|m| parse_message(m, &self.account_id))
                .collect();
            messages.sort_by_key(|m| std::cmp::Reverse(m.received_at));
            Ok(messages)
        })
    }

    fn get_message<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<MailMessage>> {
        Box::pin(async move {
            let id: String = id
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '=' | '+' | '/'))
                .collect::<String>()
                .replace('/', "-")
                .replace('+', "_");
            let response = self
                .api
                .send(
                    reqwest::Method::GET,
                    &format!("me/messages/{id}?$select={SELECT},body"),
                    |r| r.header("Prefer", "outlook.body-content-type=\"text\""),
                )
                .await?;
            let body = self.api.expect_ok(response)?;
            parse_message(&body, &self.account_id)
                .ok_or_else(|| AppError::not_found("Outlook message not found"))
        })
    }

    fn get_thread<'a>(&'a self, thread_id: &'a str) -> BoxFuture<'a, AppResult<Vec<MailMessage>>> {
        Box::pin(async move {
            let escaped = thread_id.replace('\'', "''");
            let body = self
                .api
                .get(
                    "me/messages",
                    &[
                        ("$filter", format!("conversationId eq '{escaped}'")),
                        ("$select", SELECT.into()),
                        ("$top", "50".into()),
                    ],
                )
                .await?;
            let mut messages: Vec<MailMessage> = body
                .get("value")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|m| parse_message(m, &self.account_id))
                .collect();
            messages.sort_by_key(|m| m.received_at);
            Ok(messages)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, OnceLock,
    };

    use serde_json::json;

    use super::*;
    use crate::{connectors::api::StaticToken, test_support::MockServer};

    fn outlook(server: &MockServer) -> OutlookMail {
        OutlookMail {
            api: ApiClient::new(
                reqwest::Client::new(),
                Arc::new(StaticToken("tok".into())),
                "Outlook Mail",
                "Outlook Mail",
                &format!("{}/graph/v1.0", server.base_url),
            ),
            account_id: "oid-1".into(),
        }
    }

    fn message(id: &str, received: &str, subject: &str) -> Value {
        json!({
            "id": id, "conversationId": format!("conv-{id}"), "receivedDateTime": received,
            "from": {"emailAddress": {"name": "Acme Talent", "address": "jobs@acme.io"}},
            "toRecipients": [{"emailAddress": {"name": "Ana", "address": "ana@outlook.com"}}],
            "subject": subject, "bodyPreview": "Hello", "webLink": format!("https://outlook.live.com/owa/?ItemID={id}")
        })
    }

    #[test]
    fn maps_graph_messages_into_the_common_model() {
        let mut value = message("m1", "2026-09-20T08:00:00Z", "Interview");
        value["body"] = json!({"contentType": "html", "content": "<p>Join <a href=\"https://teams.example/x\">here</a></p>"});
        let m = parse_message(&value, "oid").unwrap();
        assert_eq!(m.sender, "Acme Talent <jobs@acme.io>");
        assert_eq!(m.sender_domain().as_deref(), Some("acme.io"));
        assert_eq!(m.thread_id, "conv-m1");
        assert_eq!(m.recipients, ["Ana <ana@outlook.com>"]);
        assert!(
            m.body_text
                .unwrap()
                .contains("here (https://teams.example/x)"),
            "HTML-only mail"
        );
        assert!(parse_message(
            &json!({"id": "x", "@removed": {"reason": "deleted"}}),
            "oid"
        )
        .is_none());
    }

    #[test]
    fn builds_quoted_kql_without_operators_from_outside() {
        let q = kql(&MailQuery {
            text: Some("interview\" OR from:evil".into()),
            from: Some("acme.io".into()),
            after: Some(1_790_000_000_000),
            before: None,
        });
        assert!(q.starts_with('"') && q.ends_with('"'));
        assert!(q.contains("from:acme.io") && q.contains("received>=2026-"));
        assert!(!q.contains(" OR ") && !q.contains("from:evil"));
    }

    /// A mock Graph whose links point back at itself.
    async fn graph(
        handler: impl Fn(&str, &str) -> Option<(u16, String)> + Send + Sync + 'static,
    ) -> MockServer {
        let base: Arc<OnceLock<String>> = Arc::default();
        let known = base.clone();
        let server = MockServer::start(move |req| {
            let base = known.get().cloned().unwrap_or_default();
            assert!(req.headers.contains("authorization: bearer tok"));
            handler(&req.target, &base)
        })
        .await;
        base.set(server.base_url.clone()).unwrap();
        server
    }

    #[tokio::test]
    async fn initial_sync_follows_next_links_to_the_delta_link() {
        let server = graph(|t, base| {
            if t.starts_with("/graph/v1.0/me/mailFolders/inbox/messages/delta?changeType=created") {
                return Some((200, json!({
                    "value": [message("a", "2026-09-20T08:00:00Z", "Application received")],
                    "@odata.nextLink": format!("{base}/graph/v1.0/me/mailFolders/inbox/messages/delta?$skiptoken=p2")
                }).to_string()));
            }
            if t.contains("$skiptoken=p2") {
                return Some((200, json!({
                    "value": [message("b", "2026-09-21T08:00:00Z", "Interview"), message("a", "2026-09-20T08:00:00Z", "dup")],
                    "@odata.deltaLink": format!("{base}/graph/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=d1")
                }).to_string()));
            }
            None
        })
        .await;
        let batch = outlook(&server).sync_changes(None, 0).await.unwrap();
        assert!(batch.cursor.ends_with("$deltatoken=d1"));
        let ids: Vec<_> = batch
            .messages
            .iter()
            .map(|m| m.message_id.as_str())
            .collect();
        assert_eq!(ids, ["a", "b"], "oldest first, duplicates removed");
        assert!(!batch.resynced);
        let first = &server.requests()[0];
        assert!(first.headers.contains("prefer: odata.maxpagesize=50"));
    }

    #[tokio::test]
    async fn delta_links_continue_and_expired_tokens_resync() {
        let expired = Arc::new(AtomicUsize::new(0));
        let seen = expired.clone();
        let server = graph(move |t, base| {
            if t.contains("$deltatoken=old") {
                seen.fetch_add(1, Ordering::SeqCst);
                return Some((410, r#"{"error":{"code":"syncStateNotFound","message":"expired"}}"#.into()));
            }
            if t.contains("$deltatoken=good") {
                return Some((200, json!({
                    "value": [message("c", "2026-09-22T08:00:00Z", "Next steps")],
                    "@odata.deltaLink": format!("{base}/graph/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=good2")
                }).to_string()));
            }
            if t.contains("changeType=created") {
                assert!(t.contains("receivedDateTime+ge+2026-09-"), "bounded resync: {t}");
                return Some((200, json!({
                    "value": [],
                    "@odata.deltaLink": format!("{base}/graph/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=fresh")
                }).to_string()));
            }
            None
        })
        .await;
        let mail = outlook(&server);
        let link = |token: &str| {
            format!(
                "{}/graph/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken={token}",
                server.base_url
            )
        };
        let batch = mail.sync_changes(Some(&link("good")), 0).await.unwrap();
        assert_eq!(batch.messages[0].message_id, "c");
        assert!(batch.cursor.ends_with("good2"));
        assert!(!batch.resynced);

        let batch = mail
            .sync_changes(Some(&link("old")), 1_790_300_000_000)
            .await
            .unwrap();
        assert!(batch.resynced);
        assert!(batch.cursor.ends_with("fresh"));
        assert_eq!(expired.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn links_to_other_hosts_are_never_followed() {
        // A page link to another site ends the round without a request.
        let server = graph(|t, _| {
            t.contains("changeType=created").then(|| {
                (200, json!({
                    "value": [],
                    "@odata.nextLink": "https://evil.example.com/graph/v1.0/me/messages/delta?$skiptoken=x"
                }).to_string())
            })
        })
        .await;
        let error = outlook(&server).sync_changes(None, 0).await.unwrap_err();
        assert!(error.to_string().contains("another site"), "{error}");
        assert_eq!(server.requests().len(), 1, "the token never left");

        // A stored cursor to another site is not followed either: like an
        // expired token, it restarts a bounded round at Graph.
        let server = graph(|t, base| {
            assert!(!t.contains("evil"), "{t}");
            t.contains("changeType=created").then(|| {
                (200, json!({
                    "value": [],
                    "@odata.deltaLink": format!("{base}/graph/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=fresh")
                }).to_string())
            })
        })
        .await;
        let batch = outlook(&server)
            .sync_changes(
                Some("https://evil.example.com/graph/v1.0/me/messages/delta?$deltatoken=x"),
                1_790_300_000_000,
            )
            .await
            .unwrap();
        assert!(batch.resynced);
        assert!(batch.cursor.ends_with("fresh"));
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].target.contains("receivedDateTime+ge+2026-09-"));
    }
}
