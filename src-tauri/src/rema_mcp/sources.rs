//! The source registry: what ReMa MCP may do with each job source, and on
//! what basis. A user-provided URL is an input, not a permission: a URL is
//! read only when its source's mode allows it.

use reqwest::Url;

use super::contract::AcquisitionMode;
use crate::analytics::normalize;

/// When the registry entries below were last reviewed.
pub const REVIEWED: &str = "2026-09-26";

#[derive(Debug)]
pub struct SourceInfo {
    /// Registry id, also a `sources` filter value.
    pub id: &'static str,
    pub name: &'static str,
    pub mode: AcquisitionMode,
    /// Host names (or suffixes after a dot) that belong to the source.
    pub hosts: &'static [&'static str],
    /// How vacancies are discovered.
    pub discovery: &'static str,
    /// How content is fetched.
    pub content: &'static str,
    pub needs_credentials: bool,
    pub implemented: bool,
    /// Documentation or permission basis.
    pub basis: &'static str,
    /// Fetch, retention, redistribution and request limits.
    pub restrictions: &'static str,
    /// For site-scoped discovery queries.
    pub site: Option<&'static str>,
    /// Countries whose searches include this board (regional boards).
    pub countries: &'static [&'static str],
}

const DISCOVERY_ONLY: &str = "Kept as a discovery link: the source's terms were not reviewed for \
                              automated reading, so ReMa does not fetch it.";

