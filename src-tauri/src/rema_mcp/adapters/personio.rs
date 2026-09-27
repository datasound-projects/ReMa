//! Personio open-position XML feed: `GET https://{company}.jobs.personio.de/xml`
//! (only when the employer enabled it). Parsed with `quick-xml`, which reads
//! no DTDs and expands no external entities; only the five XML entities and
//! numeric references are resolved.

use quick_xml::events::Event;

use super::{blank, evidence, finish_common, source_id, Ctx};
use crate::rema_mcp::{
    contract::{
        AcquisitionMode, Availability, DatePrecision, ErrorCode, JobRecord, Seniority, ToolError,
    },
    extract,
    fetch::Accept,
};

const SOURCE: &str = "personio";

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Position {
    pub id: String,
    pub name: String,
    pub subcompany: Option<String>,
    pub offices: Vec<String>,
    pub department: Option<String>,
    pub employment_type: Option<String>,
    pub seniority: Option<String>,
    pub schedule: Option<String>,
    pub keywords: Option<String>,
    pub created_at: Option<String>,
    /// (section name, HTML value)
    pub descriptions: Vec<(String, String)>,
}

pub async fn read(
    ctx: &Ctx<'_>,
    company: &str,
    id: &str,
    domain: &str,
) -> Result<JobRecord, ToolError> {
    let url = ctx.apis.personio_feed(company, domain);
    let body = ctx.feed(&url, Accept::Xml).await?;
    let positions = parse_feed(&body)?;
    let position = positions.into_iter().find(|p| p.id == id).ok_or_else(|| {
        ToolError::new(
            ErrorCode::JobNotFound,
            "the employer's Personio feed no longer lists this position",
        )
    })?;
    let job_url = format!("https://{company}.{domain}/job/{id}");
    Ok(record(&position, company, &url, &job_url, ctx.now))
}

/// Every open position in the employer's feed.
pub async fn list(ctx: &Ctx<'_>, company: &str, domain: &str) -> Result<Vec<JobRecord>, ToolError> {
    let url = ctx.apis.personio_feed(company, domain);
    let body = ctx.feed(&url, Accept::Xml).await?;
    Ok(parse_feed(&body)?
        .iter()
        .take(super::MAX_LISTED)
        .map(|p| {
            let job_url = format!("https://{company}.{domain}/job/{}", p.id);
            record(p, company, &url, &job_url, ctx.now)
        })
        .collect())
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        other => other
            .strip_prefix("#x")
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .or_else(|| other.strip_prefix('#').and_then(|d| d.parse().ok()))
            .and_then(char::from_u32),
    }
}

/// Every `<position>` in the feed.
pub fn parse_feed(xml: &str) -> Result<Vec<Position>, ToolError> {
    let unreadable = || ToolError::new(ErrorCode::ParsingFailed, "the Personio feed is unreadable");
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut positions = Vec::new();
    let mut current: Option<Position> = None;
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut description_name = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if name == "position" {
                    current = Some(Position::default());
                }
                path.push(name);
                text.clear();
            }
            Ok(Event::Text(t)) => text.push_str(&t.decode().map_err(|_| unreadable())?),
            Ok(Event::CData(c)) => text.push_str(&String::from_utf8_lossy(&c.into_inner())),
            Ok(Event::GeneralRef(r)) => {
                let name = r.decode().map_err(|_| unreadable())?;
                // Undeclared or custom entities (&xxe;) are dropped, never expanded.
                text.extend(entity(&name));
            }
            Ok(Event::End(_)) => {
                let name = path.pop().unwrap_or_default();
                let value = text.trim().to_string();
                let parent = path.last().map(String::as_str).unwrap_or_default();
                if let Some(p) = current.as_mut() {
                    match (parent, name.as_str()) {
                        ("position", "id") => p.id = value.clone(),
                        ("position", "name") => p.name = value.clone(),
                        ("position", "subcompany") => p.subcompany = Some(value.clone()),
                        ("position", "office") | ("additionalOffices", "office") => {
                            if !value.is_empty() {
                                p.offices.push(value.clone())
                            }
                        }
                        ("position", "department") => p.department = Some(value.clone()),
                        ("position", "employmentType") => p.employment_type = Some(value.clone()),
                        ("position", "seniority") => p.seniority = Some(value.clone()),
                        ("position", "schedule") => p.schedule = Some(value.clone()),
                        ("position", "keywords") => p.keywords = Some(value.clone()),
                        ("position", "createdAt") => p.created_at = Some(value.clone()),
                        ("jobDescription", "name") => description_name = value.clone(),
                        ("jobDescription", "value") => p
                            .descriptions
                            .push((std::mem::take(&mut description_name), value.clone())),
                        (_, "position") => positions.extend(current.take()),
                        _ => {}
                    }
                }
                text.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => return Err(unreadable()),
            _ => {}
        }
        if positions.len() > 2_000 {
            break;
        }
    }
    Ok(positions
        .into_iter()
        .filter(|p| !p.id.is_empty() && !p.name.is_empty())
        .collect())
}

