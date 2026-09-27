//! Reading public pages as data (B30): visible text by block (headings,
//! list items, paragraphs), safe links, meta descriptions and JSON-LD —
//! never scripts, forms, embedded objects or remote images, and never
//! rendered HTML. Plus the small text measures Business uses to relate an
//! offer's words to a company's pages.

use serde_json::Value;

use crate::analytics::page::decode_entities;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Heading(u8),
    Item,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: Kind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// As written (relative addresses are resolved by the caller).
    pub href: String,
    pub text: String,
}

/// What a page shows a reader.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub title: Option<String>,
    pub site_name: Option<String>,
    pub description: Option<String>,
    pub blocks: Vec<Block>,
    pub links: Vec<Link>,
    pub json_ld: Vec<Value>,
}

impl Page {
    /// The visible text, one block per line.
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .map(|b| match b.kind {
                Kind::Heading(_) => format!("\n{}", b.text),
                Kind::Item => format!("- {}", b.text),
                Kind::Text => b.text.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }
}

/// Elements whose content is never read as text.
const SKIPPED: &[&str] = &[
    "script", "style", "noscript", "svg", "template", "iframe", "object", "embed", "canvas",
    "select", "textarea", "button", "form", "video", "audio", "picture", "map",
];
/// Page chrome: its links are kept (menus lead to features and pricing),
/// its text is not content.
const CHROME: &[&str] = &["nav", "header", "footer", "aside"];
const BLOCKS: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "ul",
    "ol",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "tr",
    "td",
    "th",
    "section",
    "article",
    "main",
    "table",
    "dt",
    "dd",
    "blockquote",
    "figcaption",
];

/// An attribute's value in a tag's inner text (`a href="x" class=y`).
pub fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find(name) {
        let at = from + pos;
        from = at + name.len();
        let before = lower[..at].chars().last();
        if before.is_some_and(|c| !c.is_whitespace()) {
            continue;
        }
        let rest = tag[at + name.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or_default(),
            Some(_) => rest
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .unwrap_or_default(),
            None => "",
        };
        return Some(decode_entities(value).trim().to_string());
    }
    None
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Walker {
    page: Page,
    current: String,
    kind: Kind,
    chrome: usize,
    anchor: Option<(String, String)>,
}

impl Walker {
    fn flush(&mut self) {
        let text = collapse(&decode_entities(&self.current));
        self.current.clear();
        if text.is_empty() || text == "-" || text.chars().count() < 2 {
            return;
        }
        if self.page.blocks.last().is_some_and(|b| b.text == text) {
            return;
        }
        self.page.blocks.push(Block {
            kind: self.kind,
            text,
        });
    }
}

/// Reads a page (at most what the caller fetched; nothing is executed).
pub fn read_html(html: &str) -> Page {
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut w = Walker {
        page: Page::default(),
        current: String::new(),
        kind: Kind::Text,
        chrome: 0,
        anchor: None,
    };
    let mut i = 0;
    while i < html.len() {
        if bytes[i] != b'<' {
            let next = html[i..].find('<').map_or(html.len(), |p| i + p);
            let chunk = &html[i..next];
            if let Some((_, text)) = w.anchor.as_mut() {
                text.push_str(chunk);
            }
            if w.chrome == 0 {
                w.current.push_str(chunk);
            }
            i = next;
            continue;
        }
        if lower[i..].starts_with("<!--") {
            i = lower[i..].find("-->").map_or(html.len(), |p| i + p + 3);
            continue;
        }
        let Some(end) = html[i..].find('>').map(|e| i + e) else {
            break;
        };
        let inner = &html[i + 1..end];
        let tag = &lower[i + 1..end];
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let self_closing = tag.ends_with('/');
        if !closing && name == "script" {
            let close = lower[end..]
                .find("</script")
                .map_or(html.len(), |p| end + p);
            let kind = attr(inner, "type").unwrap_or_default().to_lowercase();
            if kind.contains("ld+json") {
                if let Ok(value) = serde_json::from_str::<Value>(html[end + 1..close].trim()) {
                    w.page.json_ld.push(value);
                }
            }
            i = lower[close..]
                .find('>')
                .map_or(html.len(), |p| close + p + 1);
            continue;
        }
        if !closing && SKIPPED.contains(&name.as_str()) && !self_closing {
            let close = format!("</{name}");
            let at = lower[end..].find(&close).map_or(html.len(), |p| end + p);
            i = lower[at..].find('>').map_or(html.len(), |p| at + p + 1);
            continue;
        }
        match name.as_str() {
            "title" if !closing => {
                let close = lower[end..].find("</title").map_or(html.len(), |p| end + p);
                let title = collapse(&decode_entities(&html[end + 1..close]));
                if !title.is_empty() && w.page.title.is_none() {
                    w.page.title = Some(title);
                }
                i = lower[close..]
                    .find('>')
                    .map_or(html.len(), |p| close + p + 1);
                continue;
            }
            "meta" => {
                let key = attr(inner, "name")
                    .or_else(|| attr(inner, "property"))
                    .unwrap_or_default()
                    .to_lowercase();
                if let Some(content) = attr(inner, "content").map(|c| collapse(&c)) {
                    if !content.is_empty() {
                        match key.as_str() {
                            "description" | "og:description" if w.page.description.is_none() => {
                                w.page.description = Some(content)
                            }
                            "og:site_name" if w.page.site_name.is_none() => {
                                w.page.site_name = Some(content)
                            }
                            _ => {}
                        }
                    }
                }
            }
            "a" if !closing => {
                if let Some(href) = attr(inner, "href") {
                    w.anchor = Some((href, String::new()));
                }
            }
            "a" => {
                if let Some((href, text)) = w.anchor.take() {
                    let text = collapse(&decode_entities(&text));
                    w.page.links.push(Link { href, text });
                }
            }
            n if CHROME.contains(&n) => {
                if closing {
                    w.chrome = w.chrome.saturating_sub(1);
                } else if !self_closing {
                    w.flush();
                    w.chrome += 1;
                }
            }
            n if BLOCKS.contains(&n) => {
                w.flush();
                w.kind = if closing {
                    Kind::Text
                } else if let Some(level) = n
                    .strip_prefix('h')
                    .and_then(|d| d.parse::<u8>().ok())
                    .filter(|_| n.len() == 2)
                {
                    Kind::Heading(level)
                } else if n == "li" {
                    Kind::Item
                } else if n == "dt" || n == "th" {
                    Kind::Heading(6)
                } else {
                    w.kind
                };
            }
            _ => {
                // An inline element starts a word; its end does not ("<b>x</b>.").
                if w.chrome == 0 && !closing {
                    w.current.push(' ');
                }
            }
        }
        i = end + 1;
    }
    w.flush();
    w.page
}

