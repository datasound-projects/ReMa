//! One citation model for every route (§48–§49).
//!
//! Sources found by ReMa's own search and sources a provider's search
//! reported (OpenAI URL citations, Anthropic web citations, Codex's opened
//! pages) become the same [`Citation`]: a number in the answer's source
//! list, title, address, site, dates and who found it. The table lives
//! outside the model. What the model writes is checked against it: a
//! reference to a number that is not in the table ("[7]" with five sources,
//! "SOURCE_7") is marked as not found, and a link to a page that is not a
//! source loses its link — neither is ever shown as a valid citation.

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use specta::Type;

use super::research::Finding;
use crate::{analytics::normalize, llm::WebEvent, time::now_ms};

/// What an unknown reference becomes in the answer.
pub const NOT_FOUND: &str = "[source not found]";

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    /// "S3".
    pub id: String,
    /// Its number in the answer's source list (from 1).
    pub number: u32,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub published_at: Option<String>,
    pub retrieved_at: i64,
    /// Who found it: "ReMa sources", "Anthropic web search", …
    pub provider: String,
    /// Words the provider quoted from the page, when it gives them.
    pub quote: Option<String>,
}

/// The valid sources of one answer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CitationTable {
    entries: Vec<Citation>,
}

fn domain_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_string())
        })
        .unwrap_or_default()
}

impl CitationTable {
    /// Adds a source (each address once); its number is its place.
    pub fn push(
        &mut self,
        title: &str,
        url: &str,
        published_at: Option<String>,
        retrieved_at: i64,
        provider: &str,
        quote: Option<String>,
    ) {
        let Some(url) = normalize::web_url(url) else {
            return;
        };
        if self.contains_url(&url) {
            return;
        }
        let number = self.entries.len() as u32 + 1;
        self.entries.push(Citation {
            id: format!("S{number}"),
            number,
            title: if title.trim().is_empty() {
                url.clone()
            } else {
                title.trim().to_string()
            },
            domain: domain_of(&url),
            url,
            published_at,
            retrieved_at,
            provider: provider.to_string(),
            quote,
        });
    }

    /// The research evidence, numbered as the model sees it.
    pub fn from_findings(findings: &[Finding]) -> Self {
        let mut table = Self::default();
        for f in findings {
            let provider = if f.via.is_empty() {
                "ReMa sources"
            } else {
                f.via.as_str()
            };
            table.push(
                &f.title,
                &f.url,
                f.published_at.clone(),
                f.retrieved_at,
                provider,
                None,
            );
        }
        table
    }

    /// Pages a provider's search reported and cited in one answer.
    pub fn from_events(events: &[WebEvent], provider: &str) -> Self {
        let now = now_ms();
        let mut table = Self::default();
        for event in events {
            match event {
                WebEvent::Finished {
                    sources,
                    error: None,
                    ..
                } => {
                    for source in sources {
                        table.push(&source.title, &source.url, None, now, provider, None);
                    }
                }
                WebEvent::Cited {
                    url, title, quote, ..
                } => {
                    if let Some(existing) = table
                        .entries
                        .iter_mut()
                        .find(|c| same_page(&c.url, url) && c.quote.is_none())
                    {
                        existing.quote = quote.clone();
                    } else {
                        table.push(title, url, None, now, provider, quote.clone());
                    }
                }
                _ => {}
            }
        }
        table
    }

    pub fn entries(&self) -> &[Citation] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, number: u32) -> Option<&Citation> {
        number
            .checked_sub(1)
            .and_then(|i| self.entries.get(i as usize))
    }

    pub fn contains_url(&self, url: &str) -> bool {
        self.entries.iter().any(|c| same_page(&c.url, url))
    }
}

