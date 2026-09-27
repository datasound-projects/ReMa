//! Greenhouse Job Board API: `GET /v1/boards/{board}/jobs/{id}` on the
//! public board API (published jobs only; no authentication; never the
//! application-submission API).

use serde_json::Value;

use super::{blank, evidence, finish_common, source_id, Ctx};
use crate::rema_mcp::{
    contract::{AcquisitionMode, Availability, DatePrecision, ErrorCode, JobRecord, ToolError},
    extract,
};

const SOURCE: &str = "greenhouse";

pub async fn read(ctx: &Ctx<'_>, board: &str, job: &str) -> Result<JobRecord, ToolError> {
    let url = format!(
        "{}/v1/boards/{board}/jobs/{job}?pay_transparency=true",
        ctx.apis.greenhouse
    );
    let value = match ctx.json(&url).await {
        Err(e) if e.code == ErrorCode::JobNotFound => {
            return Err(ToolError::new(
                ErrorCode::JobNotFound,
                "the employer's Greenhouse board no longer publishes this job",
            ))
        }
        other => other?,
    };
    parse(&value, board, &url, ctx.now)
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn parse(value: &Value, board: &str, api_url: &str, now: i64) -> Result<JobRecord, ToolError> {
    let id = value
        .get("id")
        .map(|v| v.to_string().trim_matches('"').to_string())
        .filter(|s| !s.is_empty() && s != "null")
        .ok_or_else(|| ToolError::new(ErrorCode::ParsingFailed, "the job has no id"))?;
    let title = text(value, "title")
        .ok_or_else(|| ToolError::new(ErrorCode::ParsingFailed, "the job has no title"))?;
    let mut record = blank(AcquisitionMode::DocumentedPublicFeed);
    record.title = extract::clip(&title, 200);
    record
        .source_ids
        .push(source_id(SOURCE, &format!("{board}:{id}")));
    if let Some(req) = text(value, "requisition_id") {
        record
            .source_ids
            .push(source_id("requisition", &format!("{board}:{req}")));
    }
    evidence(
        &mut record,
        "title",
        SOURCE,
        api_url,
        "ats_api",
        Some("title"),
        now,
    );
    record.employer.name = text(value, "company_name");
    if record.employer.name.is_some() {
        evidence(
            &mut record,
            "employer",
            SOURCE,
            api_url,
            "ats_api",
            Some("company_name"),
            now,
        );
    }

    let mut places: Vec<String> = Vec::new();
    if let Some(name) = value.pointer("/location/name").and_then(Value::as_str) {
        places.push(name.to_string());
    }
    for office in value
        .get("offices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(loc) = office
            .get("location")
            .and_then(Value::as_str)
            .or_else(|| office.get("name").and_then(Value::as_str))
        {
            if !places.iter().any(|p| p.eq_ignore_ascii_case(loc)) {
                places.push(loc.to_string());
            }
        }
    }
    record.locations = places.iter().map(|p| extract::location(p)).collect();

    let content = text(value, "content").unwrap_or_default();
    let (description, truncated) = extract::clean_html(&content);
    record.description =
        extract::description(Some(description).filter(|d| !d.is_empty()), truncated, true);
    if record.description.text.is_some() {
        evidence(
            &mut record,
            "description",
            SOURCE,
            api_url,
            "ats_api",
            Some("content"),
            now,
        );
    }

    // The original publication date; `updated_at` is only the last update.
    if let Some((day, _)) = text(value, "first_published")
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
            Some("first_published"),
            now,
        );
    }
    record.dates.updated_at = text(value, "updated_at");

    if let Some(range) = value
        .get("pay_input_ranges")
        .and_then(Value::as_array)
        .and_then(|r| r.first())
    {
        let cents = |key: &str| range.get(key).and_then(Value::as_f64).map(|c| c / 100.0);
        let label = [text(range, "title"), text(range, "blurb")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        let period =
            crate::analytics::normalize::period(&label.to_lowercase()).map(extract::period);
        record.compensation = extract::compensation_structured(
            cents("min_cents"),
            cents("max_cents"),
            range.get("currency_type").and_then(Value::as_str),
            period,
            None,
        );
        if record.compensation.is_some() {
            evidence(
                &mut record,
                "compensation",
                SOURCE,
                api_url,
                "ats_api",
                Some("pay_input_ranges"),
                now,
            );
        }
    }
    record.language = text(value, "language").map(|l| l.chars().take(2).collect());
    let posting_url = text(value, "absolute_url")
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"));
    record.links.canonical_url = posting_url;

    record.quality.availability = Availability::Active;
    record.quality.availability_basis =
        Some("published on the employer's Greenhouse board (Job Board API)".into());
    record.dates.last_checked_at = Some(extract::iso(now));
    evidence(
        &mut record,
        "quality.availability",
        SOURCE,
        api_url,
        "ats_api",
        None,
        now,
    );
    finish_common(&mut record, SOURCE, api_url, now);
    Ok(record)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::rema_mcp::contract::{DescriptionState, SalaryBound, SalaryPeriod};

    pub fn fixture() -> Value {
        serde_json::json!({
            "id": 4411001,
            "internal_job_id": 900,
            "title": "Senior AI Engineer (m/w/d)",
            "company_name": "Nordlicht AI",
            "requisition_id": "ENG-42",
            "updated_at": "2026-09-25T08:00:00-04:00",
            "first_published": "2026-09-23T09:00:00-04:00",
            "language": "en",
            "location": { "name": "Vienna, Austria (Hybrid)" },
            "absolute_url": "https://job-boards.greenhouse.io/nordlicht/jobs/4411001",
            "content": "&lt;h3&gt;What you'll do&lt;/h3&gt;&lt;ul&gt;&lt;li&gt;Ship LLM features&lt;/li&gt;&lt;/ul&gt;&lt;h3&gt;Requirements&lt;/h3&gt;&lt;ul&gt;&lt;li&gt;5+ years of Python&lt;/li&gt;&lt;li&gt;Experience with PyTorch&lt;/li&gt;&lt;/ul&gt;&lt;h3&gt;Nice to have&lt;/h3&gt;&lt;ul&gt;&lt;li&gt;Rust&lt;/li&gt;&lt;/ul&gt;",
            "offices": [{ "name": "Vienna", "location": "Vienna, Austria" }],
            "pay_input_ranges": [{ "min_cents": 8500000, "max_cents": 10000000, "currency_type": "EUR", "title": "Annual salary", "blurb": "" }]
        })
    }

    #[test]
    fn reads_a_published_job_with_evidence() {
        let job = parse(&fixture(), "nordlicht", "https://api/x", 1_790_380_800_000).unwrap();
        assert_eq!(job.title, "Senior AI Engineer (m/w/d)");
        assert_eq!(job.employer.name.as_deref(), Some("Nordlicht AI"));
        assert_eq!(job.source_ids[0].id, "nordlicht:4411001");
        assert_eq!(job.dates.posted_at.as_deref(), Some("2026-09-23"));
        assert_eq!(
            job.dates.updated_at.as_deref(),
            Some("2026-09-25T08:00:00-04:00"),
            "the update date stays separate"
        );
        assert_eq!(job.locations[0].city.as_deref(), Some("Vienna"));
        assert_eq!(job.locations.len(), 2);
        assert_eq!(
            job.work_mode,
            Some(crate::rema_mcp::contract::WorkMode::Hybrid)
        );
        assert_eq!(
            job.seniority,
            Some(crate::rema_mcp::contract::Seniority::Senior)
        );
        let pay = job.compensation.as_ref().unwrap();
        assert_eq!((pay.min, pay.max), (Some(85_000.0), Some(100_000.0)));
        assert_eq!(pay.bound, Some(SalaryBound::Range));
        assert_eq!(pay.period, Some(SalaryPeriod::Year));
        assert_eq!(job.description.state, DescriptionState::Full);
        assert_eq!(
            job.description.requirements,
            ["5+ years of Python", "Experience with PyTorch"]
        );
        assert_eq!(job.description.preferred, ["Rust"]);
        assert_eq!(job.quality.availability, Availability::Active);
        assert!(job
            .evidence
            .iter()
            .any(|e| e.field == "dates.posted_at" && e.path.as_deref() == Some("first_published")));
    }

    #[test]
    fn an_update_date_is_not_a_posting_date() {
        let mut value = fixture();
        value.as_object_mut().unwrap().remove("first_published");
        value.as_object_mut().unwrap().remove("pay_input_ranges");
        let job = parse(&value, "nordlicht", "https://api/x", 1_790_380_800_000).unwrap();
        assert_eq!(job.dates.posted_at, None);
        assert!(job.dates.updated_at.is_some());
        assert!(job.compensation.is_none());
    }
}
