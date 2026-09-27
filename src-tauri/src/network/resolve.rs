//! Entity resolution (NC §35): one company or person per real entity,
//! merged conservatively. Companies merge on the same normalized name or
//! the same own domain — but not when one name extends the other on that
//! domain (a likely division or subsidiary); people only on the same name
//! at the same company with a compatible title or the same profile — never
//! on a name alone.

use super::{
    evidence,
    model::{Company, Person},
};
use crate::{analytics::normalize, rema_mcp::store};

/// Hosting and network domains that never identify a company.
const SHARED_DOMAINS: &[&str] = &[
    "linkedin.com",
    "xing.com",
    "greenhouse.io",
    "lever.co",
    "ashbyhq.com",
    "personio.de",
    "personio.com",
    "recruitee.com",
    "smartrecruiters.com",
    "workable.com",
    "wikipedia.org",
    "wikidata.org",
    "github.io",
    "google.com",
    "facebook.com",
    "instagram.com",
    "x.com",
    "twitter.com",
    "medium.com",
    "crunchbase.com",
    "karriere.at",
    "stepstone.at",
    "stepstone.de",
    "indeed.com",
    "glassdoor.com",
];

pub fn company_key(name: &str) -> String {
    normalize::company_key(name)
}

/// A stable id from a key ("co_3f2a…").
pub fn id(prefix: &str, key: &str) -> String {
    format!("{prefix}_{}", &store::sha(key)[..16])
}

pub fn company_id(name: &str) -> String {
    id("co", &company_key(name))
}

/// A company's own domain, when the address is not a shared host.
pub fn own_domain(url: &str) -> Option<String> {
    let host = reqwest::Url::parse(url).ok()?.host_str()?.to_lowercase();
    let domain = crate::career_search::company::registrable_domain(&host);
    (!SHARED_DOMAINS.contains(&domain.as_str())).then_some(domain)
}

