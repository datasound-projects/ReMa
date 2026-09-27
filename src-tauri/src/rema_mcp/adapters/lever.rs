//! Lever Postings API: `GET /v0/postings/{site}/{id}` (EU employers on
//! `api.eu.lever.co`). Known employer sites only; not a global index.

use serde_json::Value;

use super::{blank, evidence, finish_common, source_id, Ctx};
use crate::rema_mcp::{
    contract::{
        AcquisitionMode, Availability, DatePrecision, ErrorCode, JobRecord, ToolError, WorkMode,
    },
    extract,
};

const SOURCE: &str = "lever";

pub async fn read(ctx: &Ctx<'_>, site: &str, id: &str, eu: bool) -> Result<JobRecord, ToolError> {
    let base = if eu {
        &ctx.apis.lever_eu
    } else {
        &ctx.apis.lever
    };
    let url = format!("{base}/v0/postings/{site}/{id}");
    let value = match ctx.json(&url).await {
        Err(e) if e.code == ErrorCode::JobNotFound => {
            return Err(ToolError::new(
                ErrorCode::JobNotFound,
                "the employer's Lever site no longer publishes this posting",
            ))
        }
        other => other?,
    };
    parse(&value, site, &url, ctx.now)
}

fn text(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn parse(value: &Value, site: &str, api_url: &str, now: i64) -> Result<JobRecord, ToolError> {
    let id = text(value, "/id")
        .ok_or_else(|| ToolError::new(ErrorCode::ParsingFailed, "the posting has no id"))?;
    let title = text(value, "/text")
        .ok_or_else(|| ToolError::new(ErrorCode::ParsingFailed, "the posting has no title"))?;
    let mut record = blank(AcquisitionMode::DocumentedPublicFeed);
    record.title = extract::clip(&title, 200);
    record
        .source_ids
        .push(source_id(SOURCE, &format!("{site}:{id}")));
    evidence(
        &mut record,
        "title",
        SOURCE,
        api_url,
        "ats_api",
        Some("text"),
        now,
    );

    let mut places: Vec<String> = value
        .pointer("/categories/allLocations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    if places.is_empty() {
        places.extend(text(value, "/categories/location"));
    }
    record.locations = places.iter().map(|p| extract::location(p)).collect();
    record.work_mode = match text(value, "/workplaceType").as_deref() {
        Some("remote") => Some(WorkMode::Remote),
        Some("hybrid") => Some(WorkMode::Hybrid),
        Some("on-site" | "onsite") => Some(WorkMode::Onsite),
        _ => None,
    };
    if record.work_mode.is_some() {
        evidence(
            &mut record,
            "work_mode",
            SOURCE,
            api_url,
            "ats_api",
            Some("workplaceType"),
            now,
        );
    }
    if let Some(commitment) = text(value, "/categories/commitment") {
        (record.employment_type, record.working_time) = extract::employment(&commitment);
    }

    // Description, then each titled list, then the closing text.
    let mut html = text(value, "/description").unwrap_or_default();
    for list in value
        .get("lists")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let heading = list.get("text").and_then(Value::as_str).unwrap_or_default();
        let items = list
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        html.push_str(&format!("<h3>{heading}</h3><ul>{items}</ul>"));
    }
    html.push_str(&text(value, "/additional").unwrap_or_default());
    let (description, truncated) = extract::clean_html(&html);
    record.description =
        extract::description(Some(description).filter(|d| !d.is_empty()), truncated, true);

    if let Some(created) = value.get("createdAt").and_then(Value::as_i64) {
        let (day, _) = extract::date_ms(created);
        record.dates.posted_at = Some(day);
        record.dates.posted_precision = Some(DatePrecision::Day);
        evidence(
            &mut record,
            "dates.posted_at",
            SOURCE,
            api_url,
            "ats_api",
            Some("createdAt"),
            now,
        );
    }
    if let Some(range) = value.get("salaryRange") {
        record.compensation = extract::compensation_structured(
            range.get("min").and_then(Value::as_f64),
            range.get("max").and_then(Value::as_f64),
            range.get("currency").and_then(Value::as_str),
            range
                .get("interval")
                .and_then(Value::as_str)
                .and_then(extract::period_word),
            text(value, "/salaryDescriptionPlain").as_deref(),
        );
        if record.compensation.is_some() {
            evidence(
                &mut record,
                "compensation",
                SOURCE,
                api_url,
                "ats_api",
                Some("salaryRange"),
                now,
            );
        }
    }
    record.links.canonical_url = text(value, "/hostedUrl");
    record.links.apply_url = text(value, "/applyUrl");
    record.quality.availability = Availability::Active;
    record.quality.availability_basis =
        Some("published on the employer's Lever site (Postings API)".into());
    record.dates.last_checked_at = Some(extract::iso(now));
    finish_common(&mut record, SOURCE, api_url, now);
    Ok(record)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::rema_mcp::contract::{EmploymentType, SalaryBound, SalaryPeriod, WorkingTime};

    pub fn fixture(id: &str) -> Value {
        serde_json::json!({
            "id": id,
            "text": "Machine Learning Engineer",
            "categories": { "location": "Berlin", "commitment": "Full-time", "team": "AI",
                            "allLocations": ["Berlin", "Munich"] },
            "country": "DE",
            "workplaceType": "hybrid",
            "description": "<div>Wir bauen KI-Produkte für unsere Kunden und du hilfst uns dabei, die Modelle in Produktion zu bringen.</div>",
            "lists": [
                { "text": "Dein Profil", "content": "<li>Erfahrung mit Python</li><li>Kubernetes von Vorteil</li>" },
                { "text": "Was wir bieten", "content": "<li>30 Urlaubstage</li>" }
            ],
            "additional": "<div>Bewirb dich jetzt.</div>",
            "hostedUrl": format!("https://jobs.eu.lever.co/donau/{id}"),
            "applyUrl": format!("https://jobs.eu.lever.co/donau/{id}/apply"),
            "createdAt": 1_790_035_200_000_i64,
            "salaryRange": { "min": 70000, "max": 90000, "currency": "EUR", "interval": "per-year-salary" }
        })
    }

    #[test]
    fn reads_a_posting_with_lists_and_salary() {
        let id = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
        let job = parse(&fixture(id), "donau", "https://api/x", 1_790_380_800_000).unwrap();
        assert_eq!(job.title, "Machine Learning Engineer");
        assert_eq!(job.locations.len(), 2);
        assert_eq!(job.work_mode, Some(WorkMode::Hybrid));
        assert_eq!(job.working_time, Some(WorkingTime::FullTime));
        assert_eq!(job.employment_type, None::<EmploymentType>);
        assert_eq!(job.dates.posted_at.as_deref(), Some("2026-09-22"));
        let pay = job.compensation.unwrap();
        assert_eq!(pay.bound, Some(SalaryBound::Range));
        assert_eq!(pay.period, Some(SalaryPeriod::Year));
        assert_eq!(job.description.requirements, ["Erfahrung mit Python"]);
        assert_eq!(job.description.preferred, ["Kubernetes von Vorteil"]);
        assert_eq!(job.description.benefits, ["30 Urlaubstage"]);
        assert_eq!(job.language.as_deref(), Some("de"));
        assert!(job.quality.inferred.contains(&"language".to_string()));
        assert!(job.links.apply_url.unwrap().ends_with("/apply"));
        assert!(
            job.employer.name.is_none(),
            "Lever does not state the employer name"
        );
    }
}
