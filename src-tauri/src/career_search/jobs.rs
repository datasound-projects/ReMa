//! ReMa's own job sources (§17–§19): the no-key discovery surface of the
//! Jobs MCP. It needs no search service and no API key, so a job search
//! always has a route:
//!
//! 1. employer boards on applicant tracking systems — boards ReMa has seen
//!    before (their jobs near the requested place first) and the boards of
//!    companies the request names, found through the company's own site;
//! 2. public job boards with documented APIs (Arbeitnow, The Muse,
//!    Remotive, the Hacker News hiring thread), chosen by region and
//!    remote work.
//!
//! Every source is asked concurrently, within the request's deadline. A
//! source that fails does not fail the search (§34); one that keeps failing
//! is rested for a while (§36, [`super::health`]).

use std::collections::HashSet;

use futures_util::future::join_all;
use reqwest::Url;

use super::{
    company::{self, Board},
    health::HealthBook,
    plan::Place,
};
use crate::{
    analytics::normalize,
    rema_mcp::{
        adapters::{boards, Ctx},
        contract::{ErrorCode, JobRecord, ToolError},
        filter,
    },
    state::AppState,
};

/// Most employer boards listed for one request.
const MAX_BOARDS: usize = 10;
/// Most jobs kept from all sources of one request (the engine reads and
/// filters them; list responses are already complete records).
const MAX_RECORDS: usize = 150;

/// What a job search asks ReMa's sources for.
#[derive(Debug, Clone, Default)]
pub struct JobAsk {
    /// The role and its close variants.
    pub roles: Vec<String>,
    pub place: Option<Place>,
    pub remote: bool,
    /// Companies named in the request.
    pub companies: Vec<String>,
}

/// One source's answer.
#[derive(Debug, Default)]
pub struct Listed {
    /// (source id that found it, the job).
    pub records: Vec<(String, JobRecord)>,
    /// Sources that answered.
    pub searched: Vec<String>,
    /// Sources that failed: (id, code, message).
    pub failed: Vec<(String, ErrorCode, String)>,
    /// Sources resting after repeated failures.
    pub resting: Vec<String>,
}

/// A source to ask.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    Board {
        board: Board,
        company: Option<String>,
    },
    Arbeitnow,
    TheMuse,
    Remotive,
    HackerNews,
}

impl Source {
    fn id(&self) -> String {
        match self {
            Self::Board { board, .. } => board.key(),
            Self::Arbeitnow => boards::ARBEITNOW.into(),
            Self::TheMuse => boards::THEMUSE.into(),
            Self::Remotive => boards::REMOTIVE.into(),
            Self::HackerNews => boards::HN.into(),
        }
    }

    /// The registry id the health book and coverage use.
    fn source(&self) -> String {
        match self {
            Self::Board { board, .. } => board.source().into(),
            other => other.id(),
        }
    }
}

/// A board ReMa has seen jobs on, with where those jobs were.
#[derive(Debug, Clone)]
pub struct KnownBoard {
    pub board: Board,
    pub company: Option<String>,
    pub cities: Vec<String>,
    pub countries: Vec<String>,
}

