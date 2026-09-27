//! Workable's public jobs widget: `GET /api/v1/widget/accounts/{account}` on
//! `apply.workable.com` — an employer's published jobs, as its careers-page
//! widget shows them (no authentication).

use serde_json::Value;

use super::{blank, evidence, finish_common, source_id, Ctx};
use crate::{
    analytics::normalize,
    rema_mcp::{
        contract::{
            AcquisitionMode, Availability, DatePrecision, ErrorCode, JobRecord, ToolError, WorkMode,
        },
        extract,
        fetch::Accept,
    },
};

const SOURCE: &str = "workable";

fn text(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Every published job of an account.
pub async fn list(ctx: &Ctx<'_>, account: &str) -> Result<Vec<JobRecord>, ToolError> {
    let url = format!(
        "{}/api/v1/widget/accounts/{account}?details=true",
        ctx.apis.workable
    );
    let body = ctx.feed(&url, Accept::Json).await?;
    let value: Value = serde_json::from_str(&body)
        .map_err(|_| ToolError::new(ErrorCode::ParsingFailed, "the Workable list is unreadable"))?;
    Ok(parse_list(&value, account, &url, ctx.now))
}

pub fn parse_list(value: &Value, account: &str, api_url: &str, now: i64) -> Vec<JobRecord> {
    let employer = text(value, "/name");
    value
        .get("jobs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(super::MAX_LISTED)
        .filter_map(|job| parse(job, account, employer.as_deref(), api_url, now))
        .collect()
}

fn parse(
    job: &Value,
    account: &str,
    employer: Option<&str>,
    api_url: &str,
    now: i64,
) -> Option<JobRecord> {
    let code = text(job, "/shortcode").or_else(|| text(job, "/code"))?;
    let title = text(job, "/title")?;
    let mut record = blank(AcquisitionMode::DocumentedPublicFeed);
    record.title = extract::clip(&title, 200);
    record
        .source_ids
        .push(source_id(SOURCE, &format!("{account}:{code}")));
    evidence(
        &mut record,
        "title",
        SOURCE,
        api_url,
        "ats_feed",
        Some("jobs[].title"),
        now,
    );
    record.employer.name = employer.map(str::to_string);

    let mut places: Vec<String> = job
        .get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|l| l.get("hidden").and_then(Value::as_bool) != Some(true))
        .filter_map(|l| {
            let parts: Vec<String> = ["/city", "/region", "/country"]
                .iter()
                .filter_map(|p| text(l, p))
                .collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        })
        .collect();
    if places.is_empty() {
        let parts: Vec<String> = ["/city", "/state", "/country"]
            .iter()
            .filter_map(|p| text(job, p))
            .collect();
        if !parts.is_empty() {
            places.push(parts.join(", "));
        }
    }
    record.locations = places.iter().map(|p| extract::location(p)).collect();
    if let (Some(first), Some(code)) = (
        record.locations.first_mut(),
        text(job, "/locations/0/countryCode"),
    ) {
        if first.country.is_none() {
            first.country = normalize::country_name(&code.to_uppercase()).map(str::to_string);
        }
    }
    if job.get("telecommuting").and_then(Value::as_bool) == Some(true) {
        record.work_mode = Some(WorkMode::Remote);
    }
    if let Some(terms) = text(job, "/employment_type") {
        (record.employment_type, record.working_time) = extract::employment(&terms);
    }
    let html = text(job, "/description").unwrap_or_default();
    let (description, truncated) = extract::clean_html(&html);
    record.description =
        extract::description(Some(description).filter(|d| !d.is_empty()), truncated, true);
    let published = text(job, "/published_on").or_else(|| text(job, "/created_at"));
    if let Some((day, _)) = published.as_deref().and_then(extract::date) {
        record.dates.posted_at = Some(day);
        record.dates.posted_precision = Some(DatePrecision::Day);
        evidence(
            &mut record,
            "dates.posted_at",
            SOURCE,
            api_url,
            "ats_feed",
            Some("published_on"),
            now,
        );
    }
    record.links.canonical_url = text(job, "/url")
        .or_else(|| text(job, "/shortlink"))
        .or_else(|| Some(format!("https://apply.workable.com/{account}/j/{code}/")));
    record.links.apply_url = text(job, "/application_url");
    record.quality.availability = Availability::Active;
    record.quality.availability_basis =
        Some("published on the employer's Workable careers page".into());
    record.dates.last_checked_at = Some(extract::iso(now));
    finish_common(&mut record, SOURCE, api_url, now);
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_accounts_jobs() {
        let value = serde_json::json!({
            "name": "Donau Data",
            "jobs": [{
                "title": "Machine Learning Engineer", "shortcode": "AB12CD34",
                "employment_type": "Full-time", "telecommuting": false,
                "url": "https://apply.workable.com/j/AB12CD34",
                "published_on": "2026-09-21",
                "locations": [{ "country": "Austria", "countryCode": "AT", "city": "Vienna", "region": "Vienna", "hidden": false }],
                "description": "<p>Build models.</p><h3>Requirements</h3><ul><li>Python</li></ul>"
            }]
        });
        let jobs = parse_list(&value, "donau-data", "https://api/x", 1_790_380_800_000);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].employer.name.as_deref(), Some("Donau Data"));
        assert_eq!(jobs[0].locations[0].city.as_deref(), Some("Vienna"));
        assert_eq!(jobs[0].dates.posted_at.as_deref(), Some("2026-09-21"));
        assert_eq!(jobs[0].description.requirements, ["Python"]);
        assert_eq!(
            jobs[0].links.canonical_url.as_deref(),
            Some("https://apply.workable.com/j/AB12CD34")
        );
    }
}