/// A person's name for matching: case and accents folded, academic
/// titles dropped.
pub fn name_key(name: &str) -> String {
    let folded: String = name
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'ä' | 'á' | 'à' | 'â' | 'ã' | 'å' => 'a',
            'ö' | 'ó' | 'ò' | 'ô' | 'õ' | 'ø' => 'o',
            'ü' | 'ú' | 'ù' | 'û' => 'u',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ç' | 'č' | 'ć' => 'c',
            'š' | 'ś' => 's',
            'ž' | 'ź' | 'ż' => 'z',
            'ñ' | 'ń' => 'n',
            'ß' => 's',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    const TITLES: &[&str] = &[
        "dr", "mag", "prof", "ing", "dipl", "di", "msc", "bsc", "mba", "phd", "mr", "mrs", "ms",
        "dott",
    ];
    folded
        .split_whitespace()
        .filter(|w| !TITLES.contains(w))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn person_id(name: &str, company: Option<&str>) -> String {
    id(
        "pe",
        &format!(
            "{}|{}",
            name_key(name),
            company.map(company_key).unwrap_or_default()
        ),
    )
}

fn fill<T: Clone>(into: &mut Option<T>, from: &Option<T>) {
    if into.is_none() {
        into.clone_from(from);
    }
}

/// Adds a location unless one already listed names the same place: a bare
/// city and the same city with its country ("Vienna", "Vienna, Austria")
/// are one place, and the more specific name is kept. "Vienna, Austria"
/// and "Vienna, Virginia" stay two.
pub fn add_location(into: &mut Vec<String>, location: &str) {
    let location = location.trim();
    if location.is_empty() {
        return;
    }
    let lower = location.to_lowercase();
    let same_place = |kept: &str| {
        let kept = kept.to_lowercase();
        kept == lower
            || kept.starts_with(&format!("{lower},"))
            || lower.starts_with(&format!("{kept},"))
    };
    match into.iter().position(|kept| same_place(kept)) {
        Some(i) => {
            if location.len() > into[i].len() {
                into[i] = location.to_string();
            }
        }
        None => into.push(location.to_string()),
    }
}

/// Companies merged by name or own domain. Two companies with one name but
/// different own domains stay apart, each marked ambiguous.
pub fn merge_companies(companies: Vec<Company>) -> Vec<Company> {
    let mut out: Vec<Company> = Vec::new();
    for company in companies {
        let key = company_key(&company.name);
        let same = out.iter().position(|kept| {
            let same_name = company_key(&kept.name) == key
                || kept.aliases.iter().any(|a| company_key(a) == key);
            let conflicting_domains = matches!(
                (&kept.domain, &company.domain),
                (Some(a), Some(b)) if a != b
            );
            let same_domain = matches!(
                (&kept.domain, &company.domain),
                (Some(a), Some(b)) if a == b
            );
            let related = extends(&kept.name, &company.name)
                || kept.aliases.iter().any(|a| extends(a, &company.name));
            (same_domain && !related) || (same_name && !conflicting_domains)
        });
        match same {
            Some(i) => {
                let kept = &mut out[i];
                if company_key(&kept.name) != key
                    && !kept.aliases.iter().any(|a| company_key(a) == key)
                {
                    kept.aliases.push(company.name.clone());
                }
                fill(&mut kept.website, &company.website);
                fill(&mut kept.domain, &company.domain);
                fill(&mut kept.industry, &company.industry);
                fill(&mut kept.size, &company.size);
                fill(&mut kept.employees, &company.employees);
                fill(&mut kept.linkedin_url, &company.linkedin_url);
                fill(&mut kept.xing_url, &company.xing_url);
                for l in &company.locations {
                    add_location(&mut kept.locations, l);
                }
                for u in company.other_urls {
                    if !kept.other_urls.contains(&u) {
                        kept.other_urls.push(u);
                    }
                }
                for m in company.matched_because {
                    if !kept.matched_because.contains(&m) {
                        kept.matched_because.push(m);
                    }
                }
                kept.relevant_openings = kept.relevant_openings.max(company.relevant_openings);
                kept.last_verified_at = kept.last_verified_at.max(company.last_verified_at);
                evidence::merge(&mut kept.evidence, company.evidence);
            }
            None => {
                // The same name with another own domain: two companies.
                let twin = out.iter_mut().find(|kept| company_key(&kept.name) == key);
                let mut company = company;
                if let Some(twin) = twin {
                    let note = "another company with the same name was found".to_string();
                    if !twin.unverified.contains(&note) {
                        twin.unverified.push(note.clone());
                    }
                    company.unverified.push(note);
                    // Distinct ids for distinct companies.
                    company.id = id(
                        "co",
                        &format!("{key}|{}", company.domain.clone().unwrap_or_default()),
                    );
                }
                // A division or subsidiary on the same web domain: said,
                // not merged.
                if let Some(relative) = out
                    .iter_mut()
                    .find(|kept| kept.domain.is_some() && kept.domain == company.domain)
                {
                    let apart = |other: &str| {
                        format!(
                            "shares its web domain with {other}; kept apart as a possible \
                             subsidiary or division"
                        )
                    };
                    let note = apart(&company.name);
                    if !relative.unverified.contains(&note) {
                        relative.unverified.push(note);
                    }
                    company.unverified.push(apart(&relative.name));
                }
                out.push(company);
            }
        }
    }
    out
}

/// Whether one company name extends the other word for word ("Siemens" and
/// "Siemens Mobility"): on one web domain, more likely a division or a
/// subsidiary than the same company.
fn extends(a: &str, b: &str) -> bool {
    let (a, b) = (company_key(a), company_key(b));
    let a: Vec<&str> = a.split_whitespace().collect();
    let b: Vec<&str> = b.split_whitespace().collect();
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    !short.is_empty() && long.len() > short.len() && long[..short.len()] == short[..]
}

fn compatible_titles(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            let (a, b) = (a.to_lowercase(), b.to_lowercase());
            a == b || a.contains(&b) || b.contains(&a)
        }
        _ => true,
    }
}

