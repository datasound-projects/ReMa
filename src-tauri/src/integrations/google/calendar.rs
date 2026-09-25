//! Google Calendar (primary calendar) as a deterministic tool.
//!
//! Only four operations exist: list events in a window, look up an event,
//! create an interview event and update one. Every event ReMa writes carries
//! a private extended property with its interview id, so a lost database
//! write can never lead to a duplicate event.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::gmail::google_error;
use crate::{
    error::{AppError, AppResult},
    llm::{
        http::{error_message, scrub},
        BoxFuture,
    },
};

/// Private extended property linking an event to a ReMa interview.
pub const INTERVIEW_PROPERTY: &str = "remaInterviewId";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub all_day: bool,
    /// Marked as "free" (does not block time).
    pub transparent: bool,
    pub interview_id: Option<i64>,
}

/// The content of an interview event ReMa writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDraft {
    pub interview_id: i64,
    pub summary: String,
    pub description: String,
    pub location: Option<String>,
    pub start_at: i64,
    pub end_at: i64,
    pub timezone: String,
    /// The interview was cancelled: keep the event but mark it and free the time.
    pub cancelled: bool,
}

fn rfc3339(millis: i64) -> String {
    jiff::Timestamp::from_millisecond(millis)
        .map(|t| t.to_string())
        .unwrap_or_default()
}

impl EventDraft {
    pub fn to_json(&self) -> Value {
        let mut event = json!({
            "summary": self.summary,
            "description": self.description,
            "start": { "dateTime": rfc3339(self.start_at), "timeZone": self.timezone },
            "end": { "dateTime": rfc3339(self.end_at), "timeZone": self.timezone },
            "transparency": if self.cancelled { "transparent" } else { "opaque" },
            "extendedProperties": {
                "private": { INTERVIEW_PROPERTY: self.interview_id.to_string() }
            },
        });
        event["location"] = json!(self.location.clone().unwrap_or_default());
        event
    }

    /// Fingerprint of the written content, to detect needed updates.
    pub fn content_hash(&self) -> String {
        let digest = Sha256::digest(self.to_json().to_string().as_bytes());
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }
}

pub trait CalendarApi: Send + Sync {
    /// Events overlapping `[time_min, time_max)` on the primary calendar.
    fn list_events<'a>(
        &'a self,
        time_min: i64,
        time_max: i64,
    ) -> BoxFuture<'a, AppResult<Vec<CalendarEvent>>>;
    /// `None` if the event no longer exists (deleted in Calendar).
    fn get_event<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<Option<CalendarEvent>>>;
    /// An event ReMa created for this interview, found by its private property.
    fn find_by_interview<'a>(
        &'a self,
        interview_id: i64,
    ) -> BoxFuture<'a, AppResult<Option<CalendarEvent>>>;
    fn create_event<'a>(&'a self, draft: &'a EventDraft) -> BoxFuture<'a, AppResult<String>>;
    fn update_event<'a>(
        &'a self,
        id: &'a str,
        draft: &'a EventDraft,
    ) -> BoxFuture<'a, AppResult<()>>;
}

/// Existing events that overlap an interview and block time. ReMa's own
/// event for the interview, free ("transparent") and all-day events do not
/// count. Nothing is moved: conflicts are only reported.
pub fn find_conflicts(
    events: &[CalendarEvent],
    start_at: i64,
    end_at: i64,
    interview_id: i64,
) -> Vec<CalendarEvent> {
    events
        .iter()
        .filter(|e| e.interview_id != Some(interview_id))
        .filter(|e| !e.all_day && !e.transparent)
        .filter(|e| match (e.start_at, e.end_at) {
            (Some(s), Some(end)) => s < end_at && start_at < end,
            _ => false,
        })
        .cloned()
        .collect()
}

fn parse_time(value: &Value) -> (Option<i64>, bool) {
    if let Some(dt) = value.get("dateTime").and_then(Value::as_str) {
        let millis = dt
            .parse::<jiff::Timestamp>()
            .ok()
            .map(|t| t.as_millisecond());
        return (millis, false);
    }
    if let Some(date) = value.get("date").and_then(Value::as_str) {
        let millis = date
            .parse::<jiff::civil::Date>()
            .ok()
            .and_then(|d| d.to_zoned(jiff::tz::TimeZone::UTC).ok())
            .map(|z| z.timestamp().as_millisecond());
        return (millis, true);
    }
    (None, false)
}

