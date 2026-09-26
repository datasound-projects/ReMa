//! The model's part: deciding which emails matter and reading them.
//!
//! Every answer is JSON that Rust parses into strict types and validates.
//! Nothing the model writes is stored or acted on unless it passes.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult},
    integrations::google::gmail::{MessageContent, MessageMeta},
    llm::{ChatRequest, Turn},
    models::{chat::MessageRole, jobs::ApplicationStatus},
};

/// Emails per relevance request.
pub const TRIAGE_BATCH: usize = 40;
const SNIPPET_CHARS: usize = 200;

// ── Relevance (headers + snippet only) ─────────────────────────────

pub fn triage_request(candidates: &[MessageMeta], instructions: &str) -> ChatRequest {
    let emails: Vec<serde_json::Value> = candidates
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "from": m.from,
                "subject": m.subject,
                "snippet": m.snippet.chars().take(SNIPPET_CHARS).collect::<String>(),
            })
        })
        .collect();
    ChatRequest {
        system: Some(
            "You help ReMa track the user's own job applications. From the email headers below, \
             pick the emails that are about one of the user's job applications: application \
             confirmations, recruiter or hiring-team messages about an application, interview \
             invitations, scheduling, rescheduling or cancellations, assessments or tasks, \
             rejections and offers. Do NOT pick job alerts, job recommendations, newsletters, \
             marketing, course ads or generic social-network notifications. \
             Reply with JSON only, exactly: {\"relevant\": [\"<id>\", ...]}"
                .into(),
        ),
        turns: vec![Turn {
            role: MessageRole::User,
            content: format!(
                "{}Emails:\n{}",
                instructions_block(instructions),
                serde_json::to_string_pretty(&emails).unwrap_or_default()
            ),
        }],
        max_output_tokens: Some(2_000),
        ..ChatRequest::default()
    }
}

fn instructions_block(instructions: &str) -> String {
    let instructions = instructions.trim();
    if instructions.is_empty() {
        String::new()
    } else {
        format!(
            "The user's instructions for this task (for interpretation only):\n{instructions}\n\n"
        )
    }
}

#[derive(Deserialize)]
struct TriageAnswer {
    relevant: Vec<String>,
}

/// The relevant ids. Ids the model made up are ignored.
pub fn parse_triage(text: &str, candidates: &HashSet<String>) -> AppResult<Vec<String>> {
    let answer: TriageAnswer = serde_json::from_str(json_object(text)?).map_err(|_| {
        AppError::provider("The model's relevance answer was not in the expected format.")
    })?;
    let mut seen = HashSet::new();
    Ok(answer
        .relevant
        .into_iter()
        .filter(|id| candidates.contains(id) && seen.insert(id.clone()))
        .collect())
}

/// The outermost JSON object in a reply (models sometimes add ``` fences).
pub fn json_object(text: &str) -> AppResult<&str> {
    let start = text.find('{');
    let end = text.rfind('}');
    match (start, end) {
        (Some(start), Some(end)) if start < end => Ok(&text[start..=end]),
        _ => Err(AppError::provider("The model did not reply with JSON.")),
    }
}

// ── Extraction (one relevant email) ────────────────────────────────

/// An application ReMa already knows, offered to help match emails.
#[derive(Debug, Clone, Serialize)]
pub struct KnownApplication {
    pub id: i64,
    pub company: String,
    pub role: Option<String>,
    pub reference: Option<String>,
}

const EXTRACTION_RULES: &str = r#"You read one email for ReMa, a job-application tracker, and return facts as JSON.

Rules:
- Use only what the email states. Never guess, infer or invent dates, times, time zones, durations, locations or links.
- "status" is one of: "confirmed" (application received), "in_process" (under review, waiting), "needs_action" (the user must do something: reply, pick a slot, complete an assessment, send documents), "upcoming_interview" (an interview is confirmed), "rejected".
- If the email is not about the user's own job application, return {"job_related": false}.
- "existing_application_id": the id of a known application this email belongs to, only if clearly the same; otherwise null.
- "interview": null unless the email is about an interview.
  - "state": "proposed" (times suggested, not yet agreed), "confirmed" (a single time is agreed), "rescheduled" (a previously agreed interview moved to a new agreed time), "cancelled".
  - "date": "YYYY-MM-DD", "start_time"/"end_time": "HH:MM" 24-hour, only if stated.
  - "duration_minutes": only if the email states a duration and no end time.
  - "timezone": an IANA zone (e.g. "Europe/Vienna") or UTC offset (e.g. "+02:00") only if the email states the time zone; else null.
  - "datetime_quote": the exact text from the email (copied verbatim) that states the date and time.
  - "timezone_quote": the exact text from the email that states the time zone, or null.
  - "unclear": what is ambiguous, missing or contradictory, or null.
