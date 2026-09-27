//! Search discovery (§34–§37): finding candidate pages for a request.
//!
//! A search result is not evidence. Discovery only proposes addresses;
//! ReMa then reads the pages it may read, extracts their content and ranks
//! passages ([`gather`], [`super::evidence`]). Providers are interchangeable
//! ([`SearchDiscoveryProvider`]):
//!
//! - **company sites** — the official site of a company the request names
//!   (from Wikidata), its team and careers pages;
//! - **DuckDuckGo** — a public search page that needs no key, used only
//!   when its robots rules let ReMa read it, and never the only route;
//! - **the optional search service** from Advanced (Brave, Tavily,
//!   SearXNG), when one is set up.
//!
//! Job searches go to the Jobs MCP first (its sources are structured
//! postings); discovery adds the open web behind it. Queries carry only the
//! words of the request that name what is sought (§65).

use std::{sync::Arc, time::Instant};

use crate::{
    analytics::normalize,
    llm::BoxFuture,
    rema_mcp::adapters::Ctx,
    retrieval::{backend::Service, listings},
    state::AppState,
};

use super::{
    company, evidence, extract,
    plan::SearchPlan,
    registry,
    research::{self, Finding, SourceKind},
    Scopes,
};

/// Health id of DuckDuckGo.
pub const DUCKDUCKGO: &str = "duckduckgo";
const DUCKDUCKGO_URL: &str = "https://html.duckduckgo.com/html/";
/// Most pages ReMa reads for one request's evidence.
pub const MAX_PAGES: usize = 4;
/// Characters of evidence kept from one page.
const PAGE_EVIDENCE: usize = 900;

/// A page a discovery provider proposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCandidate {
    pub url: String,
    pub title: String,
    /// The provider's snippet: a hint for choosing pages, never evidence.
    pub snippet: Option<String>,
    pub provider: &'static str,
    /// Its place in the provider's results (0 first).
    pub rank: usize,
}

/// One way to find candidate pages.
pub trait SearchDiscoveryProvider: Send + Sync {
    fn name(&self) -> &'static str;

    fn search<'a>(
        &'a self,
        ctx: &'a Ctx<'a>,
        query: &'a str,
        scopes: Scopes,
        limit: usize,
    ) -> BoxFuture<'a, Result<Vec<SearchCandidate>, String>>;
}

/// The names of the discovery providers ReMa has (diagnostics).
pub fn provider_names() -> Vec<&'static str> {
    vec!["jobs-mcp", "company-sites", DUCKDUCKGO, "optional-service"]
}

// ── DuckDuckGo ─────────────────────────────────────────────────────────

/// DuckDuckGo's HTML results page. It needs no key. ReMa reads it like any
/// page — through the safe fetcher, only when the site's robots rules allow
/// it — and reads nothing but result links and titles from it; when the
/// page cannot be read or its layout is not recognised, the provider says
/// so and the other routes carry on.
pub struct DuckDuckGo {
    base: String,
}

impl Default for DuckDuckGo {
    fn default() -> Self {
        // Debug builds can use a local stand-in for end-to-end tests.
        #[cfg(debug_assertions)]
        if let Ok(base) = std::env::var("REMA_DEV_DDG_URL") {
            return Self { base };
        }
        Self {
            base: DUCKDUCKGO_URL.to_string(),
        }
    }
}

