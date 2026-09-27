//! Relevant document detail. The normalized context holds the stable facts;
//! an excerpt of a CV is added only when a request needs more:
//!
//! - work on a CV itself (review, rewrite, tailor, a cover letter, or a CV
//!   named in the request): that CV's text;
//! - a question about something the summary shortened or left out (a
//!   company, a project, a tool): the few CV passages that mention it, if
//!   they add anything the summary does not already say.
//!
//! Everything else gets no excerpt.

use std::collections::BTreeSet;

use super::{token_set, tokens, ProfileContext, SourceKind};

/// A CV passage: an entry or a list section, with its section heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub source: usize,
    pub heading: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excerpt {
    pub source: usize,
    pub text: String,
}

const WHOLE_DOCUMENT_CHARS: usize = 10_000;
const PASSAGE_CHARS: usize = 1_500;
const MAX_PASSAGES: usize = 3;

const DOCUMENT_WORDS: &[&str] = &["cv", "cvs", "resume", "resumes", "résumé", "lebenslauf"];
const DOCUMENT_TASKS: &[&str] = &[
    "review",
    "rewrite",
    "improve",
    "tailor",
    "adapt",
    "proofread",
    "feedback",
    "critique",
    "translate",
    "shorten",
    "polish",
    "check",
    "rate",
    "summarize",
    "summarise",
    "compare",
    "update",
    "edit",
    "format",
    "restructure",
    "optimize",
    "optimise",
    "ats",
    "overarbeite",
    "überarbeite",
    "verbessere",
    "anpassen",
];
const LETTER_PHRASES: &[&str] = &[
    "cover letter",
    "motivation letter",
    "motivational letter",
    "anschreiben",
    "motivationsschreiben",
];

/// Words that say nothing about which passage is meant.
const STOPWORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "with",
    "what",
    "which",
    "who",
    "how",
    "when",
    "where",
    "why",
    "did",
    "does",
    "was",
    "were",
    "are",
    "have",
    "has",
    "had",
    "can",
    "could",
    "would",
    "should",
    "will",
    "about",
    "from",
    "into",
    "that",
    "this",
    "these",
    "those",
    "there",
    "their",
    "them",
    "they",
    "you",
    "your",
    "yours",
    "our",
    "out",
    "any",
    "all",
    "some",
    "more",
    "most",
    "please",
    "help",
    "tell",
    "show",
    "give",
    "make",
    "write",
    "find",
    "list",
    "like",
    "want",
    "need",
    "get",
    "got",
    "just",
    "also",
    "than",
    "then",
    "very",
    "much",
    "many",
    "mine",
    "its",
    "not",
    "but",
    "been",
    "being",
    "let",
    "lets",
    "job",
    "jobs",
    "role",
    "roles",
    "work",
    "worked",
    "working",
    "experience",
    "skills",
    "skill",
    "company",
    "companies",
    "team",
    "year",
    "years",
    "time",
    "cv",
    "resume",
    "profile",
    "application",
    "applications",
    "position",
    "career",
    "current",
    "new",
    "good",
    "best",
    "based",
    "week",
    "today",
    "tomorrow",
    "plan",
    "next",
    "one",
    "two",
    "use",
    "used",
    "using",
    "know",
    "think",
    "should",
    "could",
    "und",
    "der",
    "die",
    "das",
    "ein",
    "eine",
    "ich",
    "mein",
    "meine",
    "mit",
    "für",
    "von",
    "wie",
    "welche",
    "bitte",
    "habe",
    "bei",
];

fn wants_whole_document(request: &str) -> bool {
    let lowered = request.to_lowercase();
    if LETTER_PHRASES.iter().any(|p| lowered.contains(p)) {
        return true;
    }
    let words = tokens(request);
    words.iter().any(|w| DOCUMENT_WORDS.contains(&w.as_str()))
        && words.iter().any(|w| DOCUMENT_TASKS.contains(&w.as_str()))
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    let end = head
        .rfind('\n')
        .filter(|&i| i > max / 2)
        .unwrap_or(head.len());
    format!("{}\n[… shortened]", head[..end].trim_end())
}

/// One CV as text, section by section.
fn whole(context: &ProfileContext, source: usize) -> Option<Excerpt> {
    let mut text = String::new();
    let mut heading = "";
    for block in context.blocks.iter().filter(|b| b.source == source) {
        if block.heading != heading && !block.heading.is_empty() {
            heading = &block.heading;
            text.push_str(&format!("\n{heading}\n"));
        }
        text.push_str(&block.text);
        text.push('\n');
    }
    let text = text.trim();
    (!text.is_empty()).then(|| Excerpt {
        source,
        text: cut(text, WHOLE_DOCUMENT_CHARS),
    })
}

/// The excerpts a request needs (usually none).
pub fn select(context: &ProfileContext, request: &str) -> Vec<Excerpt> {
    if request.trim().is_empty() || context.blocks.is_empty() {
        return Vec::new();
    }
    let lowered = request.to_lowercase();
    let cvs: Vec<usize> = context
        .sources
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s.kind, SourceKind::PrimaryCv | SourceKind::Cv))
        .map(|(i, _)| i)
        .collect();

    // A CV named in the request, or work on "my CV" (the primary one).
    let named: Vec<usize> = cvs
        .iter()
        .copied()
        .filter(|&i| {
            let name = context.sources[i].name.to_lowercase();
            name.chars().count() >= 4 && lowered.contains(&name)
        })
        .collect();
    if !named.is_empty() || wants_whole_document(request) {
        let chosen = if named.is_empty() {
            cvs.iter()
                .copied()
                .find(|&i| context.blocks.iter().any(|b| b.source == i))
                .into_iter()
                .collect()
        } else {
            named
        };
        return chosen
            .into_iter()
            .take(2)
            .filter_map(|i| whole(context, i))
            .collect();
    }

    // Passages about what the request mentions.
    let significant: BTreeSet<String> = tokens(request)
        .into_iter()
        .filter(|w| w.chars().count() >= 3 && !STOPWORDS.contains(&w.as_str()))
        .collect();
    if significant.is_empty() {
        return Vec::new();
    }
    // Capitalized words are usually names: a company, a school, a product.
    let names: BTreeSet<String> = request
        .split_whitespace()
        .filter(|w| w.chars().next().is_some_and(char::is_uppercase))
        .flat_map(tokens)
        .filter(|w| significant.contains(w))
        .collect();
    let summary = token_set(&context.render());
    let mut scored: Vec<(usize, usize, &super::excerpts::Block)> = context
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(order, block)| {
            let words = token_set(&block.text);
            let hits = significant.iter().filter(|w| words.contains(*w)).count();
            let name_hits = names.iter().filter(|w| words.contains(*w)).count();
            let score = hits + name_hits;
            // Something the summary already says in full adds nothing.
            let adds = words.iter().any(|w| !summary.contains(w));
            (score >= 2 && adds).then_some((score, order, block))
        })
        .collect();
    scored.sort_by_key(|(score, order, _)| (std::cmp::Reverse(*score), *order));
    scored
        .into_iter()
        .take(MAX_PASSAGES)
        .map(|(_, _, block)| Excerpt {
            source: block.source,
            text: cut(
                &if block.heading.is_empty() {
                    block.text.clone()
                } else {
                    format!("{}: {}", block.heading, block.text)
                },
                PASSAGE_CHARS,
            ),
        })
        .collect()
}
