//! Reads each posting's own page and applies the request's hard filters.
//!
//! Pages are read like a browser opening them once: public addresses only,
//! no cookies or scripts, and a site that refuses automated reading is not
//! retried or worked around — its posting keeps only what the search said,
//! marked as unchecked. Pages that are gone, expired or closed are dropped.
//! What a posting does not state stays empty; nothing is estimated.

use std::{collections::HashMap, time::Duration};

use futures_util::{stream, StreamExt};
use tokio_util::sync::CancellationToken;

use super::{
    intent::{stem, JobQuery, MinSalary},
    Candidate, Excluded, Found, Listing, Progress, SalaryStatus, Verification,
};
use crate::{
    analytics::{
        normalize::{self, Salary},
        page::{self, FetchFailure},
    },
    models::analytics::{SalaryPeriod, WorkMode},
    state::AppState,
    time::now_ms,
};

/// Most pages read for one request.
const MAX_READS: usize = 16;
const CONCURRENT_READS: usize = 4;
const PAGE_TIMEOUT: Duration = Duration::from_secs(15);
/// All page reads together.
const READ_BUDGET: Duration = Duration::from_secs(75);
/// Most listings shown.
const MAX_LISTINGS: usize = 20;
const DAY_MS: i64 = 86_400_000;

/// Validated listings and what was left out.
#[derive(Debug, Clone, Default)]
pub struct Checked {
    pub listings: Vec<Listing>,
    pub excluded: Excluded,
    pub pages_read: usize,
}

/// A job board's search results page rather than one posting.
pub fn is_search_page(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let query_search = parsed.query_pairs().any(|(key, _)| {
        matches!(
            key.to_lowercase().as_str(),
            "q" | "query" | "keyword" | "keywords" | "search" | "k" | "kw" | "what" | "text"
        )
    });
    let path = parsed.path().to_lowercase();
    let path_search = path
        .split('/')
        .any(|segment| matches!(segment, "search" | "suche" | "jobsuche" | "results"));
    query_search || path_search
}

/// What reading a page gave.
enum Read {
    Page {
        url: String,
        html: String,
    },
    Failed(FetchFailure),
    /// Not read: over the budget, or the request was stopped.
    Skipped,
}

/// Why a posting is not shown.
pub(crate) enum Drop {
    Gone,
    Expired,
    Older,
    BelowSalary,
    Elsewhere,
    OffTopic,
    Unverifiable,
}

fn merge(into: &mut Candidate, from: &Candidate) {
    let fill = |a: &mut Option<String>, b: &Option<String>| {
        if a.is_none() {
            a.clone_from(b);
        }
    };
    fill(&mut into.title, &from.title);
    fill(&mut into.company, &from.company);
    fill(&mut into.location, &from.location);
    fill(&mut into.posted, &from.posted);
    fill(&mut into.salary, &from.salary);
    fill(&mut into.snippet, &from.snippet);
    into.reported |= from.reported;
    into.listed |= from.listed;
}

/// One candidate per posting, postings only, most trustworthy first.
fn unique(candidates: &[Candidate], excluded: &mut Excluded) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for candidate in candidates {
        let Some(key) = normalize::canonical_url(&candidate.url) else {
            continue;
        };
        if is_search_page(&candidate.url)
            || !(candidate.listed || normalize::is_job_specific(&candidate.url))
        {
            // Search pages and general pages the model offered as postings.
            if candidate.listed {
                excluded.not_postings += 1;
            }
            continue;
        }
        match seen.get(&key) {
            Some(&index) => merge(&mut out[index], candidate),
            None => {
                seen.insert(key, out.len());
                out.push(candidate.clone());
            }
        }
    }
    out.sort_by_key(|c| match (c.listed, c.reported) {
        (true, true) => 0,
        (true, false) => 1,
        _ => 2,
    });
    out
}

async fn read(state: &AppState, url: &str) -> Read {
    match tokio::time::timeout(PAGE_TIMEOUT, state.analytics.read_page(state, url)).await {
        Ok(Ok((url, html))) => Read::Page { url, html },
        Ok(Err(failure)) => Read::Failed(failure),
        Err(_) => Read::Failed(FetchFailure::Failed("the page took too long".into())),
    }
}