impl DuckDuckGo {
    pub fn with_base(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    fn url(&self, query: &str) -> String {
        let mut url = reqwest::Url::parse(&self.base)
            .unwrap_or_else(|_| reqwest::Url::parse(DUCKDUCKGO_URL).expect("valid address"));
        url.query_pairs_mut().append_pair("q", query);
        url.to_string()
    }
}

/// The result links of a DuckDuckGo HTML page: (address, title, snippet).
/// Ads and DuckDuckGo's own pages are left out.
pub fn parse_duckduckgo(html: &str) -> Vec<(String, String, Option<String>)> {
    use std::sync::OnceLock;
    static ANCHOR: OnceLock<regex::Regex> = OnceLock::new();
    static SNIPPET: OnceLock<regex::Regex> = OnceLock::new();
    let anchor = ANCHOR.get_or_init(|| {
        regex::Regex::new(
            r#"(?is)<a\b([^>]*\bclass\s*=\s*["'][^"']*\bresult__a\b[^"']*["'][^>]*)>(.*?)</a>"#,
        )
        .expect("valid pattern")
    });
    let snippet = SNIPPET.get_or_init(|| {
        regex::Regex::new(r#"(?is)<(?:a|div|td)\b[^>]*\bclass\s*=\s*["'][^"']*\bresult__snippet\b[^"']*["'][^>]*>(.*?)</(?:a|div|td)>"#)
            .expect("valid pattern")
    });
    let snippets: Vec<(usize, String)> = snippet
        .captures_iter(html)
        .filter_map(|c| {
            let at = c.get(0)?.start();
            Some((at, crate::analytics::page::html_to_text(&c[1])))
        })
        .collect();
    let mut out: Vec<(String, String, Option<String>)> = Vec::new();
    let anchors: Vec<(usize, String, String)> = anchor
        .captures_iter(html)
        .filter_map(|c| {
            let at = c.get(0)?.end();
            Some((at, c[1].to_string(), c[2].to_string()))
        })
        .collect();
    for (i, (at, attrs, inner)) in anchors.iter().enumerate() {
        let Some(href) = extract::attr(attrs, "href") else {
            continue;
        };
        let Some(url) = result_address(&href) else {
            continue;
        };
        let title = crate::analytics::page::html_to_text(inner);
        let next = anchors.get(i + 1).map_or(usize::MAX, |(a, ..)| *a);
        let snippet = snippets
            .iter()
            .find(|(s, _)| *s > *at && *s < next)
            .map(|(_, text)| text.clone())
            .filter(|t| !t.is_empty());
        if !out.iter().any(|(u, ..)| u == &url) {
            out.push((url, title, snippet));
        }
    }
    out
}

/// The page a DuckDuckGo result points to: its redirect's `uddg` target,
/// or the link itself. Ads and DuckDuckGo's own addresses are not results.
fn result_address(href: &str) -> Option<String> {
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };
    let parsed = reqwest::Url::parse(&absolute).ok()?;
    let host = parsed.host_str().unwrap_or_default().to_lowercase();
    let target = if host.ends_with("duckduckgo.com") {
        if parsed.path().starts_with("/y.js") {
            return None;
        }
        parsed
            .query_pairs()
            .find(|(k, _)| k == "uddg")
            .map(|(_, v)| v.into_owned())?
    } else {
        absolute
    };
    let url = normalize::web_url(&target)?;
    let host = reqwest::Url::parse(&url).ok()?.host_str()?.to_lowercase();
    (!host.ends_with("duckduckgo.com")).then_some(url)
}

impl SearchDiscoveryProvider for DuckDuckGo {
    fn name(&self) -> &'static str {
        "DuckDuckGo"
    }

    fn search<'a>(
        &'a self,
        ctx: &'a Ctx<'a>,
        query: &'a str,
        _scopes: Scopes,
        limit: usize,
    ) -> BoxFuture<'a, Result<Vec<SearchCandidate>, String>> {
        Box::pin(async move {
            let (_, html) = company::page(ctx, &self.url(query))
                .await
                .map_err(|e| format!("DuckDuckGo: {}", e.message()))?;
            let results = parse_duckduckgo(&html);
            if results.is_empty() && !html.to_lowercase().contains("no results") {
                return Err("DuckDuckGo: its results page could not be read".into());
            }
            Ok(results
                .into_iter()
                .take(limit)
                .enumerate()
                .map(|(rank, (url, title, snippet))| SearchCandidate {
                    url,
                    title,
                    snippet,
                    provider: "DuckDuckGo",
                    rank,
                })
                .collect())
        })
    }
}

// ── The optional search service ────────────────────────────────────────

/// The search service from Advanced (optional; never required).
pub struct ServiceDiscovery(pub Arc<Service>);

