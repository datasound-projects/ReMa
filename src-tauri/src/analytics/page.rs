//! Reading a job's own page in the background.
//!
//! Many job pages embed a schema.org `JobPosting` (JSON-LD) with the
//! posting date, salary, employment type and full description; those
//! structured values are preferred. Otherwise the page text is used as the
//! description. Pages are fetched like a browser would open them: public
//! addresses only (never the local network), no cookies, no scripts, and
//! nothing that bypasses a site's protections — a refused page is simply
//! not read.

use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use futures_util::StreamExt;
use reqwest::{header, redirect, Client, StatusCode, Url};
use serde_json::Value;

use super::normalize::{self, Salary};
use crate::{
    error::{AppError, AppResult},
    models::analytics::{EmploymentType, SalaryPeriod, WorkMode},
};

const MAX_PAGE_BYTES: usize = 3 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
pub const MAX_DESCRIPTION_CHARS: usize = 20_000;

pub fn client(version: &str) -> Client {
    Client::builder()
        .redirect(redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(format!(
            "Mozilla/5.0 (compatible; ReMa/{version}; job details reader)"
        ))
        .build()
        .unwrap_or_default()
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || a == 0
        || (a == 100 && (b & 0xC0) == 64) // carrier-grade NAT
        || a >= 224)
}

/// Whether an address is on the public internet (not this computer or the
/// local network).
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_v4(v4);
            }
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local
                || (first & 0xffc0) == 0xfe80) // link local
        }
    }
}

async fn check_host(url: &Url, allow_private: bool) -> AppResult<()> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::validation("only web pages are read"));
    }
    if allow_private {
        return Ok(());
    }
    let host = url
        .host_str()
        .ok_or_else(|| AppError::validation("the link has no host"))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses: Vec<IpAddr> = match host.trim_matches(['[', ']']).parse::<IpAddr>() {
        Ok(ip) => vec![ip],
        Err(_) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| AppError::network("the site's address could not be found"))?
            .map(|a| a.ip())
            .collect(),
    };
    if addresses.is_empty() || !addresses.into_iter().all(is_public) {
        return Err(AppError::validation(
            "the link points to this computer or the local network, which ReMa does not read",
        ));
    }
    Ok(())
}

/// Fetches a page's HTML. Every redirect target is checked again.
pub async fn fetch(client: &Client, url: &str, allow_private: bool) -> AppResult<String> {
    let mut current =
        Url::parse(url).map_err(|_| AppError::validation("the link is not a web address"))?;
    for _ in 0..=MAX_REDIRECTS {
        check_host(&current, allow_private).await?;
        let response = client
            .get(current.clone())
            .header(header::ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await?;
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| AppError::network("the site redirected without a target"))?;
            current = current
                .join(location)
                .map_err(|_| AppError::network("the site redirected to an invalid address"))?;
            continue;
        }
        if matches!(
            status,
            StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED | StatusCode::TOO_MANY_REQUESTS
        ) {
            return Err(AppError::network(format!(
                "the site does not allow automated reading ({})",
                status.as_u16()
            )));
        }
        if !status.is_success() {
            return Err(AppError::network(format!(
                "the site answered {}",
                status.as_u16()
            )));
        }
        let html_like = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_none_or(|t| t.contains("html") || t.contains("text/plain"));
        if !html_like {
            return Err(AppError::network("the link is not a web page"));
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            body.extend_from_slice(&chunk);
            if body.len() > MAX_PAGE_BYTES {
                body.truncate(MAX_PAGE_BYTES);
                break;
            }
        }
        return Ok(String::from_utf8_lossy(&body).into_owned());
    }
    Err(AppError::network("the site redirected too often"))
}

// ── HTML → text ───────────────────────────────────────────────────────

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some(' '),
        "ndash" => Some('–'),
        "mdash" => Some('—'),
        "bull" | "middot" => Some('•'),
        "auml" => Some('ä'),
        "ouml" => Some('ö'),
        "uuml" => Some('ü'),
        "Auml" => Some('Ä'),
        "Ouml" => Some('Ö'),
        "Uuml" => Some('Ü'),
        "szlig" => Some('ß'),
        "euro" => Some('€'),
        e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
            .ok()
            .and_then(char::from_u32),
        e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
        _ => None,
    }
}

