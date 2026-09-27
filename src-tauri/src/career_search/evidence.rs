//! Evidence retrieval (§34, §47): a page is read, cut into chunks along its
//! structure (a heading and what follows it, table rows, list items,
//! paragraphs), and the chunks are ranked against what the request asks
//! with BM25 — cheap, deterministic and local. Only the best chunks reach a
//! model, never the whole page and never a search engine's snippet alone.

use std::collections::HashMap;

use super::extract::{Kind, Page};

/// Longest chunk, in characters (a section is split at block edges).
pub const CHUNK_CHARS: usize = 700;

/// One passage of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// The heading it belongs to ("Leadership"), if any.
    pub heading: Option<String>,
    pub text: String,
    /// Position on the page (for keeping the page's order).
    pub order: usize,
}

impl Chunk {
    /// The chunk as one line of evidence ("Leadership: Ana Berger | Head of AI").
    pub fn line(&self) -> String {
        match &self.heading {
            Some(heading) if !self.text.starts_with(heading.as_str()) => {
                format!("{heading}: {}", self.text)
            }
            _ => self.text.clone(),
        }
    }
}

/// The page's chunks: every heading starts a section; a section's blocks
/// are joined until [`CHUNK_CHARS`], then a new chunk starts under the same
/// heading.
pub fn chunks(page: &Page) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    let mut heading: Option<String> = None;
    let mut current = String::new();
    let push = |heading: &Option<String>, current: &mut String, out: &mut Vec<Chunk>| {
        let text = current.trim().to_string();
        current.clear();
        if text.chars().count() >= 3 {
            let order = out.len();
            out.push(Chunk {
                heading: heading.clone(),
                text,
                order,
            });
        }
    };
    for block in &page.blocks {
        match block.kind {
            Kind::Heading(_) => {
                push(&heading, &mut current, &mut out);
                heading = Some(block.text.clone());
            }
            Kind::Item | Kind::Text | Kind::Row => {
                let piece = match block.kind {
                    Kind::Item => format!("- {}", block.text),
                    _ => block.text.clone(),
                };
                if !current.is_empty()
                    && current.chars().count() + piece.chars().count() + 1 > CHUNK_CHARS
                {
                    push(&heading, &mut current, &mut out);
                }
                if !current.is_empty() {
                    current.push('\n');
                }
                // A single block longer than a chunk is cut at a word.
                if piece.chars().count() > CHUNK_CHARS {
                    let cut: String = piece.chars().take(CHUNK_CHARS).collect();
                    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(a, _)| a);
                    current.push_str(cut);
                    current.push('…');
                } else {
                    current.push_str(&piece);
                }
            }
        }
    }
    push(&heading, &mut current, &mut out);
    out
}

/// Words that carry no meaning for ranking (English and German).
const STOPWORDS: &[&str] = &[
    "a",
    "an",
    "and",
    "are",
    "as",
    "at",
    "be",
    "by",
    "for",
    "from",
    "has",
    "have",
    "in",
    "is",
    "it",
    "its",
    "of",
    "on",
    "or",
    "our",
    "that",
    "the",
    "their",
    "this",
    "to",
    "we",
    "with",
    "you",
    "your",
    "who",
    "what",
    "which",
    "find",
    "current",
    "currently",
    "now",
    "me",
    "der",
    "die",
    "das",
    "und",
    "oder",
    "mit",
    "für",
    "von",
    "den",
    "dem",
    "des",
    "ein",
    "eine",
    "ist",
    "sind",
    "wir",
    "sie",
    "zu",
    "im",
    "auf",
    "bei",
];

/// Lower-case word stems of a text, without stop words.
pub fn terms(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '+' || c == '#'))
        .filter(|w| !w.is_empty() && !STOPWORDS.contains(w))
        .map(crate::retrieval::intent::stem)
        .collect()
}

/// BM25 scores of the chunks for the query terms (k1 = 1.2, b = 0.75).
pub fn bm25(query: &[String], chunks: &[Chunk]) -> Vec<f64> {
    const K1: f64 = 1.2;
    const B: f64 = 0.75;
    let docs: Vec<Vec<String>> = chunks.iter().map(|c| terms(&c.line())).collect();
    let n = docs.len() as f64;
    if docs.is_empty() || query.is_empty() {
        return vec![0.0; docs.len()];
    }
    let average = docs.iter().map(Vec::len).sum::<usize>() as f64 / n;
    let mut df: HashMap<&str, f64> = HashMap::new();
    for term in query {
        let count = docs.iter().filter(|d| d.contains(term)).count() as f64;
        df.insert(term.as_str(), count);
    }
    docs.iter()
        .map(|doc| {
            let len = doc.len() as f64;
            query
                .iter()
                .map(|term| {
                    let tf = doc.iter().filter(|w| *w == term).count() as f64;
                    if tf == 0.0 {
                        return 0.0;
                    }
                    let df = df.get(term.as_str()).copied().unwrap_or(0.0);
                    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                    idf * tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * len / average.max(1.0)))
                })
                .sum()
        })
        .collect()
}