impl SearchDiscoveryProvider for ServiceDiscovery {
    fn name(&self) -> &'static str {
        "optional search service"
    }

    fn search<'a>(
        &'a self,
        ctx: &'a Ctx<'a>,
        query: &'a str,
        _scopes: Scopes,
        limit: usize,
    ) -> BoxFuture<'a, Result<Vec<SearchCandidate>, String>> {
        Box::pin(async move {
            let hits = self
                .0
                .search(query, None, ctx.cancel)
                .await
                .map_err(|e| format!("{}: {e}", self.0.name()))?;
            Ok(hits
                .into_iter()
                .take(limit)
                .enumerate()
                .map(|(rank, h)| SearchCandidate {
                    url: h.url,
                    title: h.title,
                    snippet: h.snippet,
                    provider: "optional search service",
                    rank,
                })
                .collect())
        })
    }
}

// ── Official company sites ─────────────────────────────────────────────

/// The official site of each company the request names, and the team,
/// leadership and careers pages it links to (direct source adapter).
pub struct CompanySites {
    pub state: AppState,
    pub companies: Vec<String>,
}

impl SearchDiscoveryProvider for CompanySites {
    fn name(&self) -> &'static str {
        "company sites"
    }

    fn search<'a>(
        &'a self,
        ctx: &'a Ctx<'a>,
        _query: &'a str,
        scopes: Scopes,
        limit: usize,
    ) -> BoxFuture<'a, Result<Vec<SearchCandidate>, String>> {
        Box::pin(async move {
            let mut out = Vec::new();
            for name in self.companies.iter().take(3) {
                let Ok(Some(found)) = self.state.career.company(ctx, name).await else {
                    continue;
                };
                let Some(website) = found.website.clone() else {
                    continue;
                };
                out.push(SearchCandidate {
                    url: website.clone(),
                    title: format!("{} (official site)", found.name),
                    snippet: None,
                    provider: "company sites",
                    rank: out.len(),
                });
                if scopes.people || scopes.company {
                    for (url, title, _) in company::team_pages(ctx, &website).await {
                        out.push(SearchCandidate {
                            url,
                            title,
                            snippet: None,
                            provider: "company sites",
                            rank: out.len(),
                        });
                    }
                }
            }
            out.truncate(limit);
            Ok(out)
        })
    }
}

// ── Orchestration ──────────────────────────────────────────────────────

/// What discovery proposed for a request.
#[derive(Debug, Default)]
pub struct Discovered {
    pub candidates: Vec<SearchCandidate>,
    /// Providers that answered.
    pub answered: Vec<&'static str>,
    /// Providers that could not be used, with why.
    pub failed: Vec<String>,
}

/// The query sent to an outside provider: what the plan names — companies,
/// people sought, roles, the place and the kind of source — never the
/// user's sentence, history or files (§65).
pub fn query_for(plan: &SearchPlan) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.extend(plan.companies.iter().take(2).cloned());
    parts.extend(plan.people.iter().take(2).cloned());
    parts.extend(plan.roles.iter().take(2).cloned());
    if plan.scopes.market && !plan.scopes.jobs {
        parts.push("salary".into());
    }
    if plan.scopes.contracts {
        parts.push("freelance contract".into());
    } else if plan.scopes.jobs && plan.companies.is_empty() {
        parts.push("jobs".into());
    }
    if plan.scopes.people && plan.people.is_empty() {
        parts.push("team leadership".into());
    }
    if let Some(place) = &plan.place {
        parts.push(place.label());
    }
    if parts.is_empty() {
        parts.push(plan.text.clone());
    }
    research::search_query(&parts.join(" "))
}

/// Registry sites to search first for the plan's scopes and place, as
/// `site:` restrictions (at most `n`). Professional networks are left out:
/// ReMa does not read their pages.
fn site_scopes(plan: &SearchPlan, n: usize) -> Vec<String> {
    let country = plan.place.as_ref().and_then(|p| p.code.as_deref());
    registry::allowed_domains(plan.scopes, country, &[])
        .into_iter()
        .filter(|d| !matches!(d.as_str(), "linkedin.com" | "xing.com"))
        .take(n)
        .collect()
}