pub fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        match after.find(';').filter(|&end| end <= 10) {
            Some(end) => match decode_entity(&after[..end]) {
                Some(c) => {
                    out.push(c);
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('&');
                    rest = after;
                }
            },
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

const SKIPPED: &[&str] = &[
    "script", "style", "noscript", "svg", "nav", "header", "footer", "template", "iframe", "form",
];
const BLOCKS: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "ul",
    "ol",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "tr",
    "section",
    "article",
    "table",
    "dt",
    "dd",
    "blockquote",
];

/// Visible text of an HTML fragment, one block per line; list items start
/// with "- " so descriptions keep their structure.
pub fn html_to_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::new();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < html.len() {
        if bytes[i] == b'<' {
            let Some(end) = html[i..].find('>').map(|e| i + e) else {
                break;
            };
            let tag = &lower[i + 1..end];
            let closing = tag.starts_with('/');
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if !closing && SKIPPED.contains(&name.as_str()) && !tag.ends_with('/') {
                // Skip to the matching closing tag.
                let close = format!("</{name}");
                i = lower[end..].find(&close).map_or(html.len(), |p| end + p);
                i = lower[i..].find('>').map_or(html.len(), |p| i + p + 1);
                continue;
            }
            if BLOCKS.contains(&name.as_str()) {
                out.push('\n');
                if name == "li" && !closing {
                    out.push_str("- ");
                }
            } else {
                out.push(' ');
            }
            i = end + 1;
        } else {
            let next = html[i..].find('<').map_or(html.len(), |p| i + p);
            out.push_str(&html[i..next]);
            i = next;
        }
    }
    let decoded = decode_entities(&out);
    let mut lines: Vec<String> = Vec::new();
    for line in decoded.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() || line == "-" {
            continue;
        }
        if lines.last() != Some(&line) {
            lines.push(line);
        }
    }
    let text = lines.join("\n");
    text.chars().take(MAX_DESCRIPTION_CHARS).collect()
}

// ── JSON-LD JobPosting ────────────────────────────────────────────────

/// What a job page states.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageFacts {
    pub structured: bool,
    pub company: Option<String>,
    pub description: Option<String>,
    pub date_posted: Option<String>,
    pub employment_type: Option<EmploymentType>,
    pub work_mode: Option<WorkMode>,
    pub location: Option<String>,
    pub salary: Option<Salary>,
    pub salary_text: Option<String>,
    pub reference: Option<String>,
    /// Requirement items from structured fields ("skills", "qualifications").
    pub skills: Vec<String>,
    pub requirement_text: Option<String>,
    pub benefits: Vec<String>,
}

fn ld_blocks(html: &str) -> Vec<Value> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(start) = lower[from..].find("<script").map(|p| from + p) {
        let Some(tag_end) = lower[start..].find('>').map(|p| start + p) else {
            break;
        };
        let tag = &lower[start..tag_end];
        let Some(close) = lower[tag_end..].find("</script").map(|p| tag_end + p) else {
            break;
        };
        if tag.contains("application/ld+json") {
            if let Ok(value) = serde_json::from_str::<Value>(html[tag_end + 1..close].trim()) {
                out.push(value);
            }
        }
        from = close + 1;
    }
    out
}

fn postings(value: &Value, out: &mut Vec<Value>) {
    match value {
        Value::Array(items) => items.iter().for_each(|v| postings(v, out)),
        Value::Object(map) => {
            let is_posting = match map.get("@type") {
                Some(Value::String(t)) => t == "JobPosting",
                Some(Value::Array(types)) => types.iter().any(|t| t == "JobPosting"),
                _ => false,
            };
            if is_posting {
                out.push(value.clone());
            }
            if let Some(graph) = map.get("@graph") {
                postings(graph, out);
            }
        }
        _ => {}
    }
}

fn text_of(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Object(map) => text_of(
            map.get("name")
                .or_else(|| map.get("value"))
                .or_else(|| map.get("description")),
        ),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(|v| text_of(Some(v))).collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        }
        _ => None,
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.replace([',', ' '], "").parse().ok(),
        _ => None,
    }
    .filter(|v| *v > 0.0)
}

