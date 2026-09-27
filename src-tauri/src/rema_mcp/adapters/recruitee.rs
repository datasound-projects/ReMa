//! Recruitee Careers Site API: `GET https://{company}.recruitee.com/api/offers/`
//! — an employer's published offers (public, no authentication).

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

const SOURCE: &str = "recruitee";

fn text(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A number the API may send as a string ("80000").
fn amount(value: &Value, pointer: &str) -> Option<f64> {
    let v = value.pointer(pointer)?;
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
        .filter(|a| *a > 0.0)
}

/// Every published offer of a company.
pub async fn list(ctx: &Ctx<'_>, company: &str) -> Result<Vec<JobRecord>, ToolError> {
    let url = ctx.apis.recruitee_offers(company);
    let body = ctx.feed(&url, Accept::Json).await?;
    let value: Value = serde_json::from_str(&body).map_err(|_| {
        ToolError::new(
            ErrorCode::ParsingFailed,
            "the Recruitee offers are unreadable",
        )
    })?;
    Ok(parse_list(&value, company, &url, ctx.now))
}

pub fn parse_list(value: &Value, company: &str, api_url: &str, now: i64) -> Vec<JobRecord> {
    value
        .get("offers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|o| text(o, "/status").is_none_or(|s| s == "published"))
        .take(super::MAX_LISTED)
        .filter_map(|offer| parse(offer, company, api_url, now))
        .collect()
}

fn parse(offer: &Value, company: &str, api_url: &str, now: i64) -> Option<JobRecord> {
    let id = offer
        .get("id")
        .map(|v| v.to_string().trim_matches('"').to_string())
        .filter(|s| !s.is_empty() && s != "null")?;
    let title = text(offer, "/title")?;
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
        Some("offers[].title"),
        now,
    );
    record.employer.name = text(offer, "/company_name");

    let place = text(offer, "/location").or_else(|| {
        let parts: Vec<String> = ["/city", "/country"]
            .iter()
            .filter_map(|p| text(offer, p))
            .collect();
        (!parts.is_empty()).then(|| parts.join(", "))
    });
    if let Some(place) = place {
        let mut location = extract::location(&place);
        if location.country.is_none() {
            location.country = text(offer, "/country_code")
                .and_then(|c| normalize::country_name(&c.to_uppercase()).map(str::to_string));
        }
        record.locations = vec![location];
    }
    let flag = |key: &str| offer.get(key).and_then(Value::as_bool) == Some(true);
    record.work_mode = if flag("remote") {
        Some(WorkMode::Remote)
    } else if flag("hybrid") {
        Some(WorkMode::Hybrid)
    } else if flag("on_site") {
        Some(WorkMode::Onsite)
    } else {
        None
    };
    if let Some(code) = text(offer, "/employment_type_code") {
        // "fulltime_permanent", "parttime_fixed_term", "freelance", …
        let terms = code
            .replace('_', " ")
            .replace("fulltime", "full-time")
            .replace("parttime", "part-time")
            .replace("fixed term", "fixed-term");
        (record.employment_type, record.working_time) = extract::employment(&terms);
    }
    let html = format!(
        "{}{}",
        text(offer, "/description").unwrap_or_default(),
        text(offer, "/requirements")
            .map(|r| format!("<h3>Requirements</h3>{r}"))
            .unwrap_or_default()
    );
    let (description, truncated) = extract::clean_html(&html);
    record.description =
        extract::description(Some(description).filter(|d| !d.is_empty()), truncated, true);
    let published = text(offer, "/published_at").or_else(|| text(offer, "/created_at"));
    if let Some((day, _)) = published.as_deref().and_then(extract::date) {
        record.dates.posted_at = Some(day);
        record.dates.posted_precision = Some(DatePrecision::Day);
        evidence(
            &mut record,
            "dates.posted_at",
            SOURCE,
            api_url,
            "ats_api",
            Some("published_at"),
            now,
        );
    }
    let (min, max) = (amount(offer, "/salary/min"), amount(offer, "/salary/max"));
    if min.is_some() || max.is_some() {
        record.compensation = extract::compensation_structured(
            min,
            max,
            text(offer, "/salary/currency").as_deref(),
            text(offer, "/salary/period")
                .as_deref()
                .and_then(extract::period_word),
            None,
        );
        if record.compensation.is_some() {
            evidence(
                &mut record,
                "compensation",
                SOURCE,
                api_url,
                "ats_api",
                Some("salary"),
                now,
            );
        }
    }
    record.links.canonical_url = text(offer, "/careers_url");
    record.links.apply_url = text(offer, "/careers_apply_url");
    record.quality.availability = Availability::Active;
    record.quality.availability_basis =
        Some("published on the employer's Recruitee careers site".into());
    record.dates.last_checked_at = Some(extract::iso(now));
    finish_common(&mut record, SOURCE, api_url, now);
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rema_mcp::contract::SalaryPeriod;

    #[test]
    fn reads_published_offers_with_salary() {
        let value = serde_json::json!({ "offers": [
            { "id": 901, "slug": "ai-engineer", "status": "published", "title": "AI Engineer",
              "company_name": "Wien Robotics", "location": "Vienna, Austria", "country_code": "AT",
              "hybrid": true, "employment_type_code": "fulltime_permanent",
              "description": "<p>Robots.</p>", "requirements": "<ul><li>ROS</li></ul>",
              "published_at": "2026-09-19 10:00:00 UTC",
              "salary": { "min": "85000", "max": "100000", "currency": "EUR", "period": "year" },
              "careers_url": "https://wienrobotics.recruitee.com/o/ai-engineer" },
            { "id": 902, "status": "closed", "title": "Old" }
        ]});
        let jobs = parse_list(&value, "wienrobotics", "https://api/x", 1_790_380_800_000);
        assert_eq!(jobs.len(), 1);
        let job = &jobs[0];
        assert_eq!(job.locations[0].city.as_deref(), Some("Vienna"));
        assert_eq!(job.work_mode, Some(WorkMode::Hybrid));
        assert_eq!(job.dates.posted_at.as_deref(), Some("2026-09-19"));
        let pay = job.compensation.as_ref().unwrap();
        assert_eq!((pay.min, pay.max), (Some(85_000.0), Some(100_000.0)));
        assert_eq!(pay.period, Some(SalaryPeriod::Year));
        assert_eq!(job.description.requirements, ["ROS"]);
    }
}