pub const REGISTRY: &[SourceInfo] = &[
    SourceInfo {
        id: "greenhouse",
        name: "Greenhouse",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &[
            "boards.greenhouse.io",
            "job-boards.greenhouse.io",
            "boards-api.greenhouse.io",
        ],
        discovery: "search results for employer boards",
        content: "Job Board API: GET /v1/boards/{board}/jobs/{id}",
        needs_credentials: false,
        implemented: true,
        basis: "Greenhouse Job Board API (docs.greenhouse.io/job-board.html): public GET \
                endpoints for published boards",
        restrictions: "Published board jobs only; never the application-submission API. \
                       EU-hosted boards are read as pages.",
        site: Some("greenhouse.io"),
        countries: &[],
    },
    SourceInfo {
        id: "lever",
        name: "Lever",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &[
            "jobs.lever.co",
            "jobs.eu.lever.co",
            "api.lever.co",
            "api.eu.lever.co",
        ],
        discovery: "search results for employer sites",
        content: "Postings API: GET /v0/postings/{site}/{id}",
        needs_credentials: false,
        implemented: true,
        basis: "Lever Postings API (github.com/lever/postings-api)",
        restrictions: "Known employer site only; not a global index.",
        site: Some("lever.co"),
        countries: &[],
    },
    SourceInfo {
        id: "ashby",
        name: "Ashby",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &["jobs.ashbyhq.com", "api.ashbyhq.com"],
        discovery: "search results for employer boards",
        content: "Job Postings API: GET /posting-api/job-board/{board}",
        needs_credentials: false,
        implemented: true,
        basis: "Ashby public Job Postings API (developers.ashbyhq.com)",
        restrictions: "Listed jobs only; one board read per 10 minutes.",
        site: Some("jobs.ashbyhq.com"),
        countries: &[],
    },
    SourceInfo {
        id: "personio",
        name: "Personio",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &["jobs.personio.de", "jobs.personio.com"],
        discovery: "search results for employer job pages",
        content: "Open-position XML feed: GET https://{company}.jobs.personio.de/xml",
        needs_credentials: false,
        implemented: true,
        basis: "Personio open-position XML feed (developer.personio.de)",
        restrictions: "Only when the employer enabled the feed; XML parsed without DTDs or \
                       external entities.",
        site: Some("jobs.personio.de"),
        countries: &[],
    },
    SourceInfo {
        id: "smartrecruiters",
        name: "SmartRecruiters",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &[
            "jobs.smartrecruiters.com",
            "careers.smartrecruiters.com",
            "api.smartrecruiters.com",
        ],
        discovery: "known employer career sites",
        content: "Posting API: GET /v1/companies/{company}/postings; posting pages read once",
        needs_credentials: false,
        implemented: true,
        basis: "SmartRecruiters Posting API (developers.smartrecruiters.com): public postings",
        restrictions: "Published postings of a known company only.",
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "workable",
        name: "Workable",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &["apply.workable.com"],
        discovery: "known employer career sites",
        content: "Jobs widget: GET /api/v1/widget/accounts/{account}",
        needs_credentials: false,
        implemented: true,
        basis: "Workable's public careers-page jobs widget (published jobs only)",
        restrictions: "Published jobs of a known account only.",
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "recruitee",
        name: "Recruitee",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &["recruitee.com"],
        discovery: "known employer career sites",
        content: "Careers Site API: GET https://{company}.recruitee.com/api/offers/",
        needs_credentials: false,
        implemented: true,
        basis: "Recruitee Careers Site API (docs.recruitee.com): published offers",
        restrictions: "Published offers of a known company only.",
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "arbeitnow",
        name: "Arbeitnow",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &["arbeitnow.com"],
        discovery: "the board's public job list, newest first",
        content: "Job board API: GET /api/job-board-api?page={n}",
        needs_credentials: false,
        implemented: true,
        basis: "Arbeitnow's free public job board API (arbeitnow.com/blog/job-board-api)",
        restrictions: "Mostly Germany and nearby countries; each job links to its Arbeitnow page.",
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "themuse",
        name: "The Muse",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &["themuse.com"],
        discovery: "the public jobs API, by location",
        content: "Public API: GET /api/public/jobs?page={n}&location={place}",
        needs_credentials: false,
        implemented: true,
        basis: "The Muse public API (themuse.com/developers/api/v2): no key below its hourly limit",
        restrictions: "Read sparingly (hourly limit without a key); each job links to The Muse.",
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "remotive",
        name: "Remotive",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &["remotive.com"],
        discovery: "the remote-jobs API, by search term",
        content: "Public API: GET /api/remote-jobs?search={terms}",
        needs_credentials: false,
        implemented: true,
        basis: "Remotive public remote-jobs API (remotive.com/api-documentation)",
        restrictions: "Remote roles only; read a few times a day (kept six hours); Remotive is \
                       named and linked as the source.",
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "hn_hiring",
        name: "Hacker News: Who is hiring?",
        mode: AcquisitionMode::DocumentedPublicFeed,
        hosts: &["news.ycombinator.com"],
        discovery: "the monthly hiring thread, through the HN Algolia search API",
        content: "HN Search API: GET /api/v1/search?tags=comment,story_{thread}",
        needs_credentials: false,
        implemented: true,
        basis: "Hacker News Search API (hn.algolia.com/api): public, no key",
        restrictions: "Postings are comments by the hiring companies; whether a role is still \
                       open is not stated.",
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "web",
        name: "Employer career pages",
        mode: AcquisitionMode::PermittedPublicPage,
        hosts: &[],
        discovery: "web search results",
        content: "one GET of the job page; schema.org JobPosting, else main text",
        needs_credentials: false,
        implemented: true,
        basis: "schema.org JobPosting and Google's job-posting structured-data guidance",
        restrictions: "Public addresses only; robots.txt honored; no scripts, cookies or \
                       logged-in content; structured markup is evidence, not a guarantee.",
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "linkedin",
        name: "LinkedIn",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["linkedin.com"],
        discovery: "indexed job links from the search backend",
        content: "none: links are never fetched",
        needs_credentials: false,
        implemented: true,
        basis: "LinkedIn's Job Posting API covers publishing, not candidate search; its User \
                Agreement restricts scraping",
        restrictions: "Discovered links only; the employer's own posting is looked up instead.",
        site: Some("linkedin.com/jobs"),
        countries: &[],
    },
    SourceInfo {
        id: "xing",
        name: "XING",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["xing.com"],
        discovery: "indexed job links from the search backend",
        content: "none: links are never fetched",
        needs_credentials: false,
        implemented: true,
        basis: "No verified candidate-side job-search endpoint or authorization (dev.xing.com)",
        restrictions: "Discovered links only; the employer's own posting is looked up instead.",
        site: Some("xing.com/jobs"),
        countries: &[],
    },
    SourceInfo {
        id: "karriere_at",
        name: "karriere.at",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["karriere.at"],
        discovery: "indexed job links",
        content: "none",
        needs_credentials: false,
        implemented: true,
        basis: DISCOVERY_ONLY,
        restrictions: DISCOVERY_ONLY,
        site: Some("karriere.at"),
        countries: &["Austria"],
    },
    SourceInfo {
        id: "jobs_at",
        name: "jobs.at",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["jobs.at"],
        discovery: "indexed job links",
        content: "none",
        needs_credentials: false,
        implemented: true,
        basis: DISCOVERY_ONLY,
        restrictions: DISCOVERY_ONLY,
        site: Some("jobs.at"),
        countries: &["Austria"],
    },
    SourceInfo {
        id: "stepstone",
        name: "StepStone",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["stepstone.de", "stepstone.at", "stepstone.pl"],
        discovery: "indexed job links",
        content: "none",
        needs_credentials: false,
        implemented: true,
        basis: DISCOVERY_ONLY,
        restrictions: DISCOVERY_ONLY,
        site: Some("stepstone.de"),
        countries: &["Germany"],
    },
    SourceInfo {
        id: "indeed",
        name: "Indeed",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["indeed.com", "indeed.de", "indeed.at", "indeed.ch"],
        discovery: "indexed job links",
        content: "none",
        needs_credentials: false,
        implemented: true,
        basis: DISCOVERY_ONLY,
        restrictions: DISCOVERY_ONLY,
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "glassdoor",
        name: "Glassdoor",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &[
            "glassdoor.com",
            "glassdoor.de",
            "glassdoor.at",
            "glassdoor.ch",
        ],
        discovery: "indexed job links",
        content: "none",
        needs_credentials: false,
        implemented: true,
        basis: DISCOVERY_ONLY,
        restrictions: DISCOVERY_ONLY,
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "jobs_ch",
        name: "jobs.ch",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["jobs.ch"],
        discovery: "indexed job links",
        content: "none",
        needs_credentials: false,
        implemented: true,
        basis: DISCOVERY_ONLY,
        restrictions: DISCOVERY_ONLY,
        site: Some("jobs.ch"),
        countries: &["Switzerland"],
    },
    SourceInfo {
        id: "jobup_ch",
        name: "jobup.ch",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["jobup.ch"],
        discovery: "indexed job links",
        content: "none",
        needs_credentials: false,
        implemented: true,
        basis: DISCOVERY_ONLY,
        restrictions: DISCOVERY_ONLY,
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "pracuj_pl",
        name: "Pracuj.pl",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["pracuj.pl"],
        discovery: "indexed job links",
        content: "none",
        needs_credentials: false,
        implemented: true,
        basis: DISCOVERY_ONLY,
        restrictions: DISCOVERY_ONLY,
        site: Some("pracuj.pl"),
        countries: &["Poland"],
    },
    SourceInfo {
        id: "nofluffjobs",
        name: "No Fluff Jobs",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["nofluffjobs.com"],
        discovery: "indexed job links",
        content: "none",
        needs_credentials: false,
        implemented: true,
        basis: DISCOVERY_ONLY,
        restrictions: DISCOVERY_ONLY,
        site: None,
        countries: &[],
    },
    SourceInfo {
        id: "justjoinit",
        name: "Just Join IT",
        mode: AcquisitionMode::SearchDiscoveryOnly,
        hosts: &["justjoin.it"],
        discovery: "indexed job links",
        content: "none",
        needs_credentials: false,
        implemented: true,
        basis: DISCOVERY_ONLY,
        restrictions: DISCOVERY_ONLY,
        site: None,
        countries: &[],
    },
];