/// Runs the discovery providers for a plan, in the order §36 gives:
/// official company sources first, then the no-key provider (site-scoped to
/// the registry's sources, then open), then the optional service. A
/// provider that fails or rests is skipped; the others go on.
pub async fn discover(
    state: &AppState,
    ctx: &Ctx<'_>,
    plan: &SearchPlan,
    limit: usize,
    company_sites: bool,
) -> Discovered {
    let mut out = Discovered::default();
    let query = query_for(plan);
    let health = &state.career.health;
    let push = |out: &mut Discovered, found: Vec<SearchCandidate>| {
        for candidate in found {
            if listings::is_search_page(&candidate.url) {
                continue;
            }
            let key = normalize::canonical_url(&candidate.url);
            if out
                .candidates
                .iter()
                .any(|c| normalize::canonical_url(&c.url) == key)
            {
                continue;
            }
            out.candidates.push(candidate);
        }
    };
    if company_sites && !plan.companies.is_empty() {
        let sites = CompanySites {
            state: state.clone(),
            companies: plan.companies.clone(),
        };
        if let Ok(found) = sites.search(ctx, &query, plan.scopes, limit).await {
            if !found.is_empty() {
                out.answered.push(sites.name());
            }
            push(&mut out, found);
        }
    }
    let ddg = state.career.duckduckgo();
    if health.state(DUCKDUCKGO) != super::health::HealthState::TemporarilyUnavailable {
        let mut queries: Vec<String> = site_scopes(plan, 2)
            .into_iter()
            .map(|site| format!("site:{site} {query}"))
            .collect();
        queries.push(query.clone());
        let mut ran = false;
        for q in queries {
            if ctx.cancel.is_cancelled() || Instant::now() >= ctx.deadline {
                break;
            }
            match ddg.search(ctx, &q, plan.scopes, limit).await {
                Ok(found) => {
                    ran = true;
                    health.success(DUCKDUCKGO);
                    push(&mut out, found);
                }
                Err(reason) => {
                    health.failure(DUCKDUCKGO, &reason, false);
                    out.failed.push(reason);
                    break;
                }
            }
        }
        if ran {
            out.answered.push(ddg.name());
        }
    }
    if let Some(service) = crate::retrieval::backend::configured(state)
        .await
        .ok()
        .flatten()
        .map(Arc::new)
    {
        let service = ServiceDiscovery(service);
        match service.search(ctx, &query, plan.scopes, limit).await {
            Ok(found) => {
                out.answered.push(service.name());
                push(&mut out, found);
            }
            Err(reason) => out.failed.push(reason),
        }
    }
    out
}

/// Reads the most promising candidates and keeps their best passages as
/// findings (§34): official and registry sources first, then the providers'
/// order. Professional networks are not read. Each finding names the page
/// it was read from; nothing comes from a search snippet.
pub async fn gather(
    ctx: &Ctx<'_>,
    plan: &SearchPlan,
    candidates: &[SearchCandidate],
    company_domains: &[String],
    max_pages: usize,
) -> (Vec<Finding>, Vec<String>) {
    let focus = focus_for(plan);
    let mut ranked: Vec<&SearchCandidate> = candidates
        .iter()
        .filter(|c| {
            !matches!(
                SourceKind::of(&c.url, company_domains),
                SourceKind::Professional
            )
        })
        .collect();
    ranked.sort_by_key(|c| {
        let kind = SourceKind::of(&c.url, company_domains);
        let registry_priority = registry::source_of(&c.url).map_or(3, |s| s.priority);
        (kind, registry_priority, c.rank)
    });
    let mut findings = Vec::new();
    let mut failed = Vec::new();
    for candidate in ranked.into_iter().take(max_pages) {
        if ctx.cancel.is_cancelled() || Instant::now() >= ctx.deadline {
            break;
        }
        match company::page(ctx, &candidate.url).await {
            Ok((final_url, html)) => {
                let page = extract::read_html(&html);
                let chunks = evidence::best(&page, &focus, PAGE_EVIDENCE);
                let text: Vec<String> = chunks.iter().map(evidence::Chunk::line).collect();
                let title = page
                    .title
                    .clone()
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| candidate.title.clone());
                let kind = SourceKind::of(&final_url, company_domains);
                if let Some(mut finding) =
                    Finding::new(&title, &final_url, kind, Some(text.join(" · ")))
                {
                    finding.published_at = page.published.clone();
                    finding.via = format!("ReMa search via {}", candidate.provider);
                    findings.push(finding);
                }
            }
            Err(error) => failed.push(format!("{}: {}", candidate.url, error.message())),
        }
    }
    (findings, failed)
}

