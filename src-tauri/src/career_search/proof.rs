//! Proof that a model's own web search runs: one live request, judged only
//! by what the provider reported (its search events, the pages they
//! returned, its citations), by whether a returned page really loads and
//! can be read, and by whether the answer agrees with that page — never
//! by what the answer says it did. Each of these is reported on its own.

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

/// A question only a search answers well: something published on an
/// authoritative page, whose answer is a version number the page carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub question: &'static str,
    /// The page the answer should rest on must be on this host.
    pub host: &'static str,
    /// What counts as the answer: a version number (`1.95.0`, `3.14.2`).
    pub answer: Answer,
}

/// The shape of a target's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// A dotted version number with at least two parts.
    Version,
}

/// The default targets, tried in this order; a later one is used only
/// when the earlier one's host cannot be reached from this computer.
/// `REMA_SEARCH_PROOF_TARGET=<host>` picks one of them by host.
pub const TARGETS: &[Target] = &[
    Target {
        question: "What is the newest stable Rust release announced on the official Rust blog \
                   (blog.rust-lang.org)? Give the version number, the date it was announced and \
                   the page it was announced on.",
        host: "blog.rust-lang.org",
        answer: Answer::Version,
    },
    Target {
        question: "What is the newest stable Python 3 release listed on python.org's downloads \
                   page? Give the version number, its release date and the page it is listed on.",
        host: "www.python.org",
        answer: Answer::Version,
    },
    Target {
        question: "What is the newest stable Node.js release on nodejs.org? Give the version \
                   number, its release date and the page it is listed on.",
        host: "nodejs.org",
        answer: Answer::Version,
    },
];

/// The default target's question (kept for callers that only need one).
pub const QUESTION: &str = TARGETS[0].question;

/// The target to use: the one named by `REMA_SEARCH_PROOF_TARGET`, else
/// the first default.
pub fn target() -> &'static Target {
    let wanted = std::env::var("REMA_SEARCH_PROOF_TARGET").unwrap_or_default();
    TARGETS
        .iter()
        .find(|t| !wanted.is_empty() && t.host.eq_ignore_ascii_case(wanted.trim()))
        .unwrap_or(&TARGETS[0])
}

/// Whether the provider is told it must search, or left to decide as it
/// would for any question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The capability check: the provider must search.
    Forced,
    /// The choice check: does the model search on its own for a question
    /// that plainly needs the web?
    ByChoice,
}

/// How a returned page answered when ReMa opened it itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// Loaded; `chars` of readable text, `version` the newest version
    /// number found in it (when the target asks for one).
    Readable {
        url: String,
        chars: usize,
        version: Option<String>,
    },
    /// The site declined an automated reader (a 403, a challenge page):
    /// the page is unverified, not confirmed and not gone.
    Refused { url: String, code: u16 },
}

/// What the provider reported while answering the target's question.
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
    /// The first returned page ReMa tried to open itself, and how it went.
    pub opened: Option<Opened>,
    /// Characters of answer text.
    pub answer_chars: usize,
    /// The version numbers the answer states (target answers of that shape).
    pub answer_versions: Vec<String>,
    /// Whether the answer's version agrees with the page ReMa read: `None`
    /// until a readable page with a version was found.
    pub agrees: Option<bool>,
}

/// One line of evidence, and whether it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

impl SearchProof {
    /// Each kind of evidence on its own: the provider's search, its
    /// citations, whether a returned page could be read, and whether the
    /// answer agrees with it.
    pub fn evidence(&self) -> Vec<Evidence> {
        let searched = Evidence {
            name: "Provider search",
            ok: self.searches > 0 && !self.sources.is_empty(),
            detail: if self.searches == 0 {
                "The provider reported no web search.".into()
            } else if self.sources.is_empty() {
                format!(
                    "The provider reported {} search(es) but returned no pages.",
                    self.searches
                )
            } else {
                format!(
                    "The provider reported {} search(es) returning {} page(s){}.",
                    self.searches,
                    self.sources.len(),
                    if self.failed > 0 {
                        format!(", {} failed", self.failed)
                    } else {
                        String::new()
                    }
                )
            },
        };
        let cited = Evidence {
            name: "Citations",
            ok: self.citations > 0,
            detail: if self.citations > 0 {
                format!("{} citation(s) attached to the answer.", self.citations)
            } else {
                "The answer carries no citations.".into()
            },
        };
        let readable = Evidence {
            name: "Page readable",
            ok: matches!(self.opened, Some(Opened::Readable { .. })),
            detail: match &self.opened {
                Some(Opened::Readable { url, chars, .. }) => {
                    format!("ReMa read {url} ({chars} characters of text).")
                }
                Some(Opened::Refused { url, code }) => format!(
                    "{url} declined an automated reader ({code}): unverified, not confirmed."
                ),
                None if self.sources.is_empty() => "No page to open.".into(),
                None => format!(
                    "None of the first three of {} page(s) could be opened.",
                    self.sources.len()
                ),
            },
        };
        let agrees = Evidence {
            name: "Answer agrees with the page",
            ok: self.agrees == Some(true),
            detail: match (self.agrees, &self.opened) {
                (Some(true), Some(Opened::Readable { version, .. })) => format!(
                    "The answer names {} and the page carries it.",
                    version.as_deref().unwrap_or("the version")
                ),
                (Some(false), Some(Opened::Readable { version, .. })) => format!(
                    "The answer names {} but the page read carries {}.",
                    if self.answer_versions.is_empty() {
                        "no version".to_string()
                    } else {
                        self.answer_versions.join(", ")
                    },
                    version.as_deref().unwrap_or("none")
                ),
                _ => "Not checked: no readable page with the answer on it.".into(),
            },
        };
        vec![searched, cited, readable, agrees]
    }