/// Reads the first pages concurrently, within the time budget.
async fn read_all(
    state: &AppState,
    candidates: &[Candidate],
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> Vec<Read> {
    let mut reads: Vec<Read> = candidates.iter().map(|_| Read::Skipped).collect();
    let count = candidates.len().min(MAX_READS);
    if count == 0 {
        return reads;
    }
    progress.status(&format!(
        "Checking {count} posting{}…",
        if count == 1 { "" } else { "s" }
    ));
    // Owned inputs: the reads run inside the spawned chat task.
    let jobs: Vec<(usize, String, AppState)> = candidates
        .iter()
        .take(count)
        .enumerate()
        .map(|(index, c)| (index, c.url.clone(), state.clone()))
        .collect();
    let mut pending = stream::iter(jobs)
        .map(|(index, url, state)| async move { (index, read(&state, &url).await) })
        .buffer_unordered(CONCURRENT_READS);
    let deadline = tokio::time::sleep(READ_BUDGET);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = &mut deadline => break,
            next = pending.next() => {
                let Some((index, result)) = next else { break };
                let outcome = match &result {
                    Read::Page { .. } => Ok(()),
                    Read::Failed(failure) => Err(failure.message()),
                    Read::Skipped => Err("not read".to_string()),
                };
                progress.page(&candidates[index].url, outcome);
                reads[index] = result;
            }
        }
    }
    reads
}

fn clip(text: &str, max: usize) -> String {
    normalize::clip(&page::decode_entities(text), max)
}

/// The first sentences of a description.
pub(crate) fn summary(text: &str) -> Option<String> {
    let text = normalize::clip(text, 600);
    let mut out = String::new();
    for sentence in text.split_inclusive(['.', '!', '?']) {
        if out.len() + sentence.len() > 240 {
            break;
        }
        out.push_str(sentence);
    }
    if out.is_empty() {
        out = normalize::clip(&text, 240);
    }
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// Phrases a page shows once a posting is closed.
fn says_closed(text: &str) -> bool {
    const CLOSED: &[&str] = &[
        "no longer available",
        "no longer accepting applications",
        "this job has expired",
        "this posting has expired",
        "position has been filled",
        "job is closed",
        "not accepting applications",
        "nicht mehr verfügbar",
        "nicht mehr aktiv",
        "stelle ist bereits besetzt",
        "bereits vergeben",
    ];
    let lower = text.to_lowercase();
    CLOSED.iter().any(|phrase| lower.contains(phrase))
}

fn future_date(text: &str) -> Option<jiff::civil::Date> {
    let day = text.get(..10)?;
    day.parse::<jiff::civil::Date>().ok()
}

pub(crate) fn work_mode_label(mode: WorkMode) -> &'static str {
    match mode {
        WorkMode::Remote => "Remote",
        WorkMode::Hybrid => "Hybrid",
        WorkMode::Onsite => "On-site",
    }
}

fn words(title: &str) -> Vec<String> {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
        .filter(|w| !w.is_empty())
        .map(stem)
        .collect()
}

/// Whether the title shares a word of the requested role, or has every
/// word of one of its close variants ("Machine Learning Engineer" for "AI").
pub(crate) fn relevant(title: &str, query: &JobQuery, related: &[String]) -> bool {
    let wanted = query.role_terms();
    if wanted.is_empty() {
        return true;
    }
    let have = words(title);
    wanted.iter().any(|w| have.contains(w))
        || related.iter().any(|role| {
            let need = words(role);
            !need.is_empty() && need.iter().all(|w| have.contains(w))
        })
}

/// Whether a location is (or includes) the requested place.
pub(crate) fn in_place(location: &str, wanted: &str) -> bool {
    let have = normalize::place(location);
    let want = normalize::place(wanted);
    if have.cities.is_empty() && have.countries.is_empty() {
        // Unrecognized ("Remote", "EMEA"): cannot be ruled out.
        return true;
    }
    match (want.city(), want.country()) {
        (Some(city), _) => {
            have.cities.contains(&city)
                || (have.cities.is_empty()
                    && want.country().is_some_and(|c| have.countries.contains(&c)))
        }
        (None, Some(country)) => have.countries.contains(&country),
        (None, None) => location.to_lowercase().contains(&wanted.to_lowercase()),
    }
}

