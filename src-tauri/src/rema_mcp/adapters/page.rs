//! Employer career pages: one GET of a permitted page (robots.txt honored,
//! public addresses only, no scripts). A schema.org `JobPosting` in JSON-LD
//! is preferred; structured markup is evidence, not a guarantee. Pages
//! without it are accepted only when their text reads like one vacancy.

use serde_json::Value;

use super::{blank, evidence, fetch_error, finish_common, source_id, Ctx};
use crate::{
    analytics::{normalize, page as html_page},
    rema_mcp::{
        contract::{
            AcquisitionMode, Availability, DatePrecision, ErrorCode, JobRecord, SectionKind,
            ToolError, WorkMode,
        },
        extract,
        fetch::{Accept, FetchError, PAGE_LIMIT},
    },
};

const SOURCE: &str = "web";

pub async fn read(ctx: &Ctx<'_>, url: &str, title_hint: &str) -> Result<JobRecord, ToolError> {
    match ctx
        .fetcher
        .robots_allow(url, ctx.deadline, ctx.cancel)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            return Err(ToolError::new(
                ErrorCode::BlockedBySourcePolicy,
                "the site's robots.txt does not allow reading this page",
            ))
        }
        Err(error) => return Err(fetch_error(error)),
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
        .await
        .map_err(|e| match e {
            FetchError::Gone(code) => ToolError::new(
                ErrorCode::JobNotFound,
                format!("the page is no longer online ({code})"),
            ),
            other => fetch_error(other),
        })?;
    let html = response.body.unwrap_or_default();
    let mut record = parse(&html, &response.final_url, title_hint, ctx.now)?;
    if response.truncated {
        record.description.truncated = true;
    }
    Ok(record)
}

fn text(value: Option<&Value>) -> Option<String> {
    html_page::text_of(value)
        .map(|t| html_page::decode_entities(&t).trim().to_string())
        .filter(|t| !t.is_empty())
}

struct Address {
    label: String,
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
}

fn addresses(posting: &Value) -> Vec<Address> {
    let locations = match posting.get("jobLocation") {
        Some(Value::Array(items)) => items.clone(),
        Some(other) => vec![other.clone()],
        None => Vec::new(),
    };
    locations
        .iter()
        .filter_map(|loc| {
            let address = loc.get("address").unwrap_or(loc);
            let city = text(address.get("addressLocality"));
            let region = text(address.get("addressRegion"));
            let country = text(address.get("addressCountry"));
            let label = [city.clone(), region.clone(), country.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ");
            (!label.is_empty()).then_some(Address {
                label,
                city,
                region,
                country,
            })
        })
        .collect()
}

fn title_words(text: &str) -> Vec<String> {
    extract::normalized_title(text)
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// The page's vacancy: the only `JobPosting`, or the one whose URL or title
/// matches. A page listing several vacancies is not one job.
fn choose<'a>(
    postings: &'a [Value],
    url: &str,
    title_hint: &str,
) -> Result<Option<&'a Value>, ToolError> {
    match postings {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        many => {
            let key = normalize::canonical_url(url);
            if let Some(p) = many.iter().find(|p| {
                text(p.get("url")).and_then(|u| normalize::canonical_url(&u)) == key
                    && key.is_some()
            }) {
                return Ok(Some(p));
            }
            let wanted = title_words(title_hint);
            let exact: Vec<&Value> = many
                .iter()
                .filter(|p| {
                    !wanted.is_empty()
                        && title_words(&text(p.get("title")).unwrap_or_default()) == wanted
                })
                .collect();
            match exact.as_slice() {
                [one] => Ok(Some(one)),
                _ => Err(ToolError::new(
                    ErrorCode::NotAJobPosting,
                    "the page lists several vacancies, not one job",
                )),
            }
        }
    }
}