pub fn parse_event(value: &Value) -> Option<CalendarEvent> {
    if value.get("status").and_then(Value::as_str) == Some("cancelled") {
        return None;
    }
    let (start_at, all_day) = parse_time(value.get("start")?);
    let (end_at, _) = parse_time(value.get("end")?);
    Some(CalendarEvent {
        id: value.get("id")?.as_str()?.to_string(),
        title: value
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("(busy)")
            .to_string(),
        start_at,
        end_at,
        all_day,
        transparent: value.get("transparency").and_then(Value::as_str) == Some("transparent"),
        interview_id: value
            .pointer(&format!("/extendedProperties/private/{INTERVIEW_PROPERTY}"))
            .and_then(Value::as_str)
            .and_then(|v| v.parse().ok()),
    })
}

/// The real client for the Google Calendar REST API.
pub struct HttpCalendar {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl HttpCalendar {
    pub fn new(http: reqwest::Client, base: &str, access_token: String) -> Self {
        Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token: access_token,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/calendars/primary/{path}", self.base)
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> AppResult<Option<Value>> {
        let response = request.bearer_auth(&self.token).send().await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status.is_success() {
            return serde_json::from_str(&body)
                .map(Some)
                .map_err(|_| AppError::provider("Google Calendar sent an unreadable response."));
        }
        if status.as_u16() == 404 || status.as_u16() == 410 {
            return Ok(None);
        }
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| error_message(&v))
            .map(|m| scrub(&m, &[&self.token]))
            .unwrap_or_default();
        Err(google_error("Google Calendar", status.as_u16(), &detail))
    }