fn period_amount(salary: &Salary, value: f64, period: SalaryPeriod) -> Option<f64> {
    let yearly = match salary.period? {
        SalaryPeriod::Year => value,
        SalaryPeriod::Month => value * 12.0,
        _ => return None,
    };
    Some(match period {
        SalaryPeriod::Month => yearly / 12.0,
        _ => yearly,
    })
}

/// The request's posting-date window (§28): an older posting is not shown;
/// one without a date is shown with a note.
pub(crate) fn check_date(
    posted: Option<i64>,
    days: Option<u32>,
    now: i64,
    notes: &mut Vec<String>,
) -> Result<(), Drop> {
    if let Some(days) = days {
        match posted {
            Some(date) if (now - date) / DAY_MS > i64::from(days) => return Err(Drop::Older),
            Some(_) => {}
            None => notes.push("posting date not shown".to_string()),
        }
    }
    Ok(())
}

/// The requested salary minimum (§27): a stated salary below it is not
/// shown; an unknown one is shown and marked, never counted as meeting it.
pub(crate) fn check_salary(
    salary: Option<&Salary>,
    estimate: bool,
    floor: Option<&MinSalary>,
    notes: &mut Vec<String>,
) -> Result<SalaryStatus, Drop> {
    let status = match (salary, estimate) {
        (None, _) => SalaryStatus::NotListed,
        (Some(_), true) => SalaryStatus::Estimated,
        (Some(_), false) => SalaryStatus::Verified,
    };
    let Some(floor) = floor else {
        return Ok(status);
    };
    match (salary, status) {
        (None, _) => notes.push("salary not stated".to_string()),
        (Some(_), SalaryStatus::Estimated) => notes.push("salary is only an estimate".to_string()),
        (Some(stated), _) => {
            let other_currency =
                matches!((floor.currency, stated.currency), (Some(a), Some(b)) if a != b);
            let top = stated.max.or(stated.min);
            match top.and_then(|v| period_amount(stated, v, floor.period)) {
                _ if other_currency => notes.push("salary in another currency".to_string()),
                Some(top) if top < floor.amount => return Err(Drop::BelowSalary),
                Some(_) => {
                    let bottom = stated
                        .min
                        .and_then(|v| period_amount(stated, v, floor.period));
                    if bottom.is_some_and(|b| b < floor.amount) {
                        notes.push("salary range starts below the requested minimum".into());
                    }
                }
                None => notes.push("salary period not stated".to_string()),
            }
        }
    }
    Ok(status)
}

/// Where a posting was found, by its address.
pub(crate) fn source_label(url: &str) -> String {
    normalize::source_name(url)
        .or_else(|| {
            crate::rema_mcp::sources::classify(url)
                .ok()
                .filter(|c| c.source.id != "web")
                .map(|c| c.source.name.to_string())
        })
        .or_else(|| {
            reqwest::Url::parse(url).ok().and_then(|u| {
                u.host_str()
                    .map(|h| h.trim_start_matches("www.").to_string())
            })
        })
        .unwrap_or_else(|| "web".to_string())
}

/// Builds the listing, or says why it is not shown.
#[cfg(test)]
fn build(
    candidate: &Candidate,
    read: &Read,
    query: &JobQuery,
    now: i64,
    trust_listed: bool,
) -> Result<Listing, Drop> {
    build_with(candidate, read, query, &[], now, trust_listed)
}