pub fn parse(html: &str, url: &str, title_hint: &str, now: i64) -> Result<JobRecord, ToolError> {
    let mut found = Vec::new();
    for block in html_page::ld_blocks(html) {
        html_page::postings(&block, &mut found);
    }
    let body_start = html.to_ascii_lowercase().find("<body").unwrap_or(0);
    let (page_text, _) = extract::clean_html(&html[body_start..]);
    let mut record = blank(AcquisitionMode::PermittedPublicPage);
    record.links.canonical_url = Some(url.to_string());
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_string())
        })
        .unwrap_or_default();

    let Some(posting) = choose(&found, url, title_hint)? else {
        return page_text_record(record, &page_text, html, url, title_hint, now);
    };
    let path = |field: &str| format!("JobPosting.{field}");
    let title = text(posting.get("title"))
        .ok_or_else(|| ToolError::new(ErrorCode::ParsingFailed, "the JobPosting has no title"))?;
    record.title = extract::clip(&title, 200);
    evidence(
        &mut record,
        "title",
        SOURCE,
        url,
        "json_ld",
        Some(&path("title")),
        now,
    );
    match posting.get("hiringOrganization") {
        Some(Value::Object(org)) => {
            record.employer.name = text(org.get("name"));
            record.employer.website = text(org.get("sameAs")).or_else(|| text(org.get("url")));
        }
        other => record.employer.name = text(other),
    }
    if record.employer.name.is_some() {
        evidence(
            &mut record,
            "employer",
            SOURCE,
            url,
            "json_ld",
            Some(&path("hiringOrganization")),
            now,
        );
    }
    record.locations = addresses(posting)
        .into_iter()
        .map(|a| {
            let mut loc = extract::location(&a.label);
            loc.city = loc.city.or(a.city);
            loc.region = a.region;
            loc.country = loc.country.or_else(|| {
                a.country
                    .as_deref()
                    .and_then(normalize::country_name)
                    .map(str::to_string)
            });
            loc
        })
        .collect();
    if text(posting.get("jobLocationType"))
        .is_some_and(|t| t.to_uppercase().contains("TELECOMMUTE"))
    {
        record.work_mode = Some(WorkMode::Remote);
        evidence(
            &mut record,
            "work_mode",
            SOURCE,
            url,
            "json_ld",
            Some(&path("jobLocationType")),
            now,
        );
    }
    // Who may apply from where: kept as stated, separate from office places.
    let requirements = match posting.get("applicantLocationRequirements") {
        Some(Value::Array(items)) => items.iter().filter_map(|v| text(Some(v))).collect(),
        Some(other) => text(Some(other)).into_iter().collect(),
        None => Vec::new(),
    };
    record.remote_eligibility = requirements;
    if let Some(kind) = text(posting.get("employmentType")) {
        (record.employment_type, record.working_time) = extract::employment(&kind);
    }
    if let Some((salary, _)) = posting.get("baseSalary").and_then(html_page::salary_of) {
        record.compensation = extract::compensation_structured(
            salary.min,
            salary.max,
            salary.currency,
            salary.period.map(extract::period),
            None,
        );
        evidence(
            &mut record,
            "compensation",
            SOURCE,
            url,
            "json_ld",
            Some(&path("baseSalary")),
            now,
        );
    }
    if let Some((day, _)) = text(posting.get("datePosted"))
        .as_deref()
        .and_then(extract::date)
    {
        record.dates.posted_at = Some(day);
        record.dates.posted_precision = Some(DatePrecision::Day);
        evidence(
            &mut record,
            "dates.posted_at",
            SOURCE,
            url,
            "json_ld",
            Some(&path("datePosted")),
            now,
        );
    }
    record.dates.valid_through = text(posting.get("validThrough"))
        .as_deref()
        .and_then(extract::date)
        .map(|(d, _)| d);
    if let Some(reference) = text(posting.get("identifier")) {
        record.source_ids.push(source_id(
            "employer_ref",
            &format!("{host}:{}", extract::clip(&reference, 60)),
        ));
    }
    let description_html = text(posting.get("description")).unwrap_or_default();
    let (description, truncated) = extract::clean_html(&description_html);
    let full = description.chars().count() >= 400;
    record.description =
        extract::description(Some(description).filter(|d| !d.is_empty()), truncated, full);
    if record.description.text.is_some() {
        evidence(
            &mut record,
            "description",
            SOURCE,
            url,
            "json_ld",
            Some(&path("description")),
            now,
        );
    }
    record.language = text(posting.get("inLanguage"))
        .map(|l| l.chars().take(2).collect::<String>().to_lowercase());

    let today = extract::iso_date(now);
    let (availability, basis) = if extract::says_closed(&page_text) {
        (
            Availability::Closed,
            "the employer's page says the position is closed".to_string(),
        )
    } else if let Some(until) = record.dates.valid_through.clone() {
        if until < today {
            (
                Availability::Closed,
                format!("validThrough {until} has passed"),
            )
        } else {
            (
                Availability::Active,
                format!("the employer's page states it is open until {until}"),
            )
        }
    } else {
        (
            Availability::Unknown,
            "the posting page is online but states no closing date; that alone does not prove \
             it is open"
                .to_string(),
        )
    };
    record.quality.availability = availability;
    record.quality.availability_basis = Some(basis);
    record.dates.last_checked_at = Some(extract::iso(now));
    finish_common(&mut record, SOURCE, url, now);
    Ok(record)
}