    async fn list(&self, query: &[(&str, String)]) -> AppResult<Vec<CalendarEvent>> {
        let mut events = Vec::new();
        let mut page: Option<String> = None;
        for _ in 0..10 {
            let mut params = query.to_vec();
            if let Some(token) = &page {
                params.push(("pageToken", token.clone()));
            }
            let Some(body) = self
                .send(self.http.get(self.url("events")).query(&params))
                .await?
            else {
                break;
            };
            events.extend(
                body.get("items")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(parse_event),
            );
            match body.get("nextPageToken").and_then(Value::as_str) {
                Some(token) => page = Some(token.to_string()),
                None => break,
            }
        }
        Ok(events)
    }
}

impl CalendarApi for HttpCalendar {
    fn list_events<'a>(
        &'a self,
        time_min: i64,
        time_max: i64,
    ) -> BoxFuture<'a, AppResult<Vec<CalendarEvent>>> {
        Box::pin(async move {
            self.list(&[
                ("timeMin", rfc3339(time_min)),
                ("timeMax", rfc3339(time_max)),
                ("singleEvents", "true".into()),
                ("orderBy", "startTime".into()),
                ("maxResults", "250".into()),
            ])
            .await
        })
    }

    fn get_event<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<Option<CalendarEvent>>> {
        Box::pin(async move {
            let body = self
                .send(self.http.get(self.url(&format!("events/{id}"))))
                .await?;
            Ok(body.as_ref().and_then(parse_event))
        })
    }

    fn find_by_interview<'a>(
        &'a self,
        interview_id: i64,
    ) -> BoxFuture<'a, AppResult<Option<CalendarEvent>>> {
        Box::pin(async move {
            let events = self
                .list(&[
                    (
                        "privateExtendedProperty",
                        format!("{INTERVIEW_PROPERTY}={interview_id}"),
                    ),
                    ("maxResults", "10".into()),
                ])
                .await?;
            Ok(events
                .into_iter()
                .find(|e| e.interview_id == Some(interview_id)))
        })
    }

    fn create_event<'a>(&'a self, draft: &'a EventDraft) -> BoxFuture<'a, AppResult<String>> {
        Box::pin(async move {
            let body = self
                .send(self.http.post(self.url("events")).json(&draft.to_json()))
                .await?
                .ok_or_else(|| AppError::provider("Google Calendar could not create the event."))?;
            body.get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| AppError::provider("Google Calendar returned no event id."))
        })
    }

    fn update_event<'a>(
        &'a self,
        id: &'a str,
        draft: &'a EventDraft,
    ) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            self.send(
                self.http
                    .patch(self.url(&format!("events/{id}")))
                    .json(&draft.to_json()),
            )
            .await?
            .ok_or_else(|| AppError::not_found("The Calendar event no longer exists."))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockServer;

    const HOUR: i64 = 3_600_000;

    fn event(id: &str, start: i64, end: i64) -> CalendarEvent {
        CalendarEvent {
            id: id.into(),
            title: id.into(),
            start_at: Some(start),
            end_at: Some(end),
            all_day: false,
            transparent: false,
            interview_id: None,
        }
    }

    #[test]
    fn detects_only_real_overlaps() {
        let (start, end) = (10 * HOUR, 11 * HOUR);
        let mut own = event("own", start, end);
        own.interview_id = Some(7);
        let mut free = event("free", start, end);
        free.transparent = true;
        let mut all_day = event("holiday", 0, 24 * HOUR);
        all_day.all_day = true;
        let events = vec![
            event("overlap", 10 * HOUR + HOUR / 2, 11 * HOUR + HOUR / 2),
            event("before", 9 * HOUR, 10 * HOUR),
            event("after", 11 * HOUR, 12 * HOUR),
            event("inside", 10 * HOUR + 10, 10 * HOUR + 20),
            own,
            free,
            all_day,
        ];
        let ids: Vec<_> = find_conflicts(&events, start, end, 7)
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["overlap", "inside"]);
    }

    #[test]
    fn parses_timed_all_day_and_cancelled_events() {
        let timed = parse_event(&json!({
            "id": "e1", "summary": "Standup",
            "start": {"dateTime": "2026-09-28T10:00:00+02:00"}, "end": {"dateTime": "2026-09-28T10:30:00+02:00"},
            "extendedProperties": {"private": {"remaInterviewId": "12"}}
        }))
        .unwrap();
        assert_eq!(timed.end_at.unwrap() - timed.start_at.unwrap(), HOUR / 2);
        assert_eq!(timed.interview_id, Some(12));

        let all_day = parse_event(
            &json!({"id": "e2", "start": {"date": "2026-09-28"}, "end": {"date": "2026-09-29"}}),
        )
        .unwrap();
        assert!(all_day.all_day);
        assert!(
            parse_event(&json!({"id": "e3", "status": "cancelled", "start": {}, "end": {}}))
                .is_none()
        );
    }

    #[test]
    fn drafts_carry_the_interview_link_and_a_stable_hash() {
        let draft = EventDraft {
            interview_id: 5,
            summary: "AI Engineer Interview — Acme".into(),
            description: "Role: AI Engineer".into(),
            location: None,
            start_at: 1_790_000_000_000,
            end_at: 1_790_003_600_000,
            timezone: "Europe/Vienna".into(),
            cancelled: false,
        };
        let body = draft.to_json();
        assert_eq!(
            body["extendedProperties"]["private"]["remaInterviewId"],
            "5"
        );
        assert_eq!(body["start"]["timeZone"], "Europe/Vienna");
        assert!(body["start"]["dateTime"].as_str().unwrap().ends_with('Z'));
        assert_eq!(draft.content_hash(), draft.clone().content_hash());
        let moved = EventDraft {
            start_at: draft.start_at + HOUR,
            ..draft.clone()
        };
        assert_ne!(moved.content_hash(), draft.content_hash());
    }

    #[tokio::test]
    async fn creates_updates_and_finds_events_over_http() {
        let server = MockServer::start(|req| {
            let t = req.target.as_str();
            if req.method == "POST" && t.ends_with("/calendars/primary/events") {
                return Some((200, r#"{"id":"new-event"}"#.into()));
            }
            if req.method == "PATCH" && t.ends_with("/events/new-event") {
                return Some((200, r#"{"id":"new-event"}"#.into()));
            }
            if req.method == "GET" && t.contains("privateExtendedProperty=remaInterviewId%3D5") {
                return Some((200, r#"{"items":[{"id":"new-event","start":{"dateTime":"2026-09-28T08:00:00Z"},
                    "end":{"dateTime":"2026-09-28T09:00:00Z"},"extendedProperties":{"private":{"remaInterviewId":"5"}}}]}"#.into()));
            }
            if t.contains("/events/gone") {
                return Some((410, "{}".into()));
            }
            None
        })
        .await;
        let api = HttpCalendar::new(
            reqwest::Client::new(),
            &format!("{}/calendar/v3", server.base_url),
            "tok".into(),
        );
        let draft = EventDraft {
            interview_id: 5,
            summary: "Interview".into(),
            description: String::new(),
            location: None,
            start_at: 0,
            end_at: HOUR,
            timezone: "UTC".into(),
            cancelled: false,
        };
        assert_eq!(api.create_event(&draft).await.unwrap(), "new-event");
        api.update_event("new-event", &draft).await.unwrap();
        assert_eq!(
            api.find_by_interview(5).await.unwrap().unwrap().id,
            "new-event"
        );
        assert_eq!(api.get_event("gone").await.unwrap(), None);

        let created = server
            .requests()
            .into_iter()
            .find(|r| r.method == "POST")
            .unwrap();
        assert!(created.body.contains("remaInterviewId"));
        assert!(created.headers.contains("authorization: bearer tok"));
    }
}