/// People merged only on the same name at the same company with a
/// compatible title or the same profile. Same name, different titles:
/// both kept, marked as possibly the same person.
pub fn merge_people(people: Vec<Person>) -> Vec<Person> {
    let mut out: Vec<Person> = Vec::new();
    for person in people {
        let key = name_key(&person.name);
        let company = person.company_name.as_deref().map(company_key);
        let same = out.iter().position(|kept| {
            let same_profile = matches!(
                (&kept.linkedin_url, &person.linkedin_url),
                (Some(a), Some(b)) if normalize::canonical_url(a) == normalize::canonical_url(b)
            );
            same_profile
                || (name_key(&kept.name) == key
                    && kept.company_name.as_deref().map(company_key) == company
                    && company.is_some()
                    && compatible_titles(kept.title.as_deref(), person.title.as_deref()))
        });
        match same {
            Some(i) => {
                let kept = &mut out[i];
                // The stronger relevance and confidence win, with their reason.
                if (person.relevance, std::cmp::Reverse(person.confidence))
                    < (kept.relevance, std::cmp::Reverse(kept.confidence))
                {
                    kept.relevance = person.relevance;
                    kept.relevance_reason = person.relevance_reason.clone();
                    kept.job_id = person.job_id.clone().or(kept.job_id.take());
                }
                kept.confidence = kept.confidence.max(person.confidence);
                if kept.title.as_ref().map(String::len).unwrap_or(0)
                    < person.title.as_ref().map(String::len).unwrap_or(0)
                {
                    kept.title.clone_from(&person.title);
                }
                fill(&mut kept.linkedin_url, &person.linkedin_url);
                fill(&mut kept.xing_url, &person.xing_url);
                fill(&mut kept.other_url, &person.other_url);
                fill(&mut kept.location, &person.location);
                fill(&mut kept.job_id, &person.job_id);
                fill(&mut kept.relationship, &person.relationship);
                evidence::merge(&mut kept.evidence, person.evidence);
            }
            None => {
                let mut person = person;
                let twins: Vec<usize> = out
                    .iter()
                    .enumerate()
                    .filter(|(_, kept)| {
                        name_key(&kept.name) == key
                            && kept.company_name.as_deref().map(company_key) == company
                    })
                    .map(|(i, _)| i)
                    .collect();
                if !twins.is_empty() {
                    let note = "Another entry has this name with a different title; it may or \
                                may not be the same person."
                        .to_string();
                    for i in twins {
                        out[i].caveat.get_or_insert_with(|| note.clone());
                    }
                    person.caveat.get_or_insert(note);
                    person.id = id(
                        "pe",
                        &format!("{}|{}", person.id, person.title.clone().unwrap_or_default()),
                    );
                }
                out.push(person);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::{
        model::{Confidence, RelevanceType},
        policy::{DataClass, DataSource, Persistence},
    };

    pub(crate) fn company(name: &str, domain: Option<&str>) -> Company {
        Company {
            id: company_id(name),
            name: name.into(),
            aliases: vec![],
            website: domain.map(|d| format!("https://{d}")),
            domain: domain.map(str::to_string),
            industry: None,
            size: None,
            employees: None,
            locations: vec![],
            linkedin_url: None,
            xing_url: None,
            other_urls: vec![],
            matched_because: vec![],
            unverified: vec![],
            relevant_openings: 0,
            evidence: vec![],
            last_verified_at: 1,
        }
    }

    pub(crate) fn person(name: &str, title: Option<&str>, company: &str) -> Person {
        Person {
            id: person_id(name, Some(company)),
            name: name.into(),
            title: title.map(str::to_string),
            company_id: Some(company_id(company)),
            company_name: Some(company.into()),
            location: None,
            linkedin_url: None,
            xing_url: None,
            other_url: None,
            relevance: RelevanceType::RelevantContact,
            relevance_reason: "x".into(),
            job_id: None,
            confidence: Confidence::Low,
            evidence: vec![],
            relationship: None,
            source: DataSource::CompanyWebsite,
            class: DataClass::ProfessionalProfile,
            persistence: Persistence::PersistentPermitted,
            fetched_at: 1,
            caveat: None,
        }
    }

    #[test]
    fn companies_merge_by_name_or_domain_and_same_names_stay_apart() {
        let mut a = company("Nordlicht AI GmbH", Some("nordlicht.example"));
        a.relevant_openings = 2;
        let mut b = company("Nordlicht AI", None);
        b.industry = Some("AI".into());
        let c = company("NLAI", Some("nordlicht.example"));
        let merged = merge_companies(vec![a, b, c]);
        assert_eq!(merged.len(), 1, "{merged:#?}");
        assert_eq!(merged[0].industry.as_deref(), Some("AI"));
        assert_eq!(merged[0].aliases, ["NLAI"]);
        assert_eq!(merged[0].relevant_openings, 2);

        // Two companies called "Atlas", different own domains.
        let merged = merge_companies(vec![
            company("Atlas", Some("atlas-energy.example")),
            company("Atlas", Some("atlas-bank.example")),
        ]);
        assert_eq!(merged.len(), 2);
        assert_ne!(merged[0].id, merged[1].id);
        assert!(merged.iter().all(|c| !c.unverified.is_empty()));

        // A division or subsidiary on the parent's web domain stays apart,
        // and both say why.
        let merged = merge_companies(vec![
            company("Siemens AG", Some("siemens.example")),
            company("Siemens Mobility GmbH", Some("siemens.example")),
        ]);
        assert_eq!(merged.len(), 2, "{merged:#?}");
        assert_ne!(merged[0].id, merged[1].id);
        assert!(merged[0].unverified[0].contains("Siemens Mobility GmbH"));
        assert!(merged[1].unverified[0].contains("possible subsidiary or division"));
        assert_eq!(own_domain("https://jobs.lever.co/atlas"), None);
        assert_eq!(
            own_domain("https://careers.atlas-bank.example/x").as_deref(),
            Some("atlas-bank.example")
        );
    }

    #[test]
    fn a_city_and_the_same_city_with_its_country_are_one_location() {
        let mut locations = Vec::new();
        for l in [
            "Vienna",
            "Vienna, Austria",
            "vienna",
            "Graz",
            "Vienna, Virginia",
        ] {
            add_location(&mut locations, l);
        }
        assert_eq!(locations, ["Vienna, Austria", "Graz", "Vienna, Virginia"]);
        let mut a = company("Nordlicht AI", None);
        a.locations = vec!["Vienna, Austria".into()];
        let mut b = company("Nordlicht AI", None);
        b.locations = vec!["Vienna".into()];
        assert_eq!(
            merge_companies(vec![a, b])[0].locations,
            ["Vienna, Austria"]
        );
    }

    #[test]
    fn people_are_never_merged_on_a_name_alone() {
        let mut named = person(
            "Anna Beispiel",
            Some("Head of Talent Acquisition"),
            "Nordlicht AI",
        );
        named.relevance = RelevanceType::NamedRecruiter;
        named.confidence = Confidence::High;
        let again = person(
            "Dr. Anna Beispiel",
            Some("Head of Talent Acquisition"),
            "Nordlicht AI",
        );
        let elsewhere = person("Anna Beispiel", Some("Recruiter"), "Donau Data");
        let other_title = person(
            "Anna Beispiel",
            Some("Chief Financial Officer"),
            "Nordlicht AI",
        );
        let merged = merge_people(vec![named, again, elsewhere, other_title]);
        assert_eq!(merged.len(), 3, "{merged:#?}");
        assert_eq!(merged[0].relevance, RelevanceType::NamedRecruiter);
        assert_eq!(merged[0].confidence, Confidence::High);
        assert!(
            merged[1].caveat.is_none(),
            "another company is another person"
        );
        assert!(merged[2].caveat.is_some() && merged[0].caveat.is_some());
        assert_ne!(merged[0].id, merged[2].id);
        assert_eq!(name_key("Dr. Jürgen  Müller-Weiß"), "jurgen muller weis");
    }
}
