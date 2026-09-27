//! The provider-independent mail interface. Gmail and Outlook map into the
//! same [`MailMessage`]; everything above the connector layer (the job
//! pipeline, the assistant's tools) works only with these types.

use crate::{error::AppResult, llm::BoxFuture, models::connectors::ProviderId};

/// Longest message body passed on (characters).
pub const MAX_BODY_CHARS: usize = 12_000;

/// A mail message, normalized. Bodies are present only when a message was
/// read in full (`get_message`); sync and search return metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MailMessage {
    pub provider: Option<ProviderId>,
    pub account_id: String,
    pub message_id: String,
    /// Gmail thread / Outlook conversation.
    pub thread_id: String,
    pub conversation_id: String,
    /// When the mailbox received it (epoch ms).
    pub received_at: i64,
    /// `Name <address>`.
    pub sender: String,
    pub recipients: Vec<String>,
    pub subject: String,
    pub snippet: String,
    /// Plain text (HTML converted, links kept as `text (url)`), truncated.
    pub body_text: Option<String>,
    /// Gmail labels / Outlook categories and folder markers.
    pub provider_labels: Vec<String>,
    /// Opens the message in Gmail / Outlook on the web.
    pub provider_web_link: Option<String>,
}

impl MailMessage {
    pub fn provider(&self) -> ProviderId {
        self.provider.unwrap_or(ProviderId::Google)
    }

    /// The sender's address, lowercase.
    pub fn sender_address(&self) -> Option<String> {
        let from = self.sender.as_str();
        let address = match (from.rfind('<'), from.rfind('>')) {
            (Some(start), Some(end)) if start < end => &from[start + 1..end],
            _ => from,
        };
        let address = address.trim().to_ascii_lowercase();
        address.contains('@').then_some(address)
    }

    /// `recruiting@company.com` → `company.com`.
    pub fn sender_domain(&self) -> Option<String> {
        self.sender_address()
            .and_then(|a| {
                a.rsplit_once('@')
                    .map(|(_, d)| d.trim_end_matches('>').to_string())
            })
            .filter(|d| d.contains('.'))
    }

    /// The text for extraction: subject and body (or the snippet).
    pub fn text(&self) -> String {
        format!(
            "{}\n{}",
            self.subject,
            self.body_text.as_deref().unwrap_or(&self.snippet)
        )
    }
}

/// New messages since the stored cursor, and the cursor to store once they
/// are recorded.
#[derive(Debug, Clone, Default)]
pub struct SyncBatch {
    /// Metadata only, oldest first; duplicates removed.
    pub messages: Vec<MailMessage>,
    pub cursor: String,
    /// The stored cursor was invalid or expired and a controlled full
    /// resynchronization (bounded by `since`) was done instead.
    pub resynced: bool,
}

/// A mailbox search, built by Rust (never a raw provider query from a model).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailQuery {
    /// Words to match (subject, body, sender).
    pub text: Option<String>,
    /// Sender address or domain.
    pub from: Option<String>,
    /// Received on or after (epoch ms).
    pub after: Option<i64>,
    /// Received before (epoch ms).
    pub before: Option<i64>,
}

/// Read-only mail access.
pub trait MailProvider: Send + Sync {
    fn provider(&self) -> ProviderId;
    /// Messages received since `cursor`. Without a cursor (first sync) or
    /// when it expired: messages received after `since`, and a new cursor.
    fn sync_changes<'a>(
        &'a self,
        cursor: Option<&'a str>,
        since: i64,
    ) -> BoxFuture<'a, AppResult<SyncBatch>>;
    /// Messages received in `[after, before)` (metadata only, oldest
    /// first): a range before what was read so far, when the lookback
    /// grows. Gmail narrows it to likely job mail like a first sync.
    fn list_range<'a>(
        &'a self,
        after: i64,
        before: i64,
    ) -> BoxFuture<'a, AppResult<Vec<MailMessage>>>;
    /// Matching messages, newest first (metadata only).
    fn search<'a>(
        &'a self,
        query: &'a MailQuery,
        limit: usize,
    ) -> BoxFuture<'a, AppResult<Vec<MailMessage>>>;
    /// One message with its body.
    fn get_message<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<MailMessage>>;
    /// The messages of a thread / conversation, oldest first (metadata).
    fn get_thread<'a>(&'a self, thread_id: &'a str) -> BoxFuture<'a, AppResult<Vec<MailMessage>>>;
}

// ── Text helpers shared by providers ────────────────────────────────

pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max).collect();
        out.push_str("\n[…truncated]");
        out
    }
}

pub fn normalize_whitespace(text: &str) -> String {
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

pub fn decode_entities(text: &str) -> String {
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

/// Body text from provider parts: HTML (keeps link targets, which meeting
/// URLs often only have) or plain text, normalized and truncated.
pub fn body_text(plain: &[String], html: &[String]) -> String {
    let text = if !html.is_empty() {
        html_to_text(&html.join("\n"))
    } else {
        plain.join("\n")
    };
    truncate(&normalize_whitespace(&text), MAX_BODY_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sender_addresses_and_domains() {
        let m = |from: &str| MailMessage {
            sender: from.into(),
            ..MailMessage::default()
        };
        assert_eq!(
            m("Acme Talent <Jobs@Acme.io>").sender_domain().as_deref(),
            Some("acme.io")
        );
        assert_eq!(
            m("hr@example.com").sender_address().as_deref(),
            Some("hr@example.com")
        );
        assert_eq!(m("nobody").sender_domain(), None);
    }

    #[test]
    fn converts_html_only_mail_and_keeps_links() {
        let text = body_text(
            &[],
            &["<html><head><style>p{}</style></head><body><p>Hello&nbsp;Ana,</p>\
               <p>Join <a href=\"https://meet.example.com/abc\">here</a></p><script>x()</script></body></html>"
                .into()],
        );
        assert!(text.contains("Hello Ana,"));
        assert!(text.contains("here (https://meet.example.com/abc)"));
        assert!(!text.contains("x()") && !text.contains("p{}"));
    }

    #[test]
    fn truncates_long_bodies() {
        let text = body_text(&["word ".repeat(10_000)], &[]);
        assert!(text.chars().count() <= MAX_BODY_CHARS + 20);
        assert!(text.ends_with("[…truncated]"));
    }
}
