//! Ashby public Job Postings API: `GET /posting-api/job-board/{board}` (with
//! compensation). The board is read once per 10 minutes and shared by all
//! its jobs; unlisted jobs are not returned.

use serde_json::Value;

use super::{blank, evidence, finish_common, source_id, Ctx};
use crate::rema_mcp::{
    contract::{
        AcquisitionMode, Availability, DatePrecision, ErrorCode, JobRecord, SalaryPeriod,
        ToolError, WorkMode,
    },
    extract,
    fetch::Accept,
};

const SOURCE: &str = "ashby";

pub async fn read(ctx: &Ctx<'_>, board: &str, id: &str) -> Result<JobRecord, ToolError> {
    let url = format!(
        "{}/posting-api/job-board/{board}?includeCompensation=true",
        ctx.apis.ashby
    );
    let body = ctx.feed(&url, Accept::Json).await?;
    let value: Value = serde_json::from_str(&body)
        .map_err(|_| ToolError::new(ErrorCode::ParsingFailed, "the Ashby board is unreadable"))?;
    let job = value
        .get("jobs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|j| j.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| {
            ToolError::new(
                ErrorCode::JobNotFound,
                "the employer's Ashby board no longer lists this job",
            )
        })?;
    parse(job, board, &url, ctx.now)
}