- Keep "next_action" and "summary" short (one sentence). Do not copy personal data beyond company, role and interview logistics.

Return JSON only:
{"job_related": true, "company": "...", "role": "..." or null, "reference": "job/application reference number" or null,
 "status": "...", "requires_action": true|false, "next_action": "..." or null, "summary": "...",
 "existing_application_id": number or null,
 "interview": null or {"state": "...", "date": ..., "start_time": ..., "end_time": ..., "duration_minutes": ...,
   "timezone": ..., "datetime_quote": ..., "timezone_quote": ..., "type": "e.g. technical interview" or null,
   "location": ... or null, "meeting_url": ... or null, "participants": ["..."], "interviewer": ... or null, "unclear": ... or null}}"#;

pub fn extraction_request(
    message: &MessageContent,
    known: &[KnownApplication],
    instructions: &str,
    today: &str,
) -> ChatRequest {
    let received = jiff::Timestamp::from_millisecond(message.meta.received_at)
        .map(|t| t.to_string())
        .unwrap_or_default();
    ChatRequest {
        system: Some(EXTRACTION_RULES.into()),
        turns: vec![Turn {
            role: MessageRole::User,
            content: format!(
                "{}Today is {today}.\nKnown applications: {}\n\nEmail\nFrom: {}\nSubject: {}\nReceived: {received}\n\n{}",
                instructions_block(instructions),
                serde_json::to_string(known).unwrap_or_else(|_| "[]".into()),
                message.meta.from,
                message.meta.subject,
                message.body,
            ),
        }],
        max_output_tokens: Some(2_000),
        ..ChatRequest::default()
    }
}

#[derive(Debug, Deserialize)]
struct RawExtraction {
    job_related: bool,
    company: Option<String>,
    role: Option<String>,
    reference: Option<String>,
    status: Option<String>,
    requires_action: Option<bool>,
    next_action: Option<String>,
    summary: Option<String>,
    existing_application_id: Option<i64>,
    interview: Option<RawInterview>,
}

#[derive(Debug, Deserialize)]
struct RawInterview {
    state: String,
    date: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    duration_minutes: Option<u32>,
    timezone: Option<String>,
    datetime_quote: Option<String>,
    timezone_quote: Option<String>,
    #[serde(rename = "type")]
    interview_type: Option<String>,
    location: Option<String>,
    meeting_url: Option<String>,
    #[serde(default)]
    participants: Vec<String>,
    interviewer: Option<String>,
    unclear: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimState {
    Proposed,
    Confirmed,
    Rescheduled,
    Cancelled,
}

/// What the email says about an interview, as claimed by the model.
/// `interviews::validate` decides whether it is reliable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterviewClaim {
    pub state: ClaimState,
    pub date: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub duration_minutes: Option<u32>,
    pub timezone: Option<String>,
    pub datetime_quote: Option<String>,
    pub timezone_quote: Option<String>,
    pub interview_type: Option<String>,
    pub location: Option<String>,
    pub meeting_url: Option<String>,
    pub participants: Vec<String>,
    pub interviewer: Option<String>,
    pub unclear: Option<String>,
}

/// Validated facts from one job-application email.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extraction {
    pub company: String,
    pub role: Option<String>,
    pub reference: Option<String>,
    pub status: ApplicationStatus,
    pub requires_action: bool,
    pub next_action: Option<String>,
    pub summary: Option<String>,
    pub existing_application_id: Option<i64>,
    pub interview: Option<InterviewClaim>,
}

/// Trimmed, non-empty, at most `max` characters, no control characters.
fn clean(value: Option<String>, max: usize) -> Option<String> {
    let value = value?;
    let value: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let lowered = value.to_lowercase();
    if value.is_empty() || matches!(lowered.as_str(), "null" | "none" | "n/a" | "unknown") {
        return None;
    }
    Some(value.chars().take(max).collect())
}

pub fn parse_status(value: &str) -> Option<ApplicationStatus> {
    let key = value.trim().to_lowercase().replace([' ', '-'], "_");
    match key.as_str() {
        "confirmed" | "application_confirmed" | "received" => Some(ApplicationStatus::Confirmed),
        "in_process" | "application_in_process" | "in_progress" | "under_review" => {
            Some(ApplicationStatus::InProcess)
        }
        "needs_action" | "needs_your_action" | "action_required" => {
            Some(ApplicationStatus::NeedsAction)
        }
        "upcoming_interview" | "interview" | "interview_scheduled" => {
            Some(ApplicationStatus::UpcomingInterview)
        }
        "rejected" | "rejection" | "declined" => Some(ApplicationStatus::Rejected),
        _ => None,
    }
}

