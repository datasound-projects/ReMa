//! Companies (§24, Tier 3): the official domain, public facts and the
//! employer's own job board, from key-free public sources.
//!
//! - **Wikidata** (its public API): the item for the name, its official
//!   website (P856), headquarters (P159), industry (P452), employees
//!   (P1128), founding date (P571), and current CEO (P169), founders (P112)
//!   and chairperson (P488) — statements with an end date are past, and
//!   deprecated ones are ignored. Each fact keeps the item as its source.
//! - **The company's own site**: the careers page it links to, and the
//!   applicant tracking system board that page uses (Greenhouse, Lever,
//!   Ashby, Personio, Recruitee, SmartRecruiters, Workable). Pages are read
//!   once like a browser would, with robots.txt honored.
//! - When neither names a board, the common ATS APIs are asked for a board
//!   under the company's name (a 404 costs nothing and proves nothing).
//!
//! The domain is added to that one request's search sites, never globally.

use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use regex::Regex;
use reqwest::Url;
use serde_json::Value;

use crate::{
    analytics::normalize,
    rema_mcp::{
        adapters::{
            ashby, boards::urlencode, greenhouse, lever, personio, recruitee, smartrecruiters,
            workable, Ctx,
        },
        contract::{ErrorCode, JobRecord, ToolError},
        extract,
        fetch::{Accept, FetchError, PAGE_LIMIT},
    },
};

/// How long a resolved company is reused.
pub const COMPANY_TTL: Duration = Duration::from_secs(24 * 3600);

/// An employer's job board on an applicant tracking system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Board {
    Greenhouse(String),
    Lever { site: String, eu: bool },
    Ashby(String),
    Personio { company: String, domain: String },
    Recruitee(String),
    SmartRecruiters(String),
    Workable(String),
}

impl Board {
    /// The registry id of its source.
    pub fn source(&self) -> &'static str {
        match self {
            Self::Greenhouse(_) => "greenhouse",
            Self::Lever { .. } => "lever",
            Self::Ashby(_) => "ashby",
            Self::Personio { .. } => "personio",
            Self::Recruitee(_) => "recruitee",
            Self::SmartRecruiters(_) => "smartrecruiters",
            Self::Workable(_) => "workable",
        }
    }

    /// "greenhouse:nordlicht".
    pub fn key(&self) -> String {
        let token = match self {
            Self::Greenhouse(t) | Self::Ashby(t) | Self::Recruitee(t) => t.clone(),
            Self::SmartRecruiters(t) | Self::Workable(t) => t.clone(),
            Self::Lever { site, eu } => format!("{site}{}", if *eu { "@eu" } else { "" }),
            Self::Personio { company, .. } => company.clone(),
        };
        format!("{}:{}", self.source(), token.to_lowercase())
    }

    /// Every published job on the board.
    pub async fn list(&self, ctx: &Ctx<'_>) -> Result<Vec<JobRecord>, ToolError> {
        match self {
            Self::Greenhouse(board) => greenhouse::list(ctx, board).await,
            Self::Lever { site, eu } => lever::list(ctx, site, *eu).await,
            Self::Ashby(board) => ashby::list(ctx, board).await,
            Self::Personio { company, domain } => personio::list(ctx, company, domain).await,
            Self::Recruitee(company) => recruitee::list(ctx, company).await,
            Self::SmartRecruiters(company) => smartrecruiters::list(ctx, company).await,
            Self::Workable(account) => workable::list(ctx, account).await,
        }
    }
}

fn token(part: &str) -> Option<String> {
    let ok = !part.is_empty()
        && part.len() <= 80
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| part.to_string())
}