fn salary_of(value: &Value) -> Option<(Salary, String)> {
    let currency = text_of(value.get("currency"))?.to_uppercase();
    let amount = value.get("value")?;
    let (min, max, unit) = match amount {
        Value::Object(_) => (
            number(amount.get("minValue")).or_else(|| number(amount.get("value"))),
            number(amount.get("maxValue")).or_else(|| number(amount.get("value"))),
            text_of(amount.get("unitText")),
        ),
        _ => (number(Some(amount)), number(Some(amount)), None),
    };
    if min.is_none() && max.is_none() {
        return None;
    }
    let period = match unit.as_deref().map(str::to_uppercase).as_deref() {
        Some("YEAR") => Some(SalaryPeriod::Year),
        Some("MONTH") => Some(SalaryPeriod::Month),
        Some("WEEK") => Some(SalaryPeriod::Week),
        Some("DAY") => Some(SalaryPeriod::Day),
        Some("HOUR") => Some(SalaryPeriod::Hour),
        _ => None,
    };
    let code: &'static str = match currency.as_str() {
        "EUR" => "EUR",
        "USD" => "USD",
        "GBP" => "GBP",
        "CHF" => "CHF",
        "CAD" => "CAD",
        "AUD" => "AUD",
        "SEK" => "SEK",
        "NOK" => "NOK",
        "DKK" => "DKK",
        "PLN" => "PLN",
        "CZK" => "CZK",
        "HUF" => "HUF",
        "INR" => "INR",
        "JPY" => "JPY",
        "SGD" => "SGD",
        _ => return None,
    };
    let salary = Salary {
        min,
        max,
        currency: Some(code),
        period,
    };
    let text = normalize::format_salary(&salary);
    Some((salary, text))
}

fn location_of(posting: &Value) -> Option<String> {
    let locations = match posting.get("jobLocation")? {
        Value::Array(items) => items.clone(),
        other => vec![other.clone()],
    };
    let parts: Vec<String> = locations
        .iter()
        .filter_map(|loc| {
            let address = loc.get("address").unwrap_or(loc);
            let city = text_of(address.get("addressLocality"));
            let country = text_of(address.get("addressCountry"));
            match (city, country) {
                (Some(c), Some(k)) => Some(format!("{c}, {k}")),
                (c, k) => c.or(k),
            }
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" / "))
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1)
        .map(str::to_string)
        .collect()
}