pub fn get(id: &str) -> Option<&'static SourceInfo> {
    REGISTRY.iter().find(|s| s.id == id)
}

/// What a job URL points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Greenhouse {
        board: String,
        job: String,
    },
    Lever {
        site: String,
        id: String,
        eu: bool,
    },
    Ashby {
        board: String,
        id: String,
    },
    Personio {
        company: String,
        id: String,
        domain: String,
    },
    /// An employer or other permitted page, read once.
    Page,
    /// A discovery-only source: never fetched.
    DiscoveryOnly,
}

#[derive(Debug, Clone)]
pub struct Classified {
    pub source: &'static SourceInfo,
    pub target: Target,
    /// The URL, validated (http/https, no credentials, standard port).
    pub url: String,
    /// The source's own job id, for deduplication.
    pub source_job_id: Option<String>,
}

fn host_matches(host: &str, pattern: &str) -> bool {
    host == pattern || host.ends_with(&format!(".{pattern}"))
}

/// Why a URL is not accepted as input.
pub fn check_url(url: &str) -> Result<Url, String> {
    check_url_with(url, false)
}

/// `local_fixtures` (tests, debug builds with a local test site) also
/// accepts non-standard ports.
pub fn check_url_with(url: &str, local_fixtures: bool) -> Result<Url, String> {
    let parsed = Url::parse(url.trim()).map_err(|_| "not a valid web address".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("only http(s) addresses are read".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("addresses with credentials are not accepted".into());
    }
    if parsed.host_str().is_none_or(|h| h.is_empty()) {
        return Err("the address has no host".into());
    }
    if let Some(port) = parsed.port().filter(|_| !local_fixtures) {
        if port != 80 && port != 443 {
            return Err("only the standard web ports are read".into());
        }
    }
    Ok(parsed)
}

fn segments(url: &Url) -> Vec<String> {
    url.path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

fn safe_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 100
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Classifies a job URL (after [`check_url`]).
pub fn classify(url: &str) -> Result<Classified, String> {
    classify_with(url, false)
}

pub fn classify_with(url: &str, local_fixtures: bool) -> Result<Classified, String> {
    let parsed = check_url_with(url, local_fixtures)?;
    let host = parsed
        .host_str()
        .unwrap_or_default()
        .trim_start_matches("www.")
        .to_lowercase();
    let segs = segments(&parsed);
    let query = |name: &str| {
        parsed
            .query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    let clean = parsed.to_string();
    let found = |id: &str, target: Target, job: Option<String>| Classified {
        source: get(id).expect("registry id"),
        target,
        url: clean.clone(),
        source_job_id: job,
    };

    // ATS hosts with documented public APIs.
    if host == "boards.greenhouse.io" || host == "job-boards.greenhouse.io" {
        if segs.first().map(String::as_str) == Some("embed") {
            if let (Some(board), Some(job)) = (query("for"), query("token")) {
                if safe_part(&board) && job.chars().all(|c| c.is_ascii_digit()) {
                    let id = format!("{board}:{job}");
                    return Ok(found(
                        "greenhouse",
                        Target::Greenhouse { board, job },
                        Some(id),
                    ));
                }
            }
        }
        if let [board, jobs, job, ..] = segs.as_slice() {
            if jobs == "jobs" && safe_part(board) && job.chars().all(|c| c.is_ascii_digit()) {
                let id = format!("{board}:{job}");
                return Ok(found(
                    "greenhouse",
                    Target::Greenhouse {
                        board: board.clone(),
                        job: job.clone(),
                    },
                    Some(id),
                ));
            }
        }
    }
    if host == "jobs.lever.co" || host == "jobs.eu.lever.co" {
        if let [site, id, ..] = segs.as_slice() {
            if safe_part(site) && safe_part(id) && id.len() >= 20 {
                let key = format!("{site}:{id}");
                return Ok(found(
                    "lever",
                    Target::Lever {
                        site: site.clone(),
                        id: id.clone(),
                        eu: host == "jobs.eu.lever.co",
                    },
                    Some(key),
                ));
            }
        }
    }
    if host == "jobs.ashbyhq.com" {
        if let [board, id, ..] = segs.as_slice() {
            if safe_part(board) && safe_part(id) && id.len() >= 20 {
                let key = format!("{board}:{id}");
                return Ok(found(
                    "ashby",
                    Target::Ashby {
                        board: board.clone(),
                        id: id.clone(),
                    },
                    Some(key),
                ));
            }
        }
    }
    for domain in ["jobs.personio.de", "jobs.personio.com"] {
        if let Some(company) = host.strip_suffix(&format!(".{domain}")) {
            if let [job, id, ..] = segs.as_slice() {
                if job == "job"
                    && safe_part(company)
                    && !company.contains('.')
                    && id.chars().all(|c| c.is_ascii_digit())
                {
                    let key = format!("{company}:{id}");
                    return Ok(found(
                        "personio",
                        Target::Personio {
                            company: company.to_string(),
                            id: id.clone(),
                            domain: domain.to_string(),
                        },
                        Some(key),
                    ));
                }
            }
        }
    }

    // Employer sites and boards whose lists ReMa reads through their APIs;
    // a single posting is read as a page.
    if host == "jobs.smartrecruiters.com" || host == "careers.smartrecruiters.com" {
        if let [company, posting, ..] = segs.as_slice() {
            let id: String = posting.chars().take_while(char::is_ascii_digit).collect();
            if safe_part(company) && id.len() >= 6 {
                return Ok(found(
                    "smartrecruiters",
                    Target::Page,
                    Some(format!("{company}:{id}")),
                ));
            }
        }
    }
    if host == "apply.workable.com" {
        if let [account, j, code, ..] = segs.as_slice() {
            if j == "j" && safe_part(account) && safe_part(code) {
                return Ok(found(
                    "workable",
                    Target::Page,
                    Some(format!("{account}:{code}")),
                ));
            }
        }
        if let [j, _code, ..] = segs.as_slice() {
            if j == "j" {
                return Ok(found("workable", Target::Page, None));
            }
        }
    }
    if host.ends_with(".recruitee.com") && segs.first().map(String::as_str) == Some("o") {
        return Ok(found("recruitee", Target::Page, None));
    }
    if host_matches(&host, "arbeitnow.com") && segs.first().map(String::as_str) == Some("jobs") {
        let slug = segs.last().filter(|s| safe_part(s)).cloned();
        return Ok(found("arbeitnow", Target::Page, slug));
    }
    if host_matches(&host, "themuse.com") && segs.first().map(String::as_str) == Some("jobs") {
        return Ok(found("themuse", Target::Page, None));
    }
    if host_matches(&host, "remotive.com")
        && segs.first().map(String::as_str) == Some("remote-jobs")
    {
        let id = segs
            .last()
            .and_then(|s| s.rsplit('-').next())
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_string);
        return Ok(found("remotive", Target::Page, id));
    }
    if host == "news.ycombinator.com" && segs.first().map(String::as_str) == Some("item") {
        let id = query("id").filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()));
        return Ok(found("hn_hiring", Target::Page, id));
    }

    // Discovery-only sources (never fetched).
    for source in REGISTRY
        .iter()
        .filter(|s| s.mode == AcquisitionMode::SearchDiscoveryOnly)
    {
        if source.hosts.iter().any(|h| host_matches(&host, h)) {
            let id = normalize::platform_id(&clean).map(|(_, id)| id);
            return Ok(found(source.id, Target::DiscoveryOnly, id));
        }
    }
    // ATS hosts that are not one job (board roots, EU Greenhouse) are pages.
    Ok(found("web", Target::Page, None))
}