/// The board a link points to (a board, one of its jobs, or its embed).
pub fn board_of(url: &str) -> Option<Board> {
    let parsed = Url::parse(url.trim()).ok()?;
    let host = parsed.host_str()?.trim_start_matches("www.").to_lowercase();
    let segs: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let param = |name: &str| {
        parsed
            .query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    match host.as_str() {
        "boards.greenhouse.io" | "job-boards.greenhouse.io" | "boards-api.greenhouse.io" => {
            match segs.as_slice() {
                ["embed", ..] => param("for").and_then(|b| token(&b)).map(Board::Greenhouse),
                ["v1", "boards", board, ..] => token(board).map(Board::Greenhouse),
                [board, ..] => token(board).map(Board::Greenhouse),
                [] => param("for").and_then(|b| token(&b)).map(Board::Greenhouse),
            }
        }
        "jobs.lever.co" | "jobs.eu.lever.co" => {
            segs.first()
                .and_then(|s| token(s))
                .map(|site| Board::Lever {
                    site,
                    eu: host == "jobs.eu.lever.co",
                })
        }
        "jobs.ashbyhq.com" => segs.first().and_then(|s| token(s)).map(Board::Ashby),
        "jobs.smartrecruiters.com" | "careers.smartrecruiters.com" => segs
            .first()
            .and_then(|s| token(s))
            .map(Board::SmartRecruiters),
        "apply.workable.com" => segs
            .first()
            .filter(|s| **s != "j" && **s != "api")
            .and_then(|s| token(s))
            .map(Board::Workable),
        _ => {
            for domain in ["jobs.personio.de", "jobs.personio.com"] {
                if let Some(company) = host.strip_suffix(&format!(".{domain}")) {
                    return token(company).filter(|c| !c.contains('.')).map(|company| {
                        Board::Personio {
                            company,
                            domain: domain.to_string(),
                        }
                    });
                }
            }
            host.strip_suffix(".recruitee.com")
                .filter(|c| !c.contains('.') && *c != "www" && *c != "api")
                .and_then(token)
                .map(Board::Recruitee)
        }
    }
}

/// A public fact about a company, with where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// "Official website", "Headquarters", "CEO", …
    pub label: String,
    pub value: String,
    pub source_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Company {
    pub name: String,
    /// The Wikidata item ("Q56310962").
    pub item: Option<String>,
    pub description: Option<String>,
    pub website: Option<String>,
    /// Registrable part of the website's host ("bitpanda.com").
    pub domain: Option<String>,
    pub facts: Vec<Fact>,
    /// Current office holders Wikidata lists: (role, name).
    pub people: Vec<(String, String)>,
    pub careers_url: Option<String>,
    pub boards: Vec<Board>,
    /// Its Wikipedia article: (language, title).
    pub wikipedia: Option<(String, String)>,
    /// Its LinkedIn company page, as Wikidata records it (P4264).
    pub linkedin_url: Option<String>,
}

/// "careers.example.co.uk" → "example.co.uk"; "www.bitpanda.com" → "bitpanda.com".
pub fn registrable_domain(host: &str) -> String {
    let host = host.trim().trim_start_matches("www.").to_lowercase();
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() <= 2 {
        return host;
    }
    let second_level = ["co", "com", "ac", "org", "net", "gv", "or", "gov"];
    let n = parts.len();
    let keep = if second_level.contains(&parts[n - 2]) && parts[n - 1].len() == 2 {
        3
    } else {
        2
    };
    parts[n.saturating_sub(keep)..].join(".")
}

fn text(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Words in a Wikidata description that describe an organization.
fn describes_an_organization(description: &str) -> bool {
    const WORDS: &[&str] = &[
        "company",
        "business",
        "enterprise",
        "corporation",
        "manufacturer",
        "startup",
        "start-up",
        "bank",
        "firm",
        "conglomerate",
        "developer",
        "publisher",
        "retailer",
        "software",
        "organization",
        "organisation",
        "provider",
        "operator",
        "group",
        "unternehmen",
        "agency",
        "platform",
        "insurer",
        "consultancy",
        "subsidiary",
        "brand",
        "airline",
        "maker",
        "producer",
        "laboratory",
        "institute",
        "university",
    ];
    let lower = description.to_lowercase();
    WORDS.iter().any(|w| lower.contains(w))
}

/// The statements of a property that still hold (not deprecated, no end
/// date), preferred ones first.
fn current<'a>(entity: &'a Value, property: &str) -> Vec<&'a Value> {
    let mut claims: Vec<&Value> = entity
        .pointer(&format!("/claims/{property}"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|c| c.get("rank").and_then(Value::as_str) != Some("deprecated"))
        .filter(|c| c.pointer("/qualifiers/P582").is_none())
        .collect();
    claims.sort_by_key(|c| c.get("rank").and_then(Value::as_str) != Some("preferred"));
    claims
}

fn item_ids(entity: &Value, property: &str) -> Vec<String> {
    current(entity, property)
        .into_iter()
        .filter_map(|c| text(c, "/mainsnak/datavalue/value/id"))
        .take(4)
        .collect()
}

async fn json(ctx: &Ctx<'_>, url: &str) -> Result<Value, ToolError> {
    let body = ctx.feed_for(url, Accept::Json, COMPANY_TTL).await?;
    serde_json::from_str(&body)
        .map_err(|_| ToolError::new(ErrorCode::ParsingFailed, "Wikidata sent unreadable data"))
}

/// The Wikidata item for a company name, if one clearly matches.
pub async fn wikidata(ctx: &Ctx<'_>, name: &str) -> Result<Option<Company>, ToolError> {
    let api = format!("{}/w/api.php", ctx.apis.wikidata);
    let search = json(
        ctx,
        &format!(
            "{api}?action=wbsearchentities&search={}&language=en&type=item&limit=7&format=json",
            urlencode(name)
        ),
    )
    .await?;
    let wanted = normalize::company_key(name);
    let Some(hit) = search
        .get("search")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|h| {
            let label = text(h, "/label").unwrap_or_default();
            let description = text(h, "/description").unwrap_or_default();
            normalize::company_key(&label) == wanted && describes_an_organization(&description)
        })
    else {
        return Ok(None);
    };
    let Some(id) = text(hit, "/id") else {
        return Ok(None);
    };
    let entities = json(
        ctx,
        &format!("{api}?action=wbgetentities&ids={id}&props=claims|labels|descriptions|sitelinks&sitefilter=enwiki|dewiki&languages=en&format=json"),
    )
    .await?;
    let Some(entity) = entities.pointer(&format!("/entities/{id}")) else {
        return Ok(None);
    };
    let source = format!("https://www.wikidata.org/wiki/{id}");
    let mut company = Company {
        name: text(entity, "/labels/en/value").unwrap_or_else(|| name.to_string()),
        item: Some(id.clone()),
        description: text(entity, "/descriptions/en/value"),
        wikipedia: text(entity, "/sitelinks/enwiki/title")
            .map(|t| ("en".to_string(), t))
            .or_else(|| text(entity, "/sitelinks/dewiki/title").map(|t| ("de".to_string(), t))),
        ..Company::default()
    };
    if let Some(site) = current(entity, "P856")
        .into_iter()
        .find_map(|c| text(c, "/mainsnak/datavalue/value"))
        .and_then(|u| normalize::web_url(&u))
    {
        company.domain = Url::parse(&site)
            .ok()
            .and_then(|u| u.host_str().map(registrable_domain));
        company.facts.push(Fact {
            label: "Official website".into(),
            value: site.clone(),
            source_url: source.clone(),
        });
        company.website = Some(site);
    }
    company.linkedin_url = current(entity, "P4264")
        .into_iter()
        .find_map(|c| text(c, "/mainsnak/datavalue/value"))
        .filter(|id| {
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .map(|id| format!("https://www.linkedin.com/company/{id}"));
    if let Some(amount) = current(entity, "P1128")
        .into_iter()
        .find_map(|c| text(c, "/mainsnak/datavalue/value/amount"))
    {
        company.facts.push(Fact {
            label: "Employees".into(),
            value: amount.trim_start_matches('+').to_string(),
            source_url: source.clone(),
        });
    }
    if let Some(time) = current(entity, "P571")
        .into_iter()
        .find_map(|c| text(c, "/mainsnak/datavalue/value/time"))
    {
        let year: String = time.trim_start_matches('+').chars().take(4).collect();
        if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
            company.facts.push(Fact {
                label: "Founded".into(),
                value: year,
                source_url: source.clone(),
            });
        }
    }
    // Labels of the items the statements point to, in one request.
    let linked: Vec<(&str, Vec<String>)> = vec![
        ("Headquarters", item_ids(entity, "P159")),
        ("Industry", item_ids(entity, "P452")),
        ("CEO", item_ids(entity, "P169")),
        ("Founder", item_ids(entity, "P112")),
        ("Chairperson", item_ids(entity, "P488")),
    ];
    let mut ids: Vec<String> = linked.iter().flat_map(|(_, ids)| ids.clone()).collect();
    ids.dedup();
    ids.truncate(40);
    if !ids.is_empty() {
        let labels = json(
            ctx,
            &format!(
                "{api}?action=wbgetentities&ids={}&props=labels&languages=en&format=json",
                ids.join("|")
            ),
        )
        .await
        .unwrap_or(Value::Null);
        let label = |id: &str| text(&labels, &format!("/entities/{id}/labels/en/value"));
        for (role, ids) in linked {
            for id in ids {
                let Some(value) = label(&id) else { continue };
                match role {
                    "CEO" | "Founder" | "Chairperson" => {
                        company.people.push((role.to_string(), value.clone()))
                    }
                    _ => {}
                }
                company.facts.push(Fact {
                    label: role.to_string(),
                    value,
                    source_url: source.clone(),
                });
            }
        }
    }
    Ok(Some(company))
}

/// The introduction of a Wikipedia article (plain text).
pub async fn wikipedia_intro(ctx: &Ctx<'_>, lang: &str, title: &str) -> Option<String> {
    let url = format!(
        "{}?action=query&prop=extracts&exintro=1&explaintext=1&redirects=1&format=json&titles={}",
        ctx.apis.wikipedia_api(lang),
        urlencode(title)
    );
    let value = json(ctx, &url).await.ok()?;
    let pages = value.pointer("/query/pages")?.as_object()?;
    pages
        .values()
        .find_map(|p| text(p, "/extract"))
        .map(|t| extract::clip(&t, 900))
}

/// The article's address.
pub fn wikipedia_url(lang: &str, title: &str) -> String {
    format!(
        "https://{lang}.wikipedia.org/wiki/{}",
        urlencode(&title.replace(' ', "_")).replace("%2F", "/")
    )
}

/// Team, leadership and about pages linked from a company's homepage:
/// (address, page title, visible text), at most two.
pub async fn team_pages(ctx: &Ctx<'_>, website: &str) -> Vec<(String, String, String)> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let words = re(
        &CELL,
        r"(?i)\b(team|leadership|management|people|about(?:\s+|-)us|who(?:\s+|-)we(?:\s+|-)are|board|founders|über(?:\s+|-)uns|unternehmen|company)\b",
    );
    let Ok((home_url, home)) = page(ctx, website).await else {
        return Vec::new();
    };
    let domain = Url::parse(&home_url)
        .ok()
        .and_then(|u| u.host_str().map(registrable_domain))
        .unwrap_or_default();
    let mut urls: Vec<String> = links(&home, &home_url)
        .into_iter()
        .filter(|(url, label)| {
            words.is_match(label) || words.is_match(&url.replace(['/', '-', '_'], " "))
        })
        .filter(|(url, _)| {
            Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(registrable_domain))
                .is_some_and(|d| d == domain)
        })
        .map(|(url, _)| url)
        .collect();
    urls.dedup();
    let mut out = Vec::new();
    for url in urls.into_iter().take(2) {
        if ctx.cancel.is_cancelled() || Instant::now() >= ctx.deadline {
            break;
        }
        if let Ok((final_url, html)) = page(ctx, &url).await {
            let title =
                crate::analytics::page::page_title(&html).unwrap_or_else(|| final_url.clone());
            out.push((
                final_url,
                title,
                crate::analytics::page::html_to_text(&html),
            ));
        }
    }
    out
}