// ── Words ─────────────────────────────────────────────────────────────

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "for", "from", "has", "have", "in",
    "into", "is", "it", "its", "of", "on", "or", "our", "that", "the", "their", "them", "this",
    "to", "we", "with", "you", "your", "all", "any", "more", "most", "not", "no", "so", "than",
    "then", "there", "these", "they", "use", "used", "using", "via", "will", "who", "what", "when",
    "which", "while", "how", "also", "just", "get", "make", "made", "one", "new", "own", "per",
    "der", "die", "das", "und", "oder", "mit", "für", "fur", "von", "den", "dem", "des", "ein",
    "eine", "einer", "einen", "ist", "sind", "wir", "sie", "ihr", "ihre", "zu", "im", "auf", "aus",
    "bei", "nach", "über", "uber", "wie", "auch", "nicht", "mehr",
];

/// Words too common in business text to show a match on their own.
const GENERIC: &[&str] = &[
    "company",
    "companie",
    "business",
    "solution",
    "service",
    "platform",
    "product",
    "team",
    "customer",
    "client",
    "tool",
    "work",
    "help",
    "better",
    "easy",
    "easili",
    "fast",
    "best",
    "lead",
    "leader",
    "world",
    "global",
    "digital",
    "smart",
    "power",
    "simple",
    "modern",
    "way",
    "time",
    "people",
    "process",
    "manag",
    "management",
    "support",
    "system",
    "organization",
    "organisation",
    "unternehmen",
    "lösung",
    "losung",
    "kunden",
];

/// Lower-case word stems of a text ("Integrations" → "integration").
pub fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '+' || c == '#'))
        .filter(|w| !w.is_empty())
        .map(crate::retrieval::intent::stem)
        .collect()
}

/// The words of a text that say something (no stop words or generic
/// business words), in order, without repeats.
pub fn key_terms(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '+' || c == '#'))
    {
        if word.chars().count() < 3 && !matches!(word, "ai" | "ml" | "bi" | "hr" | "it" | "c#") {
            continue;
        }
        if STOPWORDS.contains(&word) || word.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let stem = crate::retrieval::intent::stem(word);
        if GENERIC.contains(&stem.as_str()) || GENERIC.contains(&word) {
            continue;
        }
        if !out.contains(&stem) {
            out.push(stem);
        }
    }
    out
}

/// Sentences (and list lines) of a text.
pub fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut start = 0;
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        for (n, (at, c)) in chars.iter().enumerate() {
            let next_space = chars.get(n + 1).is_none_or(|(_, n)| n.is_whitespace());
            if matches!(c, '.' | '!' | '?') && next_space {
                let s = line[start..at + c.len_utf8()].trim();
                if s.chars().count() >= 3 {
                    out.push(s.to_string());
                }
                start = at + c.len_utf8();
            }
        }
        let rest = line[start..].trim().trim_start_matches("- ");
        if rest.chars().count() >= 3 {
            out.push(rest.to_string());
        }
    }
    out
}