pub fn record(p: &Position, company: &str, feed_url: &str, job_url: &str, now: i64) -> JobRecord {
    let mut record = blank(AcquisitionMode::DocumentedPublicFeed);
    record.title = extract::clip(&p.name, 200);
    record
        .source_ids
        .push(source_id(SOURCE, &format!("{company}:{}", p.id)));
    evidence(
        &mut record,
        "title",
        SOURCE,
        feed_url,
        "ats_feed",
        Some("position/name"),
        now,
    );
    record.employer.name = p.subcompany.clone().filter(|s| !s.is_empty());
    record.locations = p.offices.iter().map(|o| extract::location(o)).collect();
    let terms = [p.employment_type.as_deref(), p.schedule.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    (record.employment_type, record.working_time) = extract::employment(&terms);
    record.seniority = extract::seniority(&p.name).or(match p.seniority.as_deref() {
        Some("entry-level") => Some(Seniority::Entry),
        Some("executive") => Some(Seniority::Executive),
        Some("student" | "intern") => Some(Seniority::Intern),
        // "experienced" is not a level on ReMa's scale: left unknown.
        _ => None,
    });
    let html: String = p
        .descriptions
        .iter()
        .map(|(name, value)| format!("<h3>{name}</h3>{value}"))
        .collect();
    let (description, truncated) = extract::clean_html(&html);
    record.description =
        extract::description(Some(description).filter(|d| !d.is_empty()), truncated, true);
    if let Some((day, _)) = p.created_at.as_deref().and_then(extract::date) {
        record.dates.posted_at = Some(day);
        record.dates.posted_precision = Some(DatePrecision::Day);
        evidence(
            &mut record,
            "dates.posted_at",
            SOURCE,
            feed_url,
            "ats_feed",
            Some("position/createdAt"),
            now,
        );
    }
    record.links.canonical_url = Some(job_url.to_string());
    record.quality.availability = Availability::Active;
    record.quality.availability_basis =
        Some("listed in the employer's Personio open-position feed".into());
    record.dates.last_checked_at = Some(extract::iso(now));
    finish_common(&mut record, SOURCE, feed_url, now);
    record
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::rema_mcp::contract::{EmploymentType, SalaryBound, WorkingTime};

    pub const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<workzag-jobs>
  <position>
    <id>123456</id>
    <subcompany>Wien AI GmbH</subcompany>
    <office>Wien</office>
    <additionalOffices><office>Graz</office></additionalOffices>
    <department>Engineering</department>
    <name>Senior AI Engineer (m/w/d)</name>
    <jobDescriptions>
      <jobDescription><name>Deine Aufgaben</name><value><![CDATA[<ul><li>LLM-Produkte entwickeln</li></ul>]]></value></jobDescription>
      <jobDescription><name>Dein Profil</name><value><![CDATA[<ul><li>Python &amp; PyTorch</li><li>Erfahrung mit AWS von Vorteil</li></ul><p>Für diese Position gilt ein Mindestgehalt von EUR 4.500,- brutto pro Monat, Bereitschaft zur Überzahlung.</p>]]></value></jobDescription>
    </jobDescriptions>
    <employmentType>permanent</employmentType>
    <seniority>experienced</seniority>
    <schedule>full-time</schedule>
    <createdAt>2026-09-22T09:00:00+00:00</createdAt>
  </position>
  <position>
    <id>123457</id>
    <office>Wien</office>
    <name>Werkstudent Data (m/w/d)</name>
    <jobDescriptions></jobDescriptions>
    <schedule>part-time</schedule>
  </position>
</workzag-jobs>"#;

    #[test]
    fn reads_the_open_position_feed() {
        let positions = parse_feed(FEED).unwrap();
        assert_eq!(positions.len(), 2);
        let job = record(
            &positions[0],
            "wien-ai",
            "https://feed",
            "https://wien-ai.jobs.personio.de/job/123456",
            1_790_380_800_000,
        );
        assert_eq!(job.title, "Senior AI Engineer (m/w/d)");
        assert_eq!(job.employer.name.as_deref(), Some("Wien AI GmbH"));
        assert_eq!(job.locations.len(), 2);
        assert_eq!(job.locations[0].city.as_deref(), Some("Vienna"));
        assert_eq!(job.employment_type, Some(EmploymentType::Permanent));
        assert_eq!(job.working_time, Some(WorkingTime::FullTime));
        assert_eq!(job.seniority, Some(Seniority::Senior));
        assert_eq!(job.description.requirements, ["Python & PyTorch"]);
        assert_eq!(job.description.preferred, ["Erfahrung mit AWS von Vorteil"]);
        let pay = job.compensation.as_ref().unwrap();
        assert_eq!(pay.min, Some(4500.0));
        assert_eq!(pay.bound, Some(SalaryBound::Floor));
        assert_eq!(job.language.as_deref(), None);
        assert_eq!(job.dates.posted_at.as_deref(), Some("2026-09-22"));
    }

    #[test]
    fn never_expands_external_entities() {
        let xml = r#"<?xml version="1.0"?>
<!DOCTYPE workzag-jobs [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
<workzag-jobs><position><id>1</id><name>AI &xxe; Engineer</name></position></workzag-jobs>"#;
        let positions = parse_feed(xml).unwrap();
        assert_eq!(positions[0].name, "AI  Engineer");
        assert!(!positions[0].name.contains("root:"));
    }
}