/// A page without `JobPosting` markup: accepted only when its text reads
/// like one vacancy (requirement or task sections, or an apply prompt).
fn page_text_record(
    mut record: JobRecord,
    page_text: &str,
    html: &str,
    url: &str,
    title_hint: &str,
    now: i64,
) -> Result<JobRecord, ToolError> {
    let sections = extract::sections(page_text);
    let job_sections = sections
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                SectionKind::Requirements | SectionKind::Responsibilities
            )
        })
        .count();
    let lower = page_text.to_lowercase();
    let apply = ["apply now", "jetzt bewerben", "aplikuj", "bewirb dich"]
        .iter()
        .any(|w| lower.contains(w));
    if job_sections == 0 && !apply {
        return Err(ToolError::new(
            ErrorCode::NotAJobPosting,
            "the page does not look like a single job posting",
        ));
    }
    let title = html_page::page_title(html)
        .map(|t| {
            t.split(['|', '–', '—'])
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| title_hint.to_string());
    if title.is_empty() {
        return Err(ToolError::new(
            ErrorCode::ParsingFailed,
            "the page has no title",
        ));
    }
    record.title = extract::clip(&title, 200);
    evidence(
        &mut record,
        "title",
        SOURCE,
        url,
        "page_text",
        Some("title"),
        now,
    );
    record.description = extract::description(Some(page_text.to_string()), false, false);
    record.quality.availability = if extract::says_closed(page_text) {
        Availability::Closed
    } else {
        Availability::Unknown
    };
    record.quality.availability_basis =
        Some(if record.quality.availability == Availability::Closed {
            "the page says the position is closed".into()
        } else {
            "read from page text without structured job data".into()
        });
    record.dates.last_checked_at = Some(extract::iso(now));
    finish_common(&mut record, SOURCE, url, now);
    Ok(record)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::rema_mcp::contract::{DescriptionState, SalaryBound, SalaryPeriod};

    pub fn posting_page(
        title: &str,
        posted: &str,
        valid_through: Option<&str>,
        extra_body: &str,
    ) -> String {
        let valid = valid_through
            .map(|v| format!(r#","validThrough":"{v}""#))
            .unwrap_or_default();
        format!(
            r#"<html><head><title>{title} – Nordlicht AI</title>
<script type="application/ld+json">{{"@context":"https://schema.org","@graph":[{{"@type":"Organization","name":"Nordlicht AI"}},{{"@type":"JobPosting","title":"{title}","hiringOrganization":{{"@type":"Organization","name":"Nordlicht AI","sameAs":"https://nordlicht.example"}},"datePosted":"{posted}"{valid},"employmentType":["FULL_TIME"],"jobLocation":[{{"@type":"Place","address":{{"addressLocality":"Wien","addressCountry":"AT"}}}}],"applicantLocationRequirements":{{"@type":"Country","name":"Austria"}},"baseSalary":{{"@type":"MonetaryAmount","currency":"EUR","value":{{"@type":"QuantitativeValue","minValue":65000,"unitText":"YEAR"}}}},"identifier":{{"@type":"PropertyValue","value":"REQ-17"}},"description":"&lt;p&gt;We build LLM products.&lt;/p&gt;&lt;h3&gt;Requirements&lt;/h3&gt;&lt;ul&gt;&lt;li&gt;Python&lt;/li&gt;&lt;/ul&gt;"}}]}}</script>
</head><body><h1>{title}</h1>{extra_body}</body></html>"#
        )
    }

    #[test]
    fn reads_the_job_posting_markup() {
        let html = posting_page("AI Engineer", "2026-09-20", Some("2026-12-31"), "");
        let job = parse(
            &html,
            "https://careers.nordlicht.example/jobs/17",
            "",
            1_790_380_800_000,
        )
        .unwrap();
        assert_eq!(job.title, "AI Engineer");
        assert_eq!(job.employer.name.as_deref(), Some("Nordlicht AI"));
        assert_eq!(job.locations[0].city.as_deref(), Some("Vienna"));
        assert_eq!(job.locations[0].country.as_deref(), Some("Austria"));
        assert_eq!(job.remote_eligibility, ["Austria"]);
        let pay = job.compensation.as_ref().unwrap();
        assert_eq!(
            (pay.min, pay.max),
            (Some(65_000.0), None),
            "minValue only: a floor"
        );
        assert_eq!(pay.bound, Some(SalaryBound::Floor));
        assert_eq!(pay.period, Some(SalaryPeriod::Year));
        assert_eq!(job.quality.availability, Availability::Active);
        assert_eq!(
            job.description.state,
            DescriptionState::Partial,
            "a short description"
        );
        assert_eq!(job.source_ids[0].id, "careers.nordlicht.example:REQ-17");
    }

    #[test]
    fn closed_and_expired_postings_are_closed_and_http_200_alone_is_not_active() {
        let closed = posting_page(
            "AI Engineer",
            "2026-09-20",
            None,
            "<p>This job has expired.</p>",
        );
        let job = parse(&closed, "https://x.example/jobs/1", "", 1_790_380_800_000).unwrap();
        assert_eq!(job.quality.availability, Availability::Closed);
        let expired = posting_page("AI Engineer", "2026-08-01", Some("2026-09-01"), "");
        let job = parse(&expired, "https://x.example/jobs/1", "", 1_790_380_800_000).unwrap();
        assert_eq!(job.quality.availability, Availability::Closed);
        let open = posting_page("AI Engineer", "2026-09-20", None, "");
        let job = parse(&open, "https://x.example/jobs/1", "", 1_790_380_800_000).unwrap();
        assert_eq!(job.quality.availability, Availability::Unknown);
    }

    #[test]
    fn pages_that_are_not_one_vacancy_are_rejected() {
        let marketing = "<html><head><title>About Nordlicht</title></head><body><p>We are a company.</p></body></html>";
        assert_eq!(
            parse(marketing, "https://x.example/about", "", 0)
                .unwrap_err()
                .code,
            ErrorCode::NotAJobPosting
        );
        let listing = format!(
            "<html><head><script type=\"application/ld+json\">[{},{}]</script></head><body></body></html>",
            r#"{"@type":"JobPosting","title":"AI Engineer","url":"https://x.example/jobs/1"}"#,
            r#"{"@type":"JobPosting","title":"Data Engineer","url":"https://x.example/jobs/2"}"#
        );
        assert_eq!(
            parse(&listing, "https://x.example/jobs", "", 0)
                .unwrap_err()
                .code,
            ErrorCode::NotAJobPosting
        );
        // …but the right one is chosen on its own URL.
        let job = parse(&listing, "https://x.example/jobs/2", "", 0).unwrap();
        assert_eq!(job.title, "Data Engineer");
        let text_page = "<html><head><title>ML Engineer | Donau</title></head><body><h2>Your tasks</h2><ul><li>Train models</li></ul><h2>Requirements</h2><ul><li>Python</li></ul><a>Apply now</a></body></html>";
        let job = parse(text_page, "https://donau.example/jobs/ml", "", 0).unwrap();
        assert_eq!(job.title, "ML Engineer");
        assert_eq!(job.quality.availability, Availability::Unknown);
        assert_eq!(job.description.state, DescriptionState::Partial);
    }
}