/// The chunks most relevant to the query, within `budget` characters, in
/// the page's order. Chunks that share no term with the query are left out
/// unless nothing matches at all (then the page's first chunks stand in).
pub fn best(page: &Page, query: &str, budget: usize) -> Vec<Chunk> {
    let all = chunks(page);
    let query_terms: Vec<String> = {
        let mut seen: Vec<String> = Vec::new();
        for term in terms(query) {
            if !seen.contains(&term) {
                seen.push(term);
            }
        }
        seen
    };
    let scores = bm25(&query_terms, &all);
    let mut ranked: Vec<(f64, &Chunk)> = scores.into_iter().zip(all.iter()).collect();
    let any_match = ranked.iter().any(|(s, _)| *s > 0.0);
    if any_match {
        ranked.retain(|(s, _)| *s > 0.0);
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.order.cmp(&b.1.order)));
    }
    let mut chosen: Vec<Chunk> = Vec::new();
    let mut used = 0;
    for (_, chunk) in ranked {
        let size = chunk.line().chars().count();
        if used + size > budget && !chosen.is_empty() {
            continue;
        }
        used += size;
        chosen.push(chunk.clone());
        if used >= budget {
            break;
        }
    }
    chosen.sort_by_key(|c| c.order);
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::career_search::extract::read_html;

    fn team_page() -> Page {
        read_html(
            r#"<h1>About Nordlicht AI</h1>
            <p>Nordlicht AI builds language models for European banks. Founded 2019 in Vienna.</p>
            <h2>Leadership</h2>
            <table><tr><td>Ana Berger</td><td>Head of AI</td></tr>
                   <tr><td>Jonas Wolf</td><td>Chief Executive Officer</td></tr></table>
            <h2>People &amp; Culture</h2>
            <p>Our recruiting team: Mira Novak, Talent Acquisition Lead.</p>
            <h2>Office</h2><p>We work from the Wien Mitte campus and remotely.</p>"#,
        )
    }

    #[test]
    fn chunks_follow_the_page_structure() {
        let chunks = chunks(&team_page());
        assert_eq!(chunks.len(), 4, "{chunks:#?}");
        assert_eq!(chunks[1].heading.as_deref(), Some("Leadership"));
        assert_eq!(
            chunks[1].text,
            "Ana Berger | Head of AI\nJonas Wolf | Chief Executive Officer"
        );
        assert_eq!(
            chunks[2].line(),
            "People & Culture: Our recruiting team: Mira Novak, Talent Acquisition Lead."
        );
    }

    #[test]
    fn long_sections_split_at_block_edges() {
        let paragraphs: String = (0..12)
            .map(|i| format!("<p>Paragraph {i} about machine learning platforms and data pipelines for teams.</p>"))
            .collect();
        let page = read_html(&format!("<h2>Engineering</h2>{paragraphs}"));
        let chunks = chunks(&page);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|c| c.text.chars().count() <= CHUNK_CHARS));
        assert!(chunks
            .iter()
            .all(|c| c.heading.as_deref() == Some("Engineering")));
    }

    #[test]
    fn ranking_brings_the_passage_that_answers_the_question() {
        let page = team_page();
        // Chosen by relevance, returned in the page's order.
        let found = best(&page, "Who is the current Head of AI at Nordlicht AI?", 200);
        assert!(
            found
                .iter()
                .any(|c| c.heading.as_deref() == Some("Leadership")),
            "{found:#?}"
        );
        assert!(found.windows(2).all(|w| w[0].order < w[1].order));
        // Room for one passage: the one naming the role.
        let one = best(&page, "Head of AI", 80);
        assert_eq!(one.len(), 1, "{one:#?}");
        assert_eq!(one[0].heading.as_deref(), Some("Leadership"));
        let recruiters = best(&page, "recruiters talent acquisition", 200);
        assert_eq!(recruiters.len(), 1, "{recruiters:#?}");
        assert!(recruiters[0].text.contains("Mira Novak"));
        // Nothing matches: the page's opening stands in, within budget.
        let other = best(&page, "quantum cryptography", 120);
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].order, 0);
    }

    #[test]
    fn bm25_prefers_rarer_terms_and_shorter_passages() {
        let chunks = vec![
            Chunk {
                heading: None,
                text: "AI AI AI team".into(),
                order: 0,
            },
            Chunk {
                heading: None,
                text: "Head of AI Ana Berger".into(),
                order: 1,
            },
        ];
        let scores = bm25(&terms("Head of AI"), &chunks);
        assert!(scores[1] > scores[0], "{scores:?}");
    }
}