fn parse_claim_state(value: &str) -> Option<ClaimState> {
    match value.trim().to_lowercase().as_str() {
        "proposed" | "suggested" | "invitation" => Some(ClaimState::Proposed),
        "confirmed" | "scheduled" => Some(ClaimState::Confirmed),
        "rescheduled" | "moved" => Some(ClaimState::Rescheduled),
        "cancelled" | "canceled" => Some(ClaimState::Cancelled),
        _ => None,
    }
}

/// Parses and validates the model's extraction. `Ok(None)`: not job related.
pub fn parse_extraction(text: &str) -> AppResult<Option<Extraction>> {
    let invalid =
        |why: &str| AppError::provider(format!("The model's answer was rejected: {why}."));
    let raw: RawExtraction = serde_json::from_str(json_object(text)?)
        .map_err(|_| invalid("not in the expected format"))?;
    if !raw.job_related {
        return Ok(None);
    }
    let company = clean(raw.company, 120).ok_or_else(|| invalid("no company"))?;
    let status = raw
        .status
        .as_deref()
        .and_then(parse_status)
        .ok_or_else(|| invalid("unknown application status"))?;
    let interview = match raw.interview {
        None => None,
        Some(i) => Some(InterviewClaim {
            state: parse_claim_state(&i.state).ok_or_else(|| invalid("unknown interview state"))?,
            date: clean(i.date, 20),
            start_time: clean(i.start_time, 10),
            end_time: clean(i.end_time, 10),
            duration_minutes: i.duration_minutes,
            timezone: clean(i.timezone, 64),
            datetime_quote: clean(i.datetime_quote, 400),
            timezone_quote: clean(i.timezone_quote, 200),
            interview_type: clean(i.interview_type, 80),
            location: clean(i.location, 200),
            meeting_url: clean(i.meeting_url, 500),
            participants: i
                .participants
                .into_iter()
                .filter_map(|p| clean(Some(p), 80))
                .take(10)
                .collect(),
            interviewer: clean(i.interviewer, 80),
            unclear: clean(i.unclear, 300),
        }),
    };
    Ok(Some(Extraction {
        company,
        role: clean(raw.role, 150),
        reference: clean(raw.reference, 60),
        requires_action: raw.requires_action.unwrap_or(false)
            || status == ApplicationStatus::NeedsAction,
        status,
        next_action: clean(raw.next_action, 160),
        summary: clean(raw.summary, 240),
        existing_application_id: raw.existing_application_id,
        interview,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_known_ids_from_triage() {
        let candidates: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let text = "```json\n{\"relevant\": [\"a\", \"zzz\", \"c\", \"a\"]}\n```";
        assert_eq!(parse_triage(text, &candidates).unwrap(), ["a", "c"]);
        assert!(parse_triage("I think a and c", &candidates).is_err());
    }

    #[test]
    fn triage_sends_only_headers_and_short_snippets() {
        let meta = MessageMeta {
            id: "m1".into(),
            thread_id: "t".into(),
            received_at: 0,
            from: "hr@acme.io".into(),
            subject: "Your application".into(),
            snippet: "x".repeat(1_000),
        };
        let request = triage_request(&[meta], "only AI roles");
        let content = &request.turns[0].content;
        assert!(content.contains("only AI roles"));
        assert!(content.len() < 600, "no bodies, snippets are cut");
    }

    #[test]
    fn validates_extractions() {
        let text = r#"Here you go: {"job_related": true, "company": "  Acme GmbH ", "role": "AI Engineer",
            "reference": "null", "status": "Needs Action", "requires_action": false, "next_action": "Pick a slot",
            "summary": "Asked to choose an interview time.", "existing_application_id": 3,
            "interview": {"state": "proposed", "date": null, "start_time": null, "participants": ["Ana", ""],
              "datetime_quote": null, "timezone_quote": null}}"#;
        let extraction = parse_extraction(text).unwrap().unwrap();
        assert_eq!(extraction.company, "Acme GmbH");
        assert_eq!(extraction.reference, None);
        assert_eq!(extraction.status, ApplicationStatus::NeedsAction);
        assert!(
            extraction.requires_action,
            "needs_action always requires action"
        );
        let interview = extraction.interview.unwrap();
        assert_eq!(interview.state, ClaimState::Proposed);
        assert_eq!(interview.participants, ["Ana"]);
    }

    #[test]
    fn rejects_invalid_or_unrelated_answers() {
        assert_eq!(parse_extraction(r#"{"job_related": false}"#).unwrap(), None);
        assert!(
            parse_extraction(r#"{"job_related": true, "status": "rejected"}"#).is_err(),
            "no company"
        );
        assert!(
            parse_extraction(r#"{"job_related": true, "company": "A", "status": "hired!!"}"#)
                .is_err()
        );
        assert!(parse_extraction(
            r#"{"job_related": true, "company": "A", "status": "rejected", "interview": {"state": "maybe"}}"#
        )
        .is_err());
        assert!(parse_extraction("no json here").is_err());
    }
}