/// Registry sources to search for a set of countries: every source that is
/// not tied to a region, and regional boards of those countries.
pub fn discovery_sources(countries: &[String]) -> Vec<&'static SourceInfo> {
    REGISTRY
        .iter()
        .filter(|s| s.site.is_some())
        .filter(|s| {
            s.countries.is_empty() || s.countries.iter().any(|c| countries.iter().any(|k| k == c))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_documented_sources_and_discovery_links() {
        let gh = classify("https://job-boards.greenhouse.io/nordlicht/jobs/4411001").unwrap();
        assert_eq!(
            gh.target,
            Target::Greenhouse {
                board: "nordlicht".into(),
                job: "4411001".into()
            }
        );
        assert_eq!(gh.source_job_id.as_deref(), Some("nordlicht:4411001"));
        let embed =
            classify("https://boards.greenhouse.io/embed/job_app?for=nordlicht&token=77").unwrap();
        assert_eq!(embed.source.id, "greenhouse");

        let lever =
            classify("https://jobs.eu.lever.co/donau/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0/apply")
                .unwrap();
        assert!(matches!(lever.target, Target::Lever { eu: true, .. }));
        let ashby = classify("https://jobs.ashbyhq.com/alpen/1a2b3c4d-5e6f-7a8b-9c0d-1e2f3a4b5c6d")
            .unwrap();
        assert_eq!(ashby.source.id, "ashby");
        let personio = classify("https://wien-ai.jobs.personio.de/job/123456?language=de").unwrap();
        assert_eq!(personio.source_job_id.as_deref(), Some("wien-ai:123456"));

        let linkedin =
            classify("https://at.linkedin.com/jobs/view/senior-ai-engineer-3999999999").unwrap();
        assert_eq!(linkedin.target, Target::DiscoveryOnly);
        assert_eq!(linkedin.source.id, "linkedin");
        assert_eq!(linkedin.source_job_id.as_deref(), Some("3999999999"));
        assert_eq!(
            classify("https://www.xing.com/jobs/wien-ai-engineer-123456789")
                .unwrap()
                .source
                .id,
            "xing"
        );
        assert_eq!(
            classify("https://www.karriere.at/jobs/7654321")
                .unwrap()
                .target,
            Target::DiscoveryOnly
        );

        let sr =
            classify("https://jobs.smartrecruiters.com/NordlichtAI/744000099-senior-ai-engineer")
                .unwrap();
        assert_eq!(sr.source.id, "smartrecruiters");
        assert_eq!(sr.source_job_id.as_deref(), Some("NordlichtAI:744000099"));
        assert_eq!(
            classify("https://www.arbeitnow.com/jobs/companies/donau/senior-ai-engineer-wien-1")
                .unwrap()
                .source_job_id
                .as_deref(),
            Some("senior-ai-engineer-wien-1")
        );
        assert_eq!(
            classify("https://remotive.com/remote-jobs/software-dev/ai-engineer-77")
                .unwrap()
                .source_job_id
                .as_deref(),
            Some("77")
        );
        let hn = classify("https://news.ycombinator.com/item?id=501").unwrap();
        assert_eq!(
            (hn.source.id, hn.source_job_id.as_deref()),
            ("hn_hiring", Some("501"))
        );

        let page = classify("https://careers.nordlicht.example/jobs/ai-engineer").unwrap();
        assert_eq!((page.source.id, page.target), ("web", Target::Page));
        // EU Greenhouse boards: the API host is not verified, so a page.
        assert_eq!(
            classify("https://job-boards.eu.greenhouse.io/x/jobs/1")
                .unwrap()
                .target,
            Target::Page
        );
    }

    #[test]
    fn rejects_unsafe_addresses() {
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "https://user:pw@example.com/job",
            "https://example.com:8443/job",
            "ftp://example.com/job",
            "not a url",
        ] {
            assert!(classify(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn regional_boards_follow_the_countries() {
        let ids = |c: &[&str]| -> Vec<&str> {
            discovery_sources(&c.iter().map(|s| s.to_string()).collect::<Vec<_>>())
                .iter()
                .map(|s| s.id)
                .collect()
        };
        let austria = ids(&["Austria"]);
        assert!(austria.contains(&"karriere_at") && austria.contains(&"linkedin"));
        assert!(!austria.contains(&"pracuj_pl"));
        assert!(ids(&["Poland"]).contains(&"pracuj_pl"));
    }
}