fn same_page(a: &str, b: &str) -> bool {
    match (normalize::canonical_url(a), normalize::canonical_url(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// An answer after its references were checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub text: String,
    /// References that named no source ("[7]", "SOURCE_7").
    pub unknown: Vec<String>,
    /// Links to pages that are not sources (the link was removed).
    pub unlinked: Vec<String>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// Checks what the model wrote against the answer's sources.
pub fn check(answer: &str, table: &CitationTable) -> Checked {
    static LINK: OnceLock<Regex> = OnceLock::new();
    static GROUP: OnceLock<Regex> = OnceLock::new();
    static BARE: OnceLock<Regex> = OnceLock::new();
    let mut unknown: Vec<String> = Vec::new();
    let mut unlinked: Vec<String> = Vec::new();

    // Links first: `[text](address)` to a page that is not a source keeps
    // its words and loses the link. Kept links are set aside so their text
    // is not read as a reference.
    let mut kept: Vec<String> = Vec::new();
    let text = re(&LINK, r"\[([^\]\n]*)\]\((https?://[^)\s]+)\)").replace_all(
        answer,
        |c: &regex::Captures| {
            let (label, url) = (&c[1], &c[2]);
            if table.contains_url(url) {
                kept.push(c[0].to_string());
                format!("\u{e000}{}\u{e001}", kept.len() - 1)
            } else {
                unlinked.push(url.to_string());
                label.to_string()
            }
        },
    );

    // "[2]", "[2, 3]", "[S2]", "[SOURCE_2]": numbers the table has stay.
    let number = |token: &str| -> Option<u32> {
        token
            .trim()
            .trim_start_matches("SOURCE_")
            .trim_start_matches("Source ")
            .trim_start_matches('S')
            .parse()
            .ok()
    };
    let text = re(
        &GROUP,
        r"\[((?:SOURCE_|S)?\d{1,3}(?:\s*,\s*(?:SOURCE_|S)?\d{1,3})*)\]",
    )
    .replace_all(&text, |c: &regex::Captures| {
        let known: Vec<String> = c[1]
            .split(',')
            .filter_map(|token| {
                let valid = number(token).filter(|n| table.get(*n).is_some());
                if valid.is_none() {
                    unknown.push(format!("[{}]", token.trim()));
                }
                valid.map(|n| n.to_string())
            })
            .collect();
        if known.is_empty() {
            NOT_FOUND.to_string()
        } else {
            format!("[{}]", known.join(", "))
        }
    });

    // "SOURCE_7" written without brackets.
    let text =
        re(&BARE, r"\bSOURCE_(\d{1,3})\b").replace_all(&text, |c: &regex::Captures| {
            match c[1].parse::<u32>().ok().filter(|n| table.get(*n).is_some()) {
                Some(n) => format!("[{n}]"),
                None => {
                    unknown.push(c[0].to_string());
                    NOT_FOUND.to_string()
                }
            }
        });

    let mut text = text.into_owned();
    for (i, link) in kept.iter().enumerate() {
        text = text.replace(&format!("\u{e000}{i}\u{e001}"), link);
    }
    Checked {
        text,
        unknown,
        unlinked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        career_search::research::SourceKind,
        llm::{WebKind, WebSource},
    };

    fn table() -> CitationTable {
        let mut findings = Vec::new();
        for (title, url) in [
            ("Nordlicht AI — Team", "https://nordlicht.example/team"),
            ("Careers", "https://nordlicht.example/careers"),
        ] {
            findings.push(Finding::new(title, url, SourceKind::Official, None).unwrap());
        }
        CitationTable::from_findings(&findings)
    }

    #[test]
    fn references_the_table_does_not_have_never_look_like_sources() {
        let table = table();
        assert_eq!(table.len(), 2);
        assert_eq!(table.get(2).unwrap().id, "S2");
        assert_eq!(table.get(1).unwrap().domain, "nordlicht.example");
        assert_eq!(table.get(1).unwrap().provider, "ReMa sources");
        let checked = check(
            "Ana Berger leads AI [1]. Hiring in Vienna [2, 7]. Founded 2019 [9]. See SOURCE_2 \
             and SOURCE_7.",
            &table,
        );
        assert_eq!(
            checked.text,
            "Ana Berger leads AI [1]. Hiring in Vienna [2]. Founded 2019 [source not found]. \
             See [2] and [source not found]."
        );
        assert_eq!(checked.unknown, ["[7]", "[9]", "SOURCE_7"]);
    }

    #[test]
    fn links_to_pages_that_are_not_sources_lose_the_link() {
        let checked = check(
            "The [team page](https://nordlicht.example/team?utm_source=x) names her; \
             [this profile](https://evil.example/collect?cv=1) does not count.",
            &table(),
        );
        assert_eq!(
            checked.text,
            "The [team page](https://nordlicht.example/team?utm_source=x) names her; this \
             profile does not count."
        );
        assert_eq!(checked.unlinked, ["https://evil.example/collect?cv=1"]);
        // A link whose text is a number is still a link, not a reference.
        let numbered = check("[3](https://nordlicht.example/careers)", &table());
        assert_eq!(numbered.text, "[3](https://nordlicht.example/careers)");
    }

    #[test]
    fn provider_citations_map_to_the_same_model() {
        let events = vec![
            WebEvent::Finished {
                id: "s".into(),
                kind: WebKind::Search,
                target: "AI jobs Vienna".into(),
                sources: vec![WebSource {
                    title: "AI Engineer — Donau Data".into(),
                    url: "https://jobs.donau.example/ai-1".into(),
                }],
                error: None,
            },
            WebEvent::Cited {
                url: "https://jobs.donau.example/ai-1".into(),
                title: "AI Engineer — Donau Data".into(),
                quote: Some("We are hiring an AI Engineer in Vienna".into()),
                range: None,
            },
            WebEvent::Cited {
                url: "https://karriere.example/job/2".into(),
                title: "ML Engineer".into(),
                quote: None,
                range: Some((10, 40)),
            },
        ];
        let table = CitationTable::from_events(&events, "Anthropic web search");
        assert_eq!(table.len(), 2);
        let first = table.get(1).unwrap();
        assert_eq!(first.provider, "Anthropic web search");
        assert_eq!(
            first.quote.as_deref(),
            Some("We are hiring an AI Engineer in Vienna")
        );
        assert_eq!(table.get(2).unwrap().domain, "karriere.example");
    }
}