/// The homepage's title and description (its own words about itself).
pub async fn homepage(ctx: &Ctx<'_>, website: &str) -> Option<(String, String, Option<String>)> {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let meta = re(
        &CELL,
        r#"(?is)<meta\s+[^>]*?(?:name|property)\s*=\s*["'](?:og:)?description["'][^>]*?content\s*=\s*["']([^"']{10,400})["']"#,
    );
    let (url, html) = page(ctx, website).await.ok()?;
    let title = crate::analytics::page::page_title(&html).unwrap_or_else(|| url.clone());
    let description = meta
        .captures(&html)
        .map(|c| extract::clip(&crate::analytics::page::decode_entities(&c[1]), 300));
    Some((url, title, description))
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// Links in a page: (absolute URL, link text).
pub fn links(html: &str, base: &str) -> Vec<(String, String)> {
    static ANCHOR: OnceLock<Regex> = OnceLock::new();
    static SRC: OnceLock<Regex> = OnceLock::new();
    let anchor = re(
        &ANCHOR,
        r#"(?is)<a\b[^>]*?\bhref\s*=\s*["']([^"'#][^"']*)["'][^>]*>(.*?)</a>"#,
    );
    let src = re(
        &SRC,
        r#"(?i)\b(?:src|data-src|action)\s*=\s*["']([^"']+)["']"#,
    );
    let Ok(base) = Url::parse(base) else {
        return Vec::new();
    };
    let absolute = |href: &str| {
        base.join(href.trim())
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"))
            .map(|u| u.to_string())
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for caps in anchor.captures_iter(html).take(2_000) {
        if let Some(url) = absolute(&caps[1]) {
            let label = crate::analytics::page::html_to_text(&caps[2]);
            out.push((url, extract::clip(&label, 80)));
        }
    }
    for caps in src.captures_iter(html).take(500) {
        if let Some(url) = absolute(&caps[1]) {
            out.push((url, String::new()));
        }
    }
    out
}

/// A careers link on a company page.
fn is_careers_link(url: &str, label: &str) -> bool {
    static CELL: OnceLock<Regex> = OnceLock::new();
    let words = re(
        &CELL,
        r"(?i)\b(careers?|jobs|join(?:\s+|-)us|work(?:\s+|-)with(?:\s+|-)us|open\s+positions|karriere|stellen(?:angebote)?|jobs?\s+(?:&|and)\s+careers?|vacancies)\b",
    );
    words.is_match(label) || words.is_match(&url.replace(['/', '-', '_', '.'], " "))
}

/// How long a company page read for research is reused: its homepage
/// gives the description, the team links and the careers links.
const PAGE_TTL: Duration = Duration::from_secs(300);

/// A public page read once like a browser would (robots.txt honored),
/// reused for a few minutes.
pub async fn page(ctx: &Ctx<'_>, url: &str) -> Result<(String, Arc<String>), FetchError> {
    if !ctx.refresh {
        if let Some(read) = ctx.feeds.recent_page(url, PAGE_TTL) {
            return Ok(read);
        }
    }
    if !ctx
        .fetcher
        .robots_allow(url, ctx.deadline, ctx.cancel)
        .await?
    {
        return Err(FetchError::Blocked(
            "the site's robots.txt does not allow reading this page".into(),
        ));
    }
    let response = ctx
        .fetcher
        .get(
            url,
            Accept::Html,
            PAGE_LIMIT,
            None,
            ctx.deadline,
            ctx.cancel,
        )
        .await?;
    let body = Arc::new(response.body.unwrap_or_default());
    ctx.feeds
        .remember_page(url, &response.final_url, body.clone(), PAGE_TTL);
    Ok((response.final_url, body))
}

fn push_board(boards: &mut Vec<Board>, board: Board) {
    if !boards.contains(&board) && boards.len() < 3 {
        boards.push(board);
    }
}

/// The careers page and job boards linked from the company's own site:
/// the homepage, its careers link, else `/careers` and `/jobs`.
pub async fn careers(ctx: &Ctx<'_>, website: &str) -> (Option<String>, Vec<Board>) {
    let mut boards = Vec::new();
    let Ok((home_url, home)) = page(ctx, website).await else {
        return (None, boards);
    };
    let home_links = links(&home, &home_url);
    for (url, _) in &home_links {
        if let Some(board) = board_of(url) {
            push_board(&mut boards, board);
        }
    }
    let domain = Url::parse(&home_url)
        .ok()
        .and_then(|u| u.host_str().map(registrable_domain))
        .unwrap_or_default();
    let mut candidates: Vec<String> = home_links
        .iter()
        .filter(|(url, label)| is_careers_link(url, label))
        .filter(|(url, _)| {
            Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(registrable_domain))
                .is_some_and(|d| d == domain)
        })
        .map(|(url, _)| url.clone())
        .collect();
    candidates.dedup();
    candidates.truncate(2);
    if candidates.is_empty() {
        if let Ok(base) = Url::parse(&home_url) {
            for path in ["/careers", "/jobs"] {
                if let Ok(url) = base.join(path) {
                    candidates.push(url.to_string());
                }
            }
        }
    }
    let mut careers_url = None;
    for url in candidates {
        if ctx.cancel.is_cancelled() || Instant::now() >= ctx.deadline {
            break;
        }
        let Ok((final_url, html)) = page(ctx, &url).await else {
            continue;
        };
        careers_url.get_or_insert(final_url.clone());
        for (link, _) in links(&html, &final_url) {
            if let Some(board) = board_of(&link) {
                push_board(&mut boards, board);
            }
        }
        if !boards.is_empty() {
            break;
        }
    }
    (careers_url, boards)
}

