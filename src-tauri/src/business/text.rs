//! Reading public pages as data (B30) with the shared content extractor
//! (`career_search::extract`): visible text by block, safe links, meta
//! descriptions and JSON-LD — never scripts, forms, embedded objects or
//! remote images, and never rendered HTML. Plus the small text measures
//! Business uses to relate an offer's words to a company's pages.

// The page reader is shared with career search and Network Connect.
use crate::career_search::extract::collapse;
pub use crate::career_search::extract::{attr, read_html, Block, Kind, Link, Page};

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