fn build_with(
    candidate: &Candidate,
    read: &Read,
    query: &JobQuery,
    related: &[String],
    now: i64,
    trust_listed: bool,
) -> Result<Listing, Drop> {
    let mut notes = Vec::new();
    let today = normalize::date_of(now);
    let (url, facts, page_title, page_text, verification) = match read {
        Read::Page { url, html } => {
            let facts = page::parse(html, candidate.title.as_deref().unwrap_or_default());
            let text = page::html_to_text(html);
            let verification = if facts.structured {
                Verification::Posting
            } else {
                Verification::Page
            };
            (
                url.clone(),
                Some(facts),
                page::page_title(html),
                Some(text),
                verification,
            )
        }
        Read::Failed(FetchFailure::Gone(_)) => return Err(Drop::Gone),
        Read::Failed(failure) => {
            if !(candidate.reported || (candidate.listed && trust_listed)) {
                return Err(Drop::Unverifiable);
            }
            notes.push(format!(
                "ReMa could not open the page: {}",
                failure.message()
            ));
            (
                candidate.url.clone(),
                None,
                None,
                None,
                Verification::SearchOnly,
            )
        }
        Read::Skipped => {
            if !(candidate.reported || (candidate.listed && trust_listed)) {
                return Err(Drop::Unverifiable);
            }
            notes.push("ReMa did not open the page (time limit)".to_string());
            (
                candidate.url.clone(),
                None,
                None,
                None,
                Verification::SearchOnly,
            )
        }
    };
    if page_text.as_deref().is_some_and(says_closed) {
        return Err(Drop::Expired);
    }
    let facts = facts.unwrap_or_default();
    if let Some(until) = facts.valid_through.as_deref().and_then(future_date) {
        if until < today {
            return Err(Drop::Expired);
        }
    }
    let title = facts
        .title
        .clone()
        .or_else(|| candidate.title.clone())
        .or(page_title)
        .map(|t| clip(&t, 120))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| normalize::source_name(&url).unwrap_or_else(|| url.clone()));
    if !relevant(&title, query, related) {
        return Err(Drop::OffTopic);
    }
    let location = facts
        .location
        .clone()
        .or_else(|| candidate.location.clone())
        .map(|l| clip(&l, 80));
    let work_mode = facts
        .work_mode
        .or_else(|| location.as_deref().and_then(normalize::work_mode))
        .map(work_mode_label);
    if let (Some(wanted), Some(location)) = (&query.location, &location) {
        let remote_ok = query.remote && work_mode == Some("Remote");
        if !remote_ok && !in_place(location, wanted) {
            return Err(Drop::Elsewhere);
        }
    }
    // The page's own date first; the search's only for unread pages.
    let posted = match facts.date_posted.as_deref() {
        Some(date) => normalize::posted(date, now),
        None => {
            let from_search = candidate
                .posted
                .as_deref()
                .and_then(|p| normalize::posted(p, now));
            if from_search.is_some() && verification != Verification::SearchOnly {
                notes.push("posting date from the search result".to_string());
            }
            from_search
        }
    };
    check_date(posted, query.posted_within_days, now, &mut notes)?;
    let salary_text = facts
        .salary_text
        .clone()
        .or_else(|| candidate.salary.clone())
        .map(|s| clip(&s, 60));
    let salary = facts
        .salary
        .clone()
        .or_else(|| salary_text.as_deref().and_then(normalize::salary));
    let salary_status = check_salary(
        salary.as_ref(),
        false,
        query.min_salary.as_ref(),
        &mut notes,
    )?;
    // As ReMa read it ("€95k–120k / year"); the posting's words otherwise.
    let salary_text = salary
        .as_ref()
        .filter(|s| s.min.is_some() || s.max.is_some())
        .map(normalize::format_salary)
        .or(salary_text);
    let summary = facts
        .description
        .as_deref()
        .and_then(summary)
        .or_else(|| candidate.snippet.as_deref().map(|s| clip(s, 240)));
    let source = source_label(&url);
    // What the posting states about its life (§50); checked now when its
    // page was read.
    let posting_facts = super::PostingFacts {
        valid_through: facts.valid_through.clone(),
        verified_at: (verification != Verification::SearchOnly).then_some(now),
        apply_url: None,
        source_job_id: normalize::platform_id(&url)
            .map(|(platform, id)| format!("{platform}:{id}")),
    };
    Ok(Listing {
        title,
        company: facts
            .company
            .clone()
            .or_else(|| candidate.company.clone())
            .map(|c| clip(&c, 80)),
        location,
        url,
        summary,
        posted,
        salary: salary_text,
        salary_status,
        work_mode: work_mode.map(str::to_string),
        verification,
        source,
        notes,
        facts: posting_facts,
    })
}