fn text(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn interval(text: &str) -> Option<SalaryPeriod> {
    let lower = text.to_lowercase();
    if !lower.starts_with("1 ") {
        return None;
    }
    extract::period_word(lower.trim_start_matches("1 "))
}

pub fn parse(job: &Value, board: &str, api_url: &str, now: i64) -> Result<JobRecord, ToolError> {
    let id = text(job, "/id")
        .ok_or_else(|| ToolError::new(ErrorCode::ParsingFailed, "the job has no id"))?;
    let title = text(job, "/title")
        .ok_or_else(|| ToolError::new(ErrorCode::ParsingFailed, "the job has no title"))?;
    let mut record = blank(AcquisitionMode::DocumentedPublicFeed);
    record.title = extract::clip(&title, 200);
    record
        .source_ids
        .push(source_id(SOURCE, &format!("{board}:{id}")));
    evidence(
        &mut record,
        "title",
        SOURCE,
        api_url,
        "ats_feed",
        Some("jobs[].title"),
        now,
    );

    let mut places: Vec<String> = text(job, "/location").into_iter().collect();
    for extra in job
        .get("secondaryLocations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(loc) = extra.get("location").and_then(Value::as_str) {
            places.push(loc.to_string());
        }
    }
    record.locations = places.iter().map(|p| extract::location(p)).collect();
    if let (Some(first), Some(country)) = (
        record.locations.first_mut(),
        text(job, "/address/postalAddress/addressCountry"),
    ) {
        if first.country.is_none() {
            first.country = crate::analytics::normalize::country_name(&country).map(str::to_string);
        }
    }
    record.work_mode = match text(job, "/workplaceType").as_deref() {
        Some("Remote") => Some(WorkMode::Remote),
        Some("Hybrid") => Some(WorkMode::Hybrid),
        Some("OnSite") => Some(WorkMode::Onsite),
        _ => job
            .get("isRemote")
            .and_then(Value::as_bool)
            .filter(|r| *r)
            .map(|_| WorkMode::Remote),
    };
    if let Some(kind) = text(job, "/employmentType") {
        let words = match kind.as_str() {
            "FullTime" => "full-time",
            "PartTime" => "part-time",
            "Intern" => "intern",
            "Contract" => "contract",
            "Temporary" => "temporary",
            other => other,
        };
        (record.employment_type, record.working_time) = extract::employment(words);
    }
    let html = text(job, "/descriptionHtml").unwrap_or_default();
    let (description, truncated) = extract::clean_html(&html);
    record.description =
        extract::description(Some(description).filter(|d| !d.is_empty()), truncated, true);

    if let Some((day, _)) = text(job, "/publishedAt").as_deref().and_then(extract::date) {
        record.dates.posted_at = Some(day);
        record.dates.posted_precision = Some(DatePrecision::Day);
        evidence(
            &mut record,
            "dates.posted_at",
            SOURCE,
            api_url,
            "ats_feed",
            Some("publishedAt"),
            now,
        );
    }
    let components: Vec<&Value> = job
        .pointer("/compensation/summaryComponents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    if let Some(salary) = components
        .iter()
        .find(|c| c.get("compensationType").and_then(Value::as_str) == Some("Salary"))
    {
        record.compensation = extract::compensation_structured(
            salary.get("minValue").and_then(Value::as_f64),
            salary.get("maxValue").and_then(Value::as_f64),
            salary.get("currencyCode").and_then(Value::as_str),
            salary
                .get("interval")
                .and_then(Value::as_str)
                .and_then(interval),
            text(job, "/compensation/compensationTierSummary").as_deref(),
        );
        if let Some(pay) = record.compensation.as_mut() {
            pay.variable_pay_mentioned |= components.iter().any(|c| {
                c.get("compensationType")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t != "Salary")
            });
            evidence(
                &mut record,
                "compensation",
                SOURCE,
                api_url,
                "ats_feed",
                Some("compensation.summaryComponents"),
                now,
            );
        }
    }
    record.links.canonical_url = text(job, "/jobUrl");
    record.links.apply_url = text(job, "/applyUrl");
    record.quality.availability = Availability::Active;
    record.quality.availability_basis =
        Some("listed on the employer's Ashby board (Job Postings API)".into());
    record.dates.last_checked_at = Some(extract::iso(now));
    finish_common(&mut record, SOURCE, api_url, now);
    Ok(record)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::rema_mcp::contract::SalaryBound;

    pub fn board(id: &str) -> Value {
        serde_json::json!({
            "apiVersion": "1",
            "jobs": [{
                "id": id,
                "title": "AI Platform Engineer",
                "location": "Zurich",
                "secondaryLocations": [{ "location": "Remote (Switzerland)" }],
                "workplaceType": "Hybrid",
                "employmentType": "FullTime",
                "isListed": true,
                "descriptionHtml": "<p>Build the platform.</p><h2>Requirements</h2><ul><li>Go</li><li>Kubernetes</li></ul>",
                "publishedAt": "2026-09-21T12:00:00.000+00:00",
                "jobUrl": format!("https://jobs.ashbyhq.com/alpen/{id}"),
                "applyUrl": format!("https://jobs.ashbyhq.com/alpen/{id}/application"),
                "address": { "postalAddress": { "addressLocality": "Zurich", "addressCountry": "Switzerland" } },
                "compensation": {
                    "compensationTierSummary": "CHF 120K – 150K • Offers Bonus",
                    "summaryComponents": [
                        { "compensationType": "Salary", "interval": "1 YEAR", "currencyCode": "CHF", "minValue": 120000, "maxValue": 150000 },
                        { "compensationType": "Bonus", "interval": "1 YEAR", "currencyCode": "CHF", "minValue": null, "maxValue": null }
                    ]
                }
            }]
        })
    }

    #[test]
    fn reads_a_listed_job_from_the_board() {
        let id = "1a2b3c4d-5e6f-7a8b-9c0d-1e2f3a4b5c6d";
        let job = parse(
            &board(id)["jobs"][0],
            "alpen",
            "https://api/x",
            1_790_380_800_000,
        )
        .unwrap();
        assert_eq!(job.title, "AI Platform Engineer");
        assert_eq!(job.work_mode, Some(WorkMode::Hybrid));
        assert_eq!(job.locations[0].country.as_deref(), Some("Switzerland"));
        let pay = job.compensation.unwrap();
        assert_eq!(
            (pay.min, pay.max, pay.currency.as_deref()),
            (Some(120_000.0), Some(150_000.0), Some("CHF"))
        );
        assert_eq!(pay.bound, Some(SalaryBound::Range));
        assert_eq!(pay.period, Some(SalaryPeriod::Year));
        assert!(pay.variable_pay_mentioned);
        assert_eq!(job.dates.posted_at.as_deref(), Some("2026-09-21"));
    }
}