/// Boards from jobs ReMa found before (Analytics jobs and the Jobs MCP
/// store), most recent first.
pub fn known_boards(state: &AppState) -> Vec<KnownBoard> {
    // (address, employer, city, country)
    type Row = (String, Option<String>, Option<String>, Option<String>);
    let rows: Vec<Row> = state
        .db
        .call(|c| {
            let mut out = Vec::new();
            let mut stmt = c.prepare(
                "SELECT source_url, company, city, country FROM jobs \
                 WHERE source_url IS NOT NULL ORDER BY last_seen DESC LIMIT 2000",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            for row in rows {
                out.push(row?);
            }
            let mut stmt = c.prepare(
                "SELECT key FROM rema_mcp_job_keys WHERE key LIKE 'url:%' \
                 ORDER BY rowid DESC LIMIT 2000",
            )?;
            let keys = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for key in keys {
                let key = key?;
                out.push((format!("https://{}", &key[4..]), None, None, None));
            }
            Ok(out)
        })
        .unwrap_or_default();
    let mut boards: Vec<KnownBoard> = Vec::new();
    for (url, company, city, country) in rows {
        let Some(board) = company::board_of(&url) else {
            continue;
        };
        match boards.iter_mut().find(|b| b.board == board) {
            Some(known) => {
                if known.company.is_none() {
                    known.company = company;
                }
                known
                    .cities
                    .extend(city.filter(|c| !known.cities.contains(c)));
                known
                    .countries
                    .extend(country.filter(|c| !known.countries.contains(c)));
            }
            None => boards.push(KnownBoard {
                board,
                company,
                cities: city.into_iter().collect(),
                countries: country.into_iter().collect(),
            }),
        }
    }
    boards
}

/// Known boards for a request: those with jobs in the requested city, then
/// in its country, then the rest (unless a place rules them out).
fn pick_known(known: Vec<KnownBoard>, ask: &JobAsk) -> Vec<(Board, Option<String>)> {
    let city = ask.place.as_ref().and_then(|p| p.city.clone());
    let country = ask.place.as_ref().and_then(|p| p.country.clone());
    let mut scored: Vec<(u8, KnownBoard)> = known
        .into_iter()
        .map(|b| {
            let score = if city.as_ref().is_some_and(|c| b.cities.contains(c)) {
                0
            } else if country.as_ref().is_some_and(|c| b.countries.contains(c)) {
                1
            } else if b.cities.is_empty() && b.countries.is_empty() {
                2
            } else {
                3
            };
            (score, b)
        })
        .collect();
    scored.sort_by_key(|(score, _)| *score);
    let place_asked = ask.place.is_some() && !ask.remote;
    scored
        .into_iter()
        // Boards known only elsewhere cannot help a place-bound search.
        .filter(|(score, _)| !place_asked || *score < 3)
        .map(|(_, b)| (b.board, b.company))
        .take(MAX_BOARDS)
        .collect()
}

/// Which public boards fit a request.
fn public_boards(ask: &JobAsk) -> Vec<Source> {
    let country = ask.place.as_ref().and_then(|p| p.code.clone());
    let mut out = Vec::new();
    // Arbeitnow lists mostly German-speaking Europe.
    if ask.remote || country.is_none() || matches!(country.as_deref(), Some("DE" | "AT" | "CH")) {
        out.push(Source::Arbeitnow);
    }
    // The Muse filters by "City, Country".
    if ask.remote || ask.place.as_ref().is_some_and(|p| p.city.is_some()) {
        out.push(Source::TheMuse);
    }
    if ask.remote || ask.place.is_none() {
        out.push(Source::Remotive);
    }
    out.push(Source::HackerNews);
    out
}

/// Whether a listed job is about one of the roles (the engine applies the
/// request's filters afterwards; this only keeps list volume sensible).
fn about_roles(record: &JobRecord, roles: &[String]) -> bool {
    roles.is_empty() || roles.iter().any(|role| filter::on_topic(record, role))
}

/// The main words of a role for a board's own search ("AI Engineer" → "AI
/// Engineer"; the first two variants at most).
fn search_terms(roles: &[String]) -> String {
    roles.first().cloned().unwrap_or_default()
}

fn rate_limited(error: &ToolError) -> bool {
    error.code == ErrorCode::RateLimited
}

async fn ask_source(
    ctx: &Ctx<'_>,
    source: &Source,
    ask: &JobAsk,
) -> Result<Vec<JobRecord>, ToolError> {
    let place = ask.place.as_ref();
    match source {
        Source::Board { board, company } => {
            let mut jobs = board.list(ctx).await?;
            for job in &mut jobs {
                if job.employer.name.is_none() {
                    job.employer.name.clone_from(company);
                }
            }
            Ok(jobs)
        }
        Source::Arbeitnow => boards::arbeitnow(ctx, 2).await,
        Source::TheMuse => {
            let (city, country) = if ask.remote && place.is_none() {
                (Some("Flexible / Remote"), None)
            } else {
                (
                    place.and_then(|p| p.city.as_deref()),
                    place.and_then(|p| p.country.as_deref()),
                )
            };
            if city == Some("Flexible / Remote") {
                return boards::themuse(ctx, None, None, 1).await;
            }
            boards::themuse(ctx, city, country, 2).await
        }
        Source::Remotive => boards::remotive(ctx, &search_terms(&ask.roles)).await,
        Source::HackerNews => {
            let terms = search_terms(&ask.roles);
            boards::hn_hiring(ctx, &terms).await
        }
    }
}

/// Asks every fitting source; never fails as a whole.
pub async fn list(state: &AppState, ctx: &Ctx<'_>, ask: &JobAsk, health: &HealthBook) -> Listed {
    let mut sources: Vec<Source> = Vec::new();
    // Named companies: their own boards first (Tier 2/3).
    for name in ask.companies.iter().take(3) {
        for board in company_boards(state, ctx, name).await {
            let source = Source::Board {
                board,
                company: Some(name.clone()),
            };
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
    }
    for (board, company) in pick_known(known_boards(state), ask) {
        let source = Source::Board { board, company };
        if !sources.iter().any(|s| s.id() == source.id()) {
            sources.push(source);
        }
    }
    sources.extend(public_boards(ask));

    let mut listed = Listed::default();
    let (open, resting): (Vec<Source>, Vec<Source>) =
        sources.into_iter().partition(|s| health.allow(&s.id()));
    listed.resting = resting.iter().map(Source::id).collect();
    let answers = join_all(
        open.iter()
            .map(|source| async move { (source, ask_source(ctx, source, ask).await) }),
    )
    .await;
    let mut seen: HashSet<String> = HashSet::new();
    for (source, answer) in answers {
        let id = source.id();
        match answer {
            Ok(jobs) => {
                health.success(&id);
                listed.searched.push(source.source());
                for job in jobs.into_iter().filter(|j| about_roles(j, &ask.roles)) {
                    let key = job
                        .source_ids
                        .first()
                        .map(|s| format!("{}:{}", s.source, s.id.to_lowercase()))
                        .or_else(|| {
                            job.links
                                .canonical_url
                                .as_deref()
                                .and_then(normalize::canonical_url)
                        })
                        .unwrap_or_else(|| job.title.clone());
                    if seen.insert(key) && listed.records.len() < MAX_RECORDS {
                        listed.records.push((source.source(), job));
                    }
                }
            }
            Err(error) if error.code == ErrorCode::Cancelled => {}
            Err(error) => {
                health.failure(&id, &error.message, rate_limited(&error));
                listed
                    .failed
                    .push((source.source(), error.code, error.message));
            }
        }
    }
    listed.searched.dedup();
    listed
}

/// A named company's boards: from its own site (found through Wikidata),
/// else by asking the ATS APIs under its name. Remembered for a day.
pub async fn company_boards(state: &AppState, ctx: &Ctx<'_>, name: &str) -> Vec<Board> {
    let key = normalize::company_key(name);
    if let Some(boards) = state.career.cached_boards(&key) {
        return boards;
    }
    let mut found = Vec::new();
    if let Ok(Some(company)) = state.career.company(ctx, name).await {
        if let Some(website) = &company.website {
            let (_, boards) = company::careers(ctx, website).await;
            found = boards;
        }
    }
    if found.is_empty() {
        found = company::probe(ctx, name).await;
    }
    state.career.remember_boards(&key, &found);
    found
}

/// Whether a URL is on a site of the source registry's jobs scope or an
/// employer board (a quick check for search results from other routes).
pub fn is_job_source(url: &str) -> bool {
    company::board_of(url).is_some()
        || super::registry::source_of(url).is_some_and(|s| s.jobs)
        || Url::parse(url).is_ok_and(|u| {
            u.path_segments().is_some_and(|mut s| {
                s.any(|p| {
                    matches!(
                        p.to_lowercase().as_str(),
                        "jobs" | "job" | "careers" | "career" | "karriere" | "stellen"
                    )
                })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(board: Board, city: Option<&str>, country: Option<&str>) -> KnownBoard {
        KnownBoard {
            board,
            company: None,
            cities: city.into_iter().map(str::to_string).collect(),
            countries: country.into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn known_boards_near_the_place_come_first() {
        let ask = JobAsk {
            place: Place::from_text("Vienna"),
            ..JobAsk::default()
        };
        let picked = pick_known(
            vec![
                known(
                    Board::Greenhouse("berlin-co".into()),
                    Some("Berlin"),
                    Some("Germany"),
                ),
                known(
                    Board::Ashby("graz-co".into()),
                    Some("Graz"),
                    Some("Austria"),
                ),
                known(
                    Board::Greenhouse("wien-co".into()),
                    Some("Vienna"),
                    Some("Austria"),
                ),
                known(Board::Recruitee("unknown".into()), None, None),
            ],
            &ask,
        );
        let keys: Vec<String> = picked.iter().map(|(b, _)| b.key()).collect();
        assert_eq!(
            keys,
            ["greenhouse:wien-co", "ashby:graz-co", "recruitee:unknown"]
        );
        // Remote searches keep boards from elsewhere.
        let remote = JobAsk {
            remote: true,
            ..ask
        };
        assert_eq!(
            pick_known(
                vec![known(
                    Board::Greenhouse("berlin-co".into()),
                    Some("Berlin"),
                    None
                )],
                &remote
            )
            .len(),
            1
        );
    }

    #[test]
    fn public_boards_follow_region_and_remote() {
        let ids = |ask: &JobAsk| {
            public_boards(ask)
                .iter()
                .map(Source::id)
                .collect::<Vec<_>>()
        };
        let vienna = JobAsk {
            place: Place::from_text("Vienna"),
            ..JobAsk::default()
        };
        assert_eq!(ids(&vienna), ["arbeitnow", "themuse", "hn_hiring"]);
        let paris = JobAsk {
            place: Place::from_text("Paris"),
            ..JobAsk::default()
        };
        assert_eq!(ids(&paris), ["themuse", "hn_hiring"]);
        let remote = JobAsk {
            remote: true,
            ..JobAsk::default()
        };
        assert_eq!(
            ids(&remote),
            ["arbeitnow", "themuse", "remotive", "hn_hiring"]
        );
    }

    #[test]
    fn recognizes_job_sources() {
        assert!(is_job_source(
            "https://boards.greenhouse.io/nordlicht/jobs/1"
        ));
        assert!(is_job_source("https://www.karriere.at/jobs/123"));
        assert!(is_job_source(
            "https://careers.example.com/careers/ai-engineer"
        ));
        assert!(!is_job_source("https://en.wikipedia.org/wiki/Vienna"));
    }
}