/// Reads the candidates' pages and keeps the listings that meet the
/// request.
pub async fn check(
    state: &AppState,
    found: &Found,
    query: &JobQuery,
    related: &[String],
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> Checked {
    let now = now_ms();
    let mut excluded = Excluded::default();
    let candidates = unique(&found.candidates, &mut excluded);
    let reads = read_all(state, &candidates, progress, cancel).await;
    let pages_read = reads
        .iter()
        .filter(|r| matches!(r, Read::Page { .. }))
        .count();
    let mut listings: Vec<Listing> = Vec::new();
    for (candidate, read) in candidates.iter().zip(&reads) {
        match build_with(candidate, read, query, related, now, !found.lists_sources) {
            Ok(listing) => {
                // Two addresses can lead to one page.
                let key = normalize::canonical_url(&listing.url);
                if !listings
                    .iter()
                    .any(|l| normalize::canonical_url(&l.url) == key)
                {
                    listings.push(listing);
                }
            }
            Err(Drop::Gone) => excluded.gone += 1,
            Err(Drop::Expired) => excluded.expired += 1,
            Err(Drop::Older) => excluded.older += 1,
            Err(Drop::BelowSalary) => excluded.below_salary += 1,
            Err(Drop::Elsewhere) => excluded.elsewhere += 1,
            Err(Drop::OffTopic) => excluded.off_topic += 1,
            Err(Drop::Unverifiable) => {}
        }
    }
    // Newest first; checked postings before unchecked ones.
    listings.sort_by(|a, b| {
        b.posted
            .unwrap_or(i64::MIN)
            .cmp(&a.posted.unwrap_or(i64::MIN))
            .then((a.verification as u8).cmp(&(b.verification as u8)))
    });
    listings.truncate(MAX_LISTINGS);
    Checked {
        listings,
        excluded,
        pages_read,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::detect;

    // 2026-09-26
    const NOW: i64 = 1_790_380_800_000;

    fn page(html: &str) -> Read {
        Read::Page {
            url: "https://careers.example.com/jobs/ai-engineer-1".into(),
            html: html.into(),
        }
    }

    fn posting(title: &str, posted: &str, salary: &str, extra: &str) -> String {
        format!(
            r#"<html><head><title>{title}</title><script type="application/ld+json">{{"@type":"JobPosting","title":"{title}","hiringOrganization":{{"name":"Nordlicht AI"}},"datePosted":"{posted}","jobLocation":{{"address":{{"addressLocality":"Vienna","addressCountry":"AT"}}}}{salary}{extra},"description":"Build LLM products. Work with a small team."}}</script></head><body></body></html>"#
        )
    }

    fn candidate() -> Candidate {
        Candidate {
            url: "https://careers.example.com/jobs/ai-engineer-1".into(),
            title: Some("AI Engineer".into()),
            reported: true,
            listed: true,
            ..Candidate::default()
        }
    }

    #[test]
    fn keeps_what_the_posting_page_states() {
        let query =
            detect("Find current AI Engineer jobs in Vienna, posted within the last 10 days.")
                .unwrap();
        let html = posting(
            "Senior AI Engineer",
            "2026-09-23",
            r#","baseSalary":{"@type":"MonetaryAmount","currency":"EUR","value":{"@type":"QuantitativeValue","minValue":85000,"maxValue":100000,"unitText":"YEAR"}}"#,
            "",
        );
        let listing = build(&candidate(), &page(&html), &query, NOW, false)
            .ok()
            .unwrap();
        assert_eq!(listing.title, "Senior AI Engineer");
        assert_eq!(listing.company.as_deref(), Some("Nordlicht AI"));
        assert_eq!(listing.verification, Verification::Posting);
        assert_eq!(
            normalize::date_of(listing.posted.unwrap()).to_string(),
            "2026-09-23"
        );
        assert!(listing.salary.unwrap().contains("85"));
        assert_eq!(
            listing.summary.as_deref(),
            Some("Build LLM products. Work with a small team.")
        );
        assert!(listing.notes.is_empty());
    }

    #[test]
    fn applies_the_date_window_and_the_salary_floor() {
        let query =
            detect("Find current AI Engineer jobs in Vienna, posted within the last 10 days.")
                .unwrap();
        let old = posting("AI Engineer", "2026-09-01", "", "");
        assert!(matches!(
            build(&candidate(), &page(&old), &query, NOW, false),
            Err(Drop::Older)
        ));

        let query =
            detect("Find senior AI engineering jobs in Vienna with salary above 90K a year")
                .unwrap();
        let low = posting(
            "AI Engineer",
            "2026-09-23",
            r#","baseSalary":{"currency":"EUR","value":{"minValue":60000,"maxValue":70000,"unitText":"YEAR"}}"#,
            "",
        );
        assert!(matches!(
            build(&candidate(), &page(&low), &query, NOW, false),
            Err(Drop::BelowSalary)
        ));
        let monthly = posting(
            "AI Engineer",
            "2026-09-23",
            r#","baseSalary":{"currency":"EUR","value":{"minValue":7000,"maxValue":8500,"unitText":"MONTH"}}"#,
            "",
        );
        let listing = build(&candidate(), &page(&monthly), &query, NOW, false)
            .ok()
            .unwrap();
        assert!(listing.notes.iter().any(|n| n.contains("starts below")));
        let unstated = posting("AI Engineer", "2026-09-23", "", "");
        let listing = build(&candidate(), &page(&unstated), &query, NOW, false)
            .ok()
            .unwrap();
        assert_eq!(listing.notes, vec!["salary not stated"]);
    }

    #[test]
    fn drops_closed_expired_and_unrelated_postings() {
        let query = detect("Find AI Engineer jobs in Vienna").unwrap();
        let expired = posting(
            "AI Engineer",
            "2026-09-01",
            "",
            r#","validThrough":"2026-09-20""#,
        );
        assert!(matches!(
            build(&candidate(), &page(&expired), &query, NOW, false),
            Err(Drop::Expired)
        ));
        let closed = "<html><title>AI Engineer</title><body>This job has expired.</body></html>";
        assert!(matches!(
            build(&candidate(), &page(closed), &query, NOW, false),
            Err(Drop::Expired)
        ));
        let sales = posting("Sales Manager", "2026-09-23", "", "");
        assert!(matches!(
            build(&candidate(), &page(&sales), &query, NOW, false),
            Err(Drop::OffTopic)
        ));
        let gone = Read::Failed(FetchFailure::Gone(404));
        assert!(matches!(
            build(&candidate(), &gone, &query, NOW, false),
            Err(Drop::Gone)
        ));
        let berlin = posting("AI Engineer", "2026-09-23", "", "")
            .replace("Vienna", "Berlin")
            .replace("\"AT\"", "\"DE\"");
        assert!(matches!(
            build(&candidate(), &page(&berlin), &query, NOW, false),
            Err(Drop::Elsewhere)
        ));
    }

    #[test]
    fn marks_pages_it_could_not_open() {
        let query =
            detect("Find AI Engineer jobs in Vienna posted within the last 10 days").unwrap();
        let refused = Read::Failed(FetchFailure::Refused(403));
        let mut from_search = candidate();
        from_search.posted = Some("2 days ago".into());
        from_search.location = Some("Wien, Österreich".into());
        let listing = build(&from_search, &refused, &query, NOW, false)
            .ok()
            .unwrap();
        assert_eq!(listing.verification, Verification::SearchOnly);
        assert!(listing.notes[0].contains("does not allow automated reading (403)"));
        assert!(listing.posted.is_some());

        // A posting only the model named, from an engine that reports its
        // results, is not shown when its page cannot be opened.
        let mut named = candidate();
        named.reported = false;
        assert!(matches!(
            build(&named, &refused, &query, NOW, false),
            Err(Drop::Unverifiable)
        ));
        // Codex does not report result addresses, so its postings stay (marked).
        assert!(build(&named, &refused, &query, NOW, true).is_ok());
    }

    #[test]
    fn recognizes_search_pages() {
        assert!(is_search_page(
            "https://www.linkedin.com/jobs/search/?keywords=AI&location=Vienna"
        ));
        assert!(is_search_page(
            "https://at.indeed.com/jobs?q=AI+Engineer&l=Wien"
        ));
        assert!(is_search_page(
            "https://www.stepstone.at/jobs/suche?what=ai"
        ));
        assert!(!is_search_page(
            "https://www.linkedin.com/jobs/view/4012345678/"
        ));
        assert!(!is_search_page(
            "https://careers.example.com/jobs/ai-engineer-123"
        ));
    }
}