/// Structured facts and the description of a job page. When a page holds
/// several postings, the one whose title matches `title` best is used.
pub fn parse(html: &str, title: &str) -> PageFacts {
    let mut found = Vec::new();
    for block in ld_blocks(html) {
        postings(&block, &mut found);
    }
    let wanted = words(title);
    let posting = found.into_iter().max_by_key(|p| {
        let have = words(&text_of(p.get("title")).unwrap_or_default());
        wanted.iter().filter(|w| have.contains(w)).count()
    });
    let Some(posting) = posting else {
        let body_start = html.to_ascii_lowercase().find("<body").unwrap_or(0);
        let text = html_to_text(&html[body_start..]);
        return PageFacts {
            description: (text.len() >= 200).then_some(text),
            ..PageFacts::default()
        };
    };
    let list = |key: &str| -> Vec<String> {
        match posting.get(key) {
            Some(Value::Array(items)) => items.iter().filter_map(|v| text_of(Some(v))).collect(),
            Some(v) => text_of(Some(v))
                .map(|t| {
                    let text = html_to_text(&t);
                    if text.contains('\n') {
                        text.lines()
                            .map(|l| l.trim_start_matches("- ").to_string())
                            .collect()
                    } else {
                        text.split(',').map(|s| s.trim().to_string()).collect()
                    }
                })
                .unwrap_or_default(),
            None => Vec::new(),
        }
    };
    let (salary, salary_text) = posting
        .get("baseSalary")
        .and_then(salary_of)
        .map_or((None, None), |(s, t)| (Some(s), Some(t)));
    let remote = text_of(posting.get("jobLocationType"))
        .is_some_and(|t| t.to_uppercase().contains("TELECOMMUTE"));
    let requirement_text: Vec<String> = [
        "qualifications",
        "educationRequirements",
        "experienceRequirements",
    ]
    .iter()
    .filter_map(|k| text_of(posting.get(*k)))
    .map(|t| html_to_text(&t))
    .collect();
    PageFacts {
        structured: true,
        company: text_of(posting.get("hiringOrganization")),
        description: text_of(posting.get("description"))
            .map(|d| html_to_text(&decode_entities(&d)))
            .filter(|d| !d.is_empty()),
        date_posted: text_of(posting.get("datePosted")).map(|d| d.chars().take(10).collect()),
        employment_type: text_of(posting.get("employmentType"))
            .and_then(|t| normalize::employment_type(&t.replace('_', "-"))),
        work_mode: remote.then_some(WorkMode::Remote),
        location: location_of(&posting),
        salary,
        salary_text,
        reference: text_of(posting.get("identifier")).map(|r| normalize::clip(&r, 60)),
        skills: list("skills"),
        requirement_text: (!requirement_text.is_empty()).then(|| requirement_text.join("\n")),
        benefits: list("jobBenefits").into_iter().take(12).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<html><head><title>AI Engineer</title>
<script type="application/ld+json">{"@context":"https://schema.org","@graph":[{"@type":"Organization","name":"X"},
{"@type":"JobPosting","title":"AI Engineer (m/w/d)","datePosted":"2025-09-20T08:00:00+02:00",
 "employmentType":["FULL_TIME"],"jobLocationType":"TELECOMMUTE",
 "hiringOrganization":{"@type":"Organization","name":"Company A"},
 "jobLocation":{"@type":"Place","address":{"addressLocality":"Wien","addressCountry":"AT"}},
 "baseSalary":{"@type":"MonetaryAmount","currency":"EUR","value":{"@type":"QuantitativeValue","minValue":90000,"maxValue":110000,"unitText":"YEAR"}},
 "identifier":{"@type":"PropertyValue","value":"REQ-4711"},
 "skills":"Python, Kubernetes",
 "description":"&lt;p&gt;We build &lt;b&gt;LLM&lt;/b&gt; platforms.&lt;/p&gt;&lt;ul&gt;&lt;li&gt;5+ years of experience&lt;/li&gt;&lt;li&gt;Terraform is a plus&lt;/li&gt;&lt;/ul&gt;"}]}
</script></head><body><nav>Menu</nav><p>Ignored</p></body></html>"#;

    #[test]
    fn reads_structured_job_postings() {
        let facts = parse(PAGE, "AI Engineer");
        assert!(facts.structured);
        assert_eq!(facts.company.as_deref(), Some("Company A"));
        assert_eq!(facts.date_posted.as_deref(), Some("2025-09-20"));
        assert_eq!(facts.employment_type, Some(EmploymentType::FullTime));
        assert_eq!(facts.work_mode, Some(WorkMode::Remote));
        assert_eq!(facts.location.as_deref(), Some("Wien, AT"));
        let salary = facts.salary.unwrap();
        assert_eq!(
            (salary.min, salary.max, salary.currency, salary.period),
            (
                Some(90_000.0),
                Some(110_000.0),
                Some("EUR"),
                Some(SalaryPeriod::Year)
            )
        );
        assert_eq!(facts.reference.as_deref(), Some("REQ-4711"));
        assert_eq!(facts.skills, ["Python", "Kubernetes"]);
        assert_eq!(
            facts.description.as_deref(),
            Some("We build LLM platforms.\n- 5+ years of experience\n- Terraform is a plus")
        );
    }

    #[test]
    fn falls_back_to_page_text() {
        let html = format!(
            "<html><head><style>p{{color:red}}</style></head><body><header>Site</header><h1>Data Engineer</h1>\
             <p>{}</p><ul><li>SQL &amp; Python</li></ul><script>var x = 1;</script></body></html>",
            "We are hiring. ".repeat(20)
        );
        let facts = parse(&html, "Data Engineer");
        assert!(!facts.structured);
        let text = facts.description.unwrap();
        assert!(text.starts_with("Data Engineer\nWe are hiring."));
        assert!(text.ends_with("- SQL & Python"));
        assert!(!text.contains("color") && !text.contains("var x") && !text.contains("Site"));
    }

    #[test]
    fn refuses_local_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "172.16.5.4",
            "169.254.1.1",
            "100.64.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["93.184.216.34", "2606:2800:220:1::1"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn never_fetches_the_local_network() {
        let client = client("test");
        let error = fetch(&client, "http://127.0.0.1:9/job", false)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("local network"), "{error}");
        assert!(fetch(&client, "file:///etc/hostname", false).await.is_err());
    }
}
