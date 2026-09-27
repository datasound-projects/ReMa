//! The career source registry: which sites career searches look at, by
//! scope and region (§23). One list, so domains are not scattered through
//! the code.
//!
//! This registry scopes *searching* (provider search domain filters, the
//! board and site discovery of ReMa's own sources). Whether and how ReMa
//! may *read* a page is decided separately by the Jobs MCP's source
//! registry (`rema_mcp::sources`): a domain listed here as a search scope
//! (LinkedIn, Indeed) can still be discovery-only there.

use serde::Serialize;
use specta::Type;

use super::requirement::Scopes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    /// A job board or job search engine.
    JobBoard,
    /// An applicant tracking system hosting employers' job pages.
    Ats,
    /// A professional network.
    Network,
    /// Company facts (profiles, funding, size).
    CompanyInfo,
    /// Salary and market data.
    Market,
}

#[derive(Debug)]
pub struct CareerSource {
    pub id: &'static str,
    pub name: &'static str,
    /// Registrable domains; subdomains are included.
    pub domains: &'static [&'static str],
    pub kind: SourceType,
    /// ISO 3166-1 alpha-2 codes; empty: everywhere.
    pub regions: &'static [&'static str],
    /// 0 is the most useful.
    pub priority: u8,
    pub jobs: bool,
    pub company: bool,
    pub people: bool,
    pub market: bool,
    /// The employer's own system (an ATS page is the employer's posting).
    pub official: bool,
}

const DACH: &[&str] = &["AT", "DE", "CH"];

macro_rules! source {
    ($id:literal, $name:literal, [$($d:literal),+], $kind:ident, $regions:expr, $prio:literal,
     jobs: $jobs:literal, company: $company:literal, people: $people:literal,
     market: $market:literal, official: $official:literal) => {
        CareerSource {
            id: $id,
            name: $name,
            domains: &[$($d),+],
            kind: SourceType::$kind,
            regions: $regions,
            priority: $prio,
            jobs: $jobs,
            company: $company,
            people: $people,
            market: $market,
            official: $official,
        }
    };
}