    /// Whether every line of evidence holds (a fully verified search), or
    /// what is missing.
    pub fn verdict(&self) -> Result<String, String> {
        let evidence = self.evidence();
        let missing: Vec<&Evidence> = evidence.iter().filter(|e| !e.ok).collect();
        if missing.is_empty() {
            return Ok(evidence
                .iter()
                .map(|e| e.detail.as_str())
                .collect::<Vec<_>>()
                .join(" "));
        }
        Err(missing
            .iter()
            .map(|e| format!("{}: {}", e.name, e.detail))
            .collect::<Vec<_>>()
            .join(" "))
    }
}

/// Dotted version numbers in a text (`1.95.0`, `3.14`), in order of
/// appearance, without duplicates.
pub fn versions_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() && (i == 0 || !bytes[i - 1].is_alphanumeric()) {
            let start = i;
            let mut dots = 0;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == '.') {
                if bytes[i] == '.' {
                    if i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_digit() {
                        break;
                    }
                    dots += 1;
                }
                i += 1;
            }
            let candidate: String = bytes[start..i].iter().collect();
            // A version, not a date or a plain number: at least one dot,
            // and no part longer than a year would be odd anyway.
            if dots >= 1
                && candidate.split('.').all(|p| p.len() <= 4)
                && !found.contains(&candidate)
            {
                found.push(candidate);
            }
        } else {
            i += 1;
        }
    }
    found
}

/// The newest of the versions on a page, by numeric parts.
fn newest(versions: &[String]) -> Option<String> {
    versions
        .iter()
        .max_by_key(|v| {
            v.split('.')
                .map(|p| p.parse::<u64>().unwrap_or(0))
                .collect::<Vec<_>>()
        })
        .cloned()
}

#[derive(Default)]
struct Recorder(Mutex<Vec<WebEvent>>);

impl WebObserver for Recorder {
    fn observe(&self, event: WebEvent) {
        self.0.lock().unwrap().push(event);
    }
}

/// Asks `model` the target's question, with the provider's own web search
/// required (`Mode::Forced`) or left to the model (`Mode::ByChoice`), and
/// opens up to three of the pages it returned, preferring the target's
/// host. Bounded: one request, a 2,000-token answer, 30 s of page reads.
pub async fn prove(
    llm: &dyn LanguageModel,
    fetcher: &Fetcher,
    endpoint: &Endpoint,
    model: &str,
    cancel: CancellationToken,
) -> AppResult<SearchProof> {
    prove_with(
        llm,
        fetcher,
        endpoint,
        model,
        target(),
        Mode::Forced,
        cancel,
    )
    .await
}

