//! SmartRecruiters Posting API: `GET /v1/companies/{company}/postings` —
//! an employer's published postings (public, no authentication). The list
//! carries titles, places, dates and terms; descriptions are on the
//! posting page, read like any permitted page when needed.

use serde_json::Value;

use super::{blank, evidence, finish_common, source_id, Ctx};
use crate::{
    analytics::normalize,
    rema_mcp::{
        contract::{
            AcquisitionMode, Availability, DatePrecision, ErrorCode, JobRecord, Location,
            ToolError, WorkMode,
        },
        extract,
        fetch::Accept,
    },
};

const SOURCE: &str = "smartrecruiters";

fn text(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Every published posting of a company.
pub async fn list(ctx: &Ctx<'_>, company: &str) -> Result<Vec<JobRecord>, ToolError> {
    let url = format!(
        "{}/v1/companies/{company}/postings?limit=100",
        ctx.apis.smartrecruiters
    );
    let body = ctx.feed(&url, Accept::Json).await?;
    let value: Value = serde_json::from_str(&body).map_err(|_| {
        ToolError::new(
            ErrorCode::ParsingFailed,
            "the SmartRecruiters list is unreadable",
        )
    })?;
    Ok(parse_list(&value, company, &url, ctx.now))
}

pub fn parse_list(value: &Value, company: &str, api_url: &str, now: i64) -> Vec<JobRecord> {
    value
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(super::MAX_LISTED)
        .filter_map(|posting| parse(posting, company, api_url, now))
        .collect()
}

fn parse(posting: &Value, company: &str, api_url: &str, now: i64) -> Option<JobRecord> {
    let id = text(posting, "/id")?;
    let title = text(posting, "/name")?;
    let mut record = blank(AcquisitionMode::DocumentedPublicFeed);
    record.title = extract::clip(&title, 200);
    record
        .source_ids
        .push(source_id(SOURCE, &format!("{company}:{id}")));
    evidence(
        &mut record,
        "title",
        SOURCE,
        api_url,
        "ats_api",
        Some("name"),
        now,
    );
    record.employer.name = text(posting, "/company/name");

    let city = text(posting, "/location/city");
    let region = text(posting, "/location/region");
    let country = text(posting, "/location/country")
        .and_then(|c| normalize::country_name(&c.to_uppercase()).map(str::to_string));
    let place = [city.clone(), region.clone(), country.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    if !place.is_empty() {
        let mut location = extract::location(&place);
        if location.city.is_none() {
            location.city = city
                .as_deref()
                .and_then(normalize::city_name)
                .map(str::to_string);
        }
        if location.country.is_none() {
            location.country = country;
        }
        record.locations = vec![Location {
            text: place,
            ..location
        }];
    }
    let remote = posting.pointer("/location/remote").and_then(Value::as_bool) == Some(true);
    let hybrid = posting.pointer("/location/hybrid").and_then(Value::as_bool) == Some(true);
    record.work_mode = if remote {
        Some(WorkMode::Remote)
    } else if hybrid {
        Some(WorkMode::Hybrid)
    } else {
        None
    };
    if let Some(terms) = text(posting, "/typeOfEmployment/label") {
        (record.employment_type, record.working_time) = extract::employment(&terms);
    }
    if let Some((day, _)) = text(posting, "/releasedDate")
        .as_deref()
        .and_then(extract::date)
    {
        record.dates.posted_at = Some(day);
        record.dates.posted_precision = Some(DatePrecision::Day);
        evidence(
            &mut record,
            "dates.posted_at",
            SOURCE,
            api_url,
            "ats_api",
            Some("releasedDate"),
            now,
        );
    }
    let identifier = text(posting, "/company/identifier").unwrap_or_else(|| company.to_string());
    record.links.canonical_url = Some(format!(
        "https://jobs.smartrecruiters.com/{identifier}/{id}"
    ));
    record.description = extract::description(None, false, false);
    record.quality.missing.push("description".into());
    record.quality.availability = Availability::Active;
    record.quality.availability_basis =
        Some("published on the employer's SmartRecruiters career site (Posting API)".into());
    record.dates.last_checked_at = Some(extract::iso(now));
    finish_common(&mut record, SOURCE, api_url, now);
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rema_mcp::contract::{EmploymentType, WorkingTime};

    #[test]
    fn reads_a_companys_postings() {
        let value = serde_json::json!({
            "offset": 0, "limit": 100, "totalFound": 2,
            "content": [
                { "id": "744000099", "name": "Senior AI Engineer",
                  "company": { "identifier": "NordlichtAI", "name": "Nordlicht AI" },
                  "releasedDate": "2026-09-20T09:00:00.000Z",
                  "location": { "city": "Vienna", "region": "Wien", "country": "at", "remote": false, "hybrid": true },
                  "typeOfEmployment": { "label": "Full-time" } },
                { "name": "no id" }
            ]
        });
        let jobs = parse_list(&value, "NordlichtAI", "https://api/x", 1_790_380_800_000);
        assert_eq!(jobs.len(), 1);
        let job = &jobs[0];
        assert_eq!(job.employer.name.as_deref(), Some("Nordlicht AI"));
        assert_eq!(job.locations[0].city.as_deref(), Some("Vienna"));
        assert_eq!(job.locations[0].country.as_deref(), Some("Austria"));
        assert_eq!(job.work_mode, Some(WorkMode::Hybrid));
        assert_eq!(job.working_time, Some(WorkingTime::FullTime));
        assert_eq!(job.employment_type, None::<EmploymentType>);
        assert_eq!(job.dates.posted_at.as_deref(), Some("2026-09-20"));
        assert_eq!(
            job.links.canonical_url.as_deref(),
            Some("https://jobs.smartrecruiters.com/NordlichtAI/744000099")
        );
        assert_eq!(job.source_ids[0].id, "NordlichtAI:744000099");
    }
}