/// What the passages of a page are ranked against: the plan's names,
/// roles and the words for what is sought.
pub fn focus_for(plan: &SearchPlan) -> String {
    let mut words: Vec<String> = Vec::new();
    words.extend(plan.companies.iter().cloned());
    words.extend(plan.people.iter().cloned());
    words.extend(plan.roles.iter().cloned());
    if plan.scopes.people {
        words.push("head lead director manager recruiter talent founder".into());
    }
    if plan.scopes.market {
        words.push("salary compensation range".into());
    }
    if plan.scopes.jobs || plan.scopes.contracts {
        words.push("hiring open positions apply".into());
    }
    if let Some(place) = &plan.place {
        words.push(place.label());
    }
    if words.is_empty() {
        words.push(plan.text.clone());
    }
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_duckduckgo_results_without_ads_or_its_own_pages() {
        let html = r#"
            <div class="result results_links results_links_deep result--ad">
              <a class="result__a" href="https://duckduckgo.com/y.js?ad_domain=ads.example&amp;u3=x">Sponsored</a>
            </div>
            <div class="result">
              <h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fnordlicht.example%2Fteam&amp;rut=abc">Team – <b>Nordlicht</b> AI</a></h2>
              <a class="result__snippet" href="//duckduckgo.com/l/?uddg=x">Meet our leadership: Ana Berger, Head of AI.</a>
            </div>
            <div class="result">
              <a class="result__a" href="https://www.karriere.example/jobs/ai-engineer-1">AI Engineer (m/w/d)</a>
            </div>
            <div class="result"><a class="result__a" href="https://duckduckgo.com/?q=more">More results</a></div>"#;
        let results = parse_duckduckgo(html);
        assert_eq!(results.len(), 2, "{results:#?}");
        assert_eq!(results[0].0, "https://nordlicht.example/team");
        assert_eq!(results[0].1, "Team – Nordlicht AI");
        assert_eq!(
            results[0].2.as_deref(),
            Some("Meet our leadership: Ana Berger, Head of AI.")
        );
        assert_eq!(
            results[1].0,
            "https://www.karriere.example/jobs/ai-engineer-1"
        );
        assert_eq!(results[1].2, None);
    }

    #[test]
    fn queries_carry_what_is_sought_not_the_users_words() {
        let plan = crate::career_search::plan::plan(
            "My email is ana@example.com and my CV is attached — who is the current Head of AI \
             at Nordlicht AI?",
        );
        let query = query_for(&plan);
        assert!(!query.contains("ana@example.com"), "{query}");
        assert!(!query.to_lowercase().contains("cv"), "{query}");
        assert!(query.contains("Nordlicht AI"), "{query}");
        let jobs = crate::career_search::plan::plan("Find AI Engineer jobs in Vienna");
        let query = query_for(&jobs);
        assert!(query.contains("AI Engineer"), "{query}");
        assert!(query.contains("Vienna"), "{query}");
        assert!(query.chars().count() <= 120);
    }

    #[test]
    fn registry_sites_scope_the_first_searches_and_networks_are_not_read() {
        let plan = crate::career_search::plan::plan("Find AI Engineer jobs in Vienna");
        let sites = site_scopes(&plan, 3);
        assert_eq!(sites.len(), 3);
        assert!(!sites.iter().any(|s| s == "linkedin.com" || s == "xing.com"));
        assert_eq!(
            sites[0], "karriere.at",
            "the place's boards first: {sites:?}"
        );
    }
}