/// Name forms an ATS board token often takes ("Donau Data" → "donaudata",
/// "donau-data").
pub fn slugs(name: &str) -> Vec<String> {
    let cleaned: String = name
        .to_lowercase()
        .replace(['&', '+'], " ")
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-')
        .collect();
    const SUFFIXES: &[&str] = &[
        "gmbh", "ag", "inc", "ltd", "llc", "se", "bv", "sa", "kg", "co",
    ];
    let words: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|w| !SUFFIXES.contains(w))
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let joined: String = words.concat();
    let mut out = vec![joined.clone()];
    if words.len() > 1 {
        out.push(words.join("-"));
    }
    out.retain(|s| s.is_ascii() && s.len() >= 2 && s.len() <= 60);
    out
}

/// Asks the common ATS APIs for a board under the company's name.
pub async fn probe(ctx: &Ctx<'_>, name: &str) -> Vec<Board> {
    let mut candidates: Vec<Board> = Vec::new();
    for slug in slugs(name).into_iter().take(2) {
        candidates.extend([
            Board::Greenhouse(slug.clone()),
            Board::Lever {
                site: slug.clone(),
                eu: false,
            },
            Board::Ashby(slug.clone()),
            Board::Recruitee(slug.clone()),
            Board::Personio {
                company: slug.clone(),
                domain: "jobs.personio.de".into(),
            },
            Board::SmartRecruiters(slug.clone()),
            Board::Workable(slug),
        ]);
    }
    let checks = candidates.into_iter().map(|board| async move {
        match board.list(ctx).await {
            Ok(jobs) if !jobs.is_empty() => Some(board),
            _ => None,
        }
    });
    let mut found = Vec::new();
    for board in futures_util::future::join_all(checks)
        .await
        .into_iter()
        .flatten()
    {
        push_board(&mut found, board);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_job_boards_in_links() {
        assert_eq!(
            board_of("https://boards.greenhouse.io/embed/job_board/js?for=nordlicht"),
            Some(Board::Greenhouse("nordlicht".into()))
        );
        assert_eq!(
            board_of("https://job-boards.greenhouse.io/nordlicht/jobs/4411001"),
            Some(Board::Greenhouse("nordlicht".into()))
        );
        assert_eq!(
            board_of("https://jobs.eu.lever.co/donau"),
            Some(Board::Lever {
                site: "donau".into(),
                eu: true
            })
        );
        assert_eq!(
            board_of("https://wien-ai.jobs.personio.de/?language=de"),
            Some(Board::Personio {
                company: "wien-ai".into(),
                domain: "jobs.personio.de".into()
            })
        );
        assert_eq!(
            board_of("https://wienrobotics.recruitee.com/o/ai-engineer"),
            Some(Board::Recruitee("wienrobotics".into()))
        );
        assert_eq!(board_of("https://apply.workable.com/j/AB12"), None);
        assert_eq!(
            board_of("https://apply.workable.com/donau-data/"),
            Some(Board::Workable("donau-data".into()))
        );
        assert_eq!(board_of("https://example.com/careers"), None);
        assert_eq!(
            Board::Lever {
                site: "Donau".into(),
                eu: true
            }
            .key(),
            "lever:donau@eu"
        );
    }

    #[test]
    fn finds_careers_links_and_domains() {
        let html = r#"<nav><a href="/about">About</a><a href="/karriere/">Karriere</a>
            <a href="https://twitter.com/x">Jobs on Twitter</a>
            <script src="https://boards.greenhouse.io/embed/job_board/js?for=nordlicht"></script></nav>"#;
        let found = links(html, "https://www.nordlicht.example/");
        assert!(found
            .iter()
            .any(|(u, l)| u == "https://www.nordlicht.example/karriere/" && l == "Karriere"));
        assert!(found.iter().any(|(u, _)| board_of(u).is_some()));
        assert!(is_careers_link(
            "https://www.nordlicht.example/karriere/",
            "Karriere"
        ));
        assert!(!is_careers_link(
            "https://www.nordlicht.example/about",
            "About"
        ));
        assert_eq!(registrable_domain("careers.example.co.uk"), "example.co.uk");
        assert_eq!(registrable_domain("www.Bitpanda.com"), "bitpanda.com");
        assert_eq!(slugs("Donau Data GmbH"), ["donaudata", "donau-data"]);
        assert_eq!(slugs("Bitpanda"), ["bitpanda"]);
    }

    #[test]
    fn keeps_only_current_statements() {
        let entity = serde_json::json!({ "claims": { "P169": [
            { "rank": "normal", "mainsnak": { "datavalue": { "value": { "id": "Q1" } } },
              "qualifiers": { "P582": [{}] } },
            { "rank": "normal", "mainsnak": { "datavalue": { "value": { "id": "Q2" } } } },
            { "rank": "deprecated", "mainsnak": { "datavalue": { "value": { "id": "Q3" } } } },
            { "rank": "preferred", "mainsnak": { "datavalue": { "value": { "id": "Q4" } } } }
        ]}});
        assert_eq!(item_ids(&entity, "P169"), ["Q4", "Q2"]);
        assert!(describes_an_organization(
            "Austrian financial technology company"
        ));
        assert!(!describes_an_organization("river in Austria"));
    }
}