/// The first sentence that contains at least `min` of the terms (as whole
/// word stems), with the terms it contains.
pub fn find_sentence(text: &str, terms: &[String], min: usize) -> Option<(String, Vec<String>)> {
    if terms.is_empty() || min == 0 {
        return None;
    }
    for sentence in sentences(text) {
        let have = words(&sentence);
        let hits: Vec<String> = terms.iter().filter(|t| have.contains(t)).cloned().collect();
        if hits.len() >= min {
            return Some((crate::analytics::normalize::clip(&sentence, 300), hits));
        }
    }
    None
}

/// Whether a whole phrase (case-insensitive, word-bounded) occurs.
pub fn has_phrase(text: &str, phrase: &str) -> bool {
    let hay = format!(" {} ", collapse(&text.to_lowercase()));
    let needle = collapse(&phrase.to_lowercase());
    if needle.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(pos) = hay[from..].find(&needle) {
        let at = from + pos;
        let before = hay[..at].chars().last().unwrap_or(' ');
        let after = hay[at + needle.len()..].chars().next().unwrap_or(' ');
        if !before.is_alphanumeric() && !after.is_alphanumeric() {
            return true;
        }
        from = at + needle.len();
    }
    false
}

/// A claim's text for comparisons (case, spacing and punctuation ignored).
pub fn norm(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A performance or result claim ("save 80% of your time", "10x faster"):
/// marketing, not a proven result (B4).
pub fn is_performance_claim(text: &str) -> bool {
    use std::sync::OnceLock;
    static CELL: OnceLock<regex::Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        regex::Regex::new(
            r"(?i)(\d+\s?%|\b\d+(\.\d+)?\s?x\b|\bfaster\b|\bsave[sd]?\b|\bspar(en|t)\b|\bboost|\bincrease[sd]?\b|\breduce[sd]?\b|\bguarantee|\bproven\b|\b#1\b|\bnumber one\b|\bbest\b|\bleading\b)",
        )
        .expect("valid pattern")
    })
    .is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_blocks_links_meta_and_json_ld_without_active_content() {
        let html = r#"<html><head><title>Support Workspace – Acme</title>
            <meta name="description" content="One inbox for &amp; support teams.">
            <meta property="og:site_name" content="Acme">
            <script>alert('x')</script>
            <script type="application/ld+json">{"@type":"SoftwareApplication","name":"Support Workspace"}</script>
            </head><body>
            <nav><a href="/features">Features</a><a href="/pricing">Pricing</a> Menu text</nav>
            <h1>Answer faster</h1>
            <p>Support Workspace searches your <b>knowledge base</b>.</p>
            <h2>Integrations</h2><ul><li>Zendesk</li><li>Salesforce</li></ul>
            <form><input name="email"><button>Sign up now</button></form>
            <img src="https://tracker.example/pixel.gif">
            <iframe src="https://evil.example"></iframe>
            <footer>© Acme <a href="/legal">Legal</a></footer></body></html>"#;
        let page = read_html(html);
        assert_eq!(page.title.as_deref(), Some("Support Workspace – Acme"));
        assert_eq!(
            page.description.as_deref(),
            Some("One inbox for & support teams.")
        );
        assert_eq!(page.site_name.as_deref(), Some("Acme"));
        assert_eq!(page.json_ld.len(), 1);
        let text = page.text();
        assert!(text.contains("Answer faster"));
        assert!(text.contains("Support Workspace searches your knowledge base."));
        assert!(text.contains("- Zendesk"));
        assert!(!text.contains("alert"), "{text}");
        assert!(!text.contains("Sign up"), "forms are not content: {text}");
        assert!(!text.contains("Menu text"), "chrome is not content: {text}");
        assert!(!text.contains("pixel"), "{text}");
        let hrefs: Vec<&str> = page.links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(hrefs, ["/features", "/pricing", "/legal"]);
        assert_eq!(page.links[0].text, "Features");
        assert!(page
            .blocks
            .iter()
            .any(|b| b.kind == Kind::Heading(2) && b.text == "Integrations"));
    }

    #[test]
    fn terms_sentences_and_phrases() {
        assert_eq!(
            key_terms("Internal knowledge search for support teams"),
            ["internal", "knowledge", "search"]
        );
        let text = "We build tractors. Our service desk uses a knowledge base search.";
        let (sentence, hits) = find_sentence(text, &key_terms("knowledge search"), 2).unwrap();
        assert_eq!(sentence, "Our service desk uses a knowledge base search.");
        assert_eq!(hits.len(), 2);
        assert!(has_phrase("Works with SAP S/4HANA.", "SAP"));
        assert!(!has_phrase("Sapphire dashboards", "SAP"));
        assert!(is_performance_claim("Save 80% of your time"));
        assert!(is_performance_claim("10x faster answers"));
        assert!(!is_performance_claim("Connects to Zendesk"));
        assert_eq!(norm("  Zendesk, Inc.! "), "zendesk inc");
    }
}