pub async fn prove_with(
    llm: &dyn LanguageModel,
    fetcher: &Fetcher,
    endpoint: &Endpoint,
    model: &str,
    target: &Target,
    mode: Mode,
    cancel: CancellationToken,
) -> AppResult<SearchProof> {
    let recorder = Arc::new(Recorder::default());
    let request = ChatRequest {
        turns: vec![Turn {
            role: MessageRole::User,
            content: target.question.into(),
        }],
        max_output_tokens: Some(2_000),
        web: Some(WebSearch {
            observer: Some(recorder.clone()),
            required: mode == Mode::Forced,
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
    proof.answer_versions = versions_in(&answer);
    let deadline = Instant::now() + Duration::from_secs(30);
    // The target's own pages first: they carry the answer.
    let mut candidates: Vec<&String> = proof
        .sources
        .iter()
        .filter(|u| host_of(u).eq_ignore_ascii_case(target.host))
        .collect();
    candidates.extend(
        proof
            .sources
            .iter()
            .filter(|u| !host_of(u).eq_ignore_ascii_case(target.host)),
    );
    let mut opened = None;
    for url in candidates.into_iter().take(3) {
        match fetcher
            .get(url, Accept::Html, 256 * 1024, None, deadline, &cancel)
            .await
        {
            Ok(response) => {
                let text = readable_text(response.body.as_deref().unwrap_or_default());
                let version = match target.answer {
                    Answer::Version => newest(&versions_in(&text)),
                };
                opened = Some(Opened::Readable {
                    url: url.clone(),
                    chars: text.chars().count(),
                    version,
                });
                break;
            }
            // Unverified: the site declines automated readers. Kept only
            // when no page can be read at all.
            Err(FetchError::Refused(code)) => {
                if opened.is_none() {
                    opened = Some(Opened::Refused {
                        url: url.clone(),
                        code,
                    });
                }
            }
            Err(_) => continue,
        }
    }
    proof.agrees = match &opened {
        Some(Opened::Readable {
            version: Some(version),
            ..
        }) => Some(proof.answer_versions.iter().any(|v| v == version)),
        _ => None,
    };
    proof.opened = opened;
    Ok(proof)
}

/// A page's text without its markup, enough to find a version number in.
fn readable_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut skip_until: Option<&str> = None;
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if let Some(end) = skip_until {
            if lower[i..].starts_with(end) {
                i += end.len();
                skip_until = None;
            } else {
                i += 1;
            }
            continue;
        }
        if lower[i..].starts_with("<script") {
            skip_until = Some("</script>");
            i += 7;
            continue;
        }
        if lower[i..].starts_with("<style") {
            skip_until = Some("</style>");
            i += 6;
            continue;
        }
        let c = bytes[i] as char;
        if c == '<' {
            in_tag = true;
            text.push(' ');
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag && html.is_char_boundary(i) {
            let ch = html[i..].chars().next().unwrap_or(' ');
            text.push(ch);
            i += ch.len_utf8();
            continue;
        }
        i += 1;
    }
    text
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
        // Pages that could not be opened prove nothing either; each kind
        // of evidence is reported on its own.
        let missing = proof.verdict().unwrap_err();
        assert!(
            missing.contains("Page readable: None of the first three"),
            "{missing}"
        );
        assert!(
            missing.contains("Answer agrees with the page: Not checked"),
            "{missing}"
        );
        assert!(!missing.contains("Provider search:"), "{missing}");
        // A site that declines robots leaves the page unverified.
        let refused = SearchProof {
            opened: Some(Opened::Refused {
                url: "https://www.rust-lang.org/".into(),
                code: 403,
            }),
            ..proof.clone()
        };
        let missing = refused.verdict().unwrap_err();
        assert!(missing.contains("(403): unverified"), "{missing}");
        // A readable page whose version the answer states: fully verified.
        let verified = SearchProof {
            opened: Some(Opened::Readable {
                url: "https://blog.rust-lang.org/2026/09/18/Rust-1.95.0/".into(),
                chars: 5_000,
                version: Some("1.95.0".into()),
            }),
            answer_versions: vec!["1.95.0".into()],
            agrees: Some(true),
            ..proof.clone()
        };
        let evidence = verified.verdict().unwrap();
        assert!(
            evidence.contains("ReMa read https://blog.rust-lang.org"),
            "{evidence}"
        );
        assert!(
            evidence.contains("names 1.95.0 and the page carries it"),
            "{evidence}"
        );
        // The same page, an answer naming another version: not verified.
        let disagrees = SearchProof {
            answer_versions: vec!["1.94.0".into()],
            agrees: Some(false),
            ..verified
        };
        assert!(disagrees
            .verdict()
            .unwrap_err()
            .contains("names 1.94.0 but the page read carries 1.95.0"));
    }

    #[test]
    fn finds_version_numbers_and_readable_text() {
        assert_eq!(
            versions_in("Rust 1.95.0 was released on 2026-09-18; see 1.94 too. Not 2026.09"),
            ["1.95.0", "1.94", "2026.09"]
        );
        assert_eq!(
            newest(&versions_in("1.9.1, 1.10.0 and 1.9.12")).as_deref(),
            Some("1.10.0")
        );
        let page = "<html><head><style>.x{color:red}</style><script>var v='9.9.9'</script></head>\
                    <body><h1>Rust 1.95.0</h1><p>Announced today.</p></body></html>";
        let text = readable_text(page);
        assert!(text.contains("Rust 1.95.0"), "{text}");
        assert!(!text.contains("9.9.9"), "{text}");
        assert_eq!(target().host, "blog.rust-lang.org");
    }
}