pub const SOURCES: &[CareerSource] = &[
    // Applicant tracking systems: the employers' own postings.
    source!("greenhouse", "Greenhouse", ["greenhouse.io"], Ats, &[], 0,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("lever", "Lever", ["lever.co"], Ats, &[], 0,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("ashby", "Ashby", ["ashbyhq.com"], Ats, &[], 0,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("workday", "Workday", ["myworkdayjobs.com"], Ats, &[], 1,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("smartrecruiters", "SmartRecruiters", ["smartrecruiters.com"], Ats, &[], 1,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("personio", "Personio", ["personio.de", "personio.com"], Ats, &[], 1,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("teamtailor", "Teamtailor", ["teamtailor.com"], Ats, &[], 1,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("workable", "Workable", ["workable.com"], Ats, &[], 1,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("recruitee", "Recruitee", ["recruitee.com"], Ats, &[], 1,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("successfactors", "SAP SuccessFactors", ["successfactors.com", "successfactors.eu"], Ats, &[], 2,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("taleo", "Oracle Taleo", ["taleo.net"], Ats, &[], 2,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("icims", "iCIMS", ["icims.com"], Ats, &[], 2,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("join", "JOIN", ["join.com"], Ats, DACH, 2,
        jobs: true, company: false, people: false, market: false, official: true),
    source!("softgarden", "softgarden", ["softgarden.io"], Ats, DACH, 2,
        jobs: true, company: false, people: false, market: false, official: true),
    // Professional networks.
    source!("linkedin", "LinkedIn", ["linkedin.com"], Network, &[], 0,
        jobs: true, company: true, people: true, market: false, official: false),
    source!("xing", "XING", ["xing.com"], Network, DACH, 0,
        jobs: true, company: true, people: true, market: false, official: false),
    // Job boards.
    source!("indeed", "Indeed", ["indeed.com"], JobBoard, &[], 1,
        jobs: true, company: false, people: false, market: true, official: false),
    source!("karriere_at", "karriere.at", ["karriere.at"], JobBoard, &["AT"], 0,
        jobs: true, company: false, people: false, market: false, official: false),
    source!("jobs_at", "jobs.at", ["jobs.at"], JobBoard, &["AT"], 1,
        jobs: true, company: false, people: false, market: false, official: false),
    source!("stepstone_at", "StepStone (AT)", ["stepstone.at"], JobBoard, &["AT"], 0,
        jobs: true, company: false, people: false, market: false, official: false),
    source!("stepstone_de", "StepStone (DE)", ["stepstone.de"], JobBoard, &["DE"], 0,
        jobs: true, company: false, people: false, market: false, official: false),
    source!("jobs_ch", "jobs.ch", ["jobs.ch"], JobBoard, &["CH"], 0,
        jobs: true, company: false, people: false, market: false, official: false),
    source!("arbeitnow", "Arbeitnow", ["arbeitnow.com"], JobBoard, DACH, 2,
        jobs: true, company: false, people: false, market: false, official: false),
    source!("welcome_to_the_jungle", "Welcome to the Jungle", ["welcometothejungle.com"], JobBoard, &["FR", "ES", "CZ"], 1,
        jobs: true, company: true, people: false, market: false, official: false),
    // Company facts and the market.
    source!("crunchbase", "Crunchbase", ["crunchbase.com"], CompanyInfo, &[], 1,
        jobs: false, company: true, people: false, market: false, official: false),
    source!("wikipedia", "Wikipedia", ["wikipedia.org"], CompanyInfo, &[], 2,
        jobs: false, company: true, people: false, market: false, official: false),
    source!("kununu", "kununu", ["kununu.com"], Market, DACH, 0,
        jobs: false, company: true, people: false, market: true, official: false),
    source!("glassdoor", "Glassdoor", ["glassdoor.com"], Market, &[], 1,
        jobs: false, company: true, people: false, market: true, official: false),
    source!("levels_fyi", "Levels.fyi", ["levels.fyi"], Market, &[], 1,
        jobs: false, company: false, people: false, market: true, official: false),
];

/// Most domains passed to a provider's search filter.
pub const MAX_DOMAINS: usize = 20;

fn relevant(source: &CareerSource, scopes: Scopes, country: Option<&str>) -> bool {
    let in_scope = (scopes.jobs && source.jobs)
        || (scopes.company && source.company)
        || (scopes.people && source.people)
        || (scopes.market && source.market);
    // Without a country, regional sources are left out.
    let in_region =
        source.regions.is_empty() || country.is_some_and(|c| source.regions.contains(&c));
    in_scope && in_region
}

/// The sites a search should stay on: the named companies' own domains
/// first, then the registry's sources for the scopes and country (regional
/// sources first, then by priority), at most [`MAX_DOMAINS`]. Empty when
/// the request has no career scope (no restriction).
pub fn allowed_domains(scopes: Scopes, country: Option<&str>, companies: &[String]) -> Vec<String> {
    if !scopes.any() {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    let mut push = |domain: &str| {
        let domain = domain.trim().trim_start_matches("www.").to_lowercase();
        if !domain.is_empty() && !out.contains(&domain) && out.len() < MAX_DOMAINS {
            out.push(domain);
        }
    };
    for company in companies {
        push(company);
    }
    let mut sources: Vec<&CareerSource> = SOURCES
        .iter()
        .filter(|s| relevant(s, scopes, country))
        .collect();
    sources.sort_by_key(|s| (s.regions.is_empty(), s.priority));
    for source in sources {
        for domain in source.domains {
            push(domain);
        }
    }
    out
}

/// The registry entry a URL belongs to.
pub fn source_of(url: &str) -> Option<&'static CareerSource> {
    let host = reqwest::Url::parse(url)
        .ok()?
        .host_str()?
        .trim_start_matches("www.")
        .to_lowercase();
    SOURCES.iter().find(|s| {
        s.domains
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jobs() -> Scopes {
        Scopes {
            jobs: true,
            ..Scopes::default()
        }
    }

    #[test]
    fn job_searches_stay_on_job_sources_of_the_region() {
        let domains = allowed_domains(jobs(), Some("AT"), &[]);
        assert!(domains.len() <= MAX_DOMAINS);
        for expected in [
            "karriere.at",
            "stepstone.at",
            "greenhouse.io",
            "lever.co",
            "linkedin.com",
        ] {
            assert!(
                domains.contains(&expected.to_string()),
                "{expected} missing: {domains:?}"
            );
        }
        // Regional boards of other countries and non-job sources stay out.
        for absent in ["stepstone.de", "jobs.ch", "levels.fyi", "crunchbase.com"] {
            assert!(!domains.contains(&absent.to_string()), "{absent} present");
        }
        // The Austrian boards come first.
        assert!(
            domains.iter().position(|d| d == "karriere.at")
                < domains.iter().position(|d| d == "lever.co")
        );
    }

    #[test]
    fn named_companies_come_first_and_the_list_is_bounded() {
        let companies = vec![
            "www.Bitpanda.com".to_string(),
            "careers.example.at".to_string(),
        ];
        let domains = allowed_domains(jobs(), Some("AT"), &companies);
        assert_eq!(&domains[..2], ["bitpanda.com", "careers.example.at"]);
        assert_eq!(domains.len(), MAX_DOMAINS);
    }

    #[test]
    fn scopes_choose_their_sources() {
        let people = allowed_domains(
            Scopes {
                people: true,
                ..Scopes::default()
            },
            Some("DE"),
            &[],
        );
        assert_eq!(people, ["xing.com", "linkedin.com"]);
        let market = allowed_domains(
            Scopes {
                market: true,
                ..Scopes::default()
            },
            None,
            &[],
        );
        assert!(market.contains(&"levels.fyi".to_string()));
        assert!(
            !market.contains(&"kununu.com".to_string()),
            "regional without a country"
        );
        assert!(allowed_domains(Scopes::default(), Some("AT"), &[]).is_empty());
    }

    #[test]
    fn urls_map_to_their_source() {
        assert_eq!(
            source_of("https://boards.greenhouse.io/acme/jobs/1").map(|s| s.id),
            Some("greenhouse")
        );
        assert_eq!(
            source_of("https://at.indeed.com/viewjob?jk=1").map(|s| s.id),
            Some("indeed")
        );
        assert_eq!(source_of("https://example.com/jobs/1").map(|s| s.id), None);
    }
}
