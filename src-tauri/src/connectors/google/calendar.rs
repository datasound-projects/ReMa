//! Google Calendar (primary calendar) behind [`CalendarProvider`]: events in
//! a window, free/busy, and the interview events ReMa manages.

use serde_json::{json, Value};

use crate::{
    connectors::{
        api::ApiClient,
        calendar::{
            rfc3339, BusyBlock, CalendarEvent, CalendarProvider, EventDraft, INTERVIEW_PROPERTY,
        },
    },
    error::{AppError, AppResult},
    llm::BoxFuture,
    models::connectors::ProviderId,
};

impl EventDraft {
    /// The Google Calendar event body.
    pub fn to_google(&self) -> Value {
        json!({
            "summary": self.summary,
            "description": self.description,
            "location": self.location.clone().unwrap_or_default(),
            "start": { "dateTime": rfc3339(self.start_at), "timeZone": self.timezone },
            "end": { "dateTime": rfc3339(self.end_at), "timeZone": self.timezone },
            "transparency": if self.cancelled { "transparent" } else { "opaque" },
            "extendedProperties": {
                "private": { INTERVIEW_PROPERTY: self.interview_id.to_string() }
            },
        })
    }
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

/// Google Calendar behind the calendar interface.
pub struct GoogleCalendar {
    pub api: ApiClient,
    /// Used for free/busy (the primary calendar's id is the account email).
    pub calendar_id: String,
}

fn event_path(id: &str) -> String {
    let id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .collect();
    format!("calendars/primary/events/{id}")
}

impl GoogleCalendar {
    async fn list(&self, query: &[(&str, String)]) -> AppResult<Vec<CalendarEvent>> {
        let mut events = Vec::new();
        let mut page: Option<String> = None;
        for _ in 0..10 {
            let mut params = query.to_vec();
            if let Some(token) = &page {
                params.push(("pageToken", token.clone()));
            }
            let body = self.api.get("calendars/primary/events", &params).await?;
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

impl CalendarProvider for GoogleCalendar {
    fn provider(&self) -> ProviderId {
        ProviderId::Google
    }

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

    fn get_availability<'a>(
        &'a self,
        time_min: i64,
        time_max: i64,
    ) -> BoxFuture<'a, AppResult<Vec<BusyBlock>>> {
        Box::pin(async move {
            let body = json!({
                "timeMin": rfc3339(time_min),
                "timeMax": rfc3339(time_max),
                "items": [{ "id": "primary" }],
            });
            let response = self
                .api
                .send(reqwest::Method::POST, "freeBusy", |r| r.json(&body))
                .await?;
            let value = self.api.expect_ok(response)?;
            let calendars = value.get("calendars").and_then(Value::as_object);
            let busy = calendars
                .and_then(|c| {
                    c.get("primary")
                        .or_else(|| c.get(&self.calendar_id))
                        .or_else(|| c.values().next())
                })
                .and_then(|c| c.get("busy"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Ok(busy
                .iter()
                .filter_map(|b| {
                    let time = |key: &str| {
                        b.get(key)?
                            .as_str()?
                            .parse::<jiff::Timestamp>()
                            .ok()
                            .map(|t| t.as_millisecond())
                    };
                    Some(BusyBlock {
                        start_at: time("start")?,
                        end_at: time("end")?,
                    })
                })
                .collect())
        })
    }

    fn get_event<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<Option<CalendarEvent>>> {
        Box::pin(async move {
            let response = self
                .api
                .send(reqwest::Method::GET, &event_path(id), |r| r)
                .await?;
            if matches!(response.status, 404 | 410) {
                return Ok(None);
            }
            Ok(parse_event(&self.api.expect_ok(response)?))
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
            let body = draft.to_google();
            let response = self
                .api
                .send(reqwest::Method::POST, "calendars/primary/events", |r| {
                    r.json(&body)
                })
                .await?;
            let value = self.api.expect_ok(response)?;
            value
                .get("id")
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
            let body = draft.to_google();
            let response = self
                .api
                .send(reqwest::Method::PATCH, &event_path(id), |r| r.json(&body))
                .await?;
            if matches!(response.status, 404 | 410) {
                return Err(AppError::not_found("The calendar event no longer exists."));
            }
            self.api.expect_ok(response).map(|_| ())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{connectors::api::StaticToken, test_support::MockServer};

    const HOUR: i64 = 3_600_000;

    fn calendar(server: &MockServer) -> GoogleCalendar {
        GoogleCalendar {
            api: ApiClient::new(
                reqwest::Client::new(),
                Arc::new(StaticToken("tok".into())),
                "Google Calendar",
                "Google Calendar",
                &format!("{}/calendar/v3", server.base_url),
            ),
            calendar_id: "me@example.com".into(),
        }
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
    fn drafts_keep_the_stated_time_zone_and_the_interview_link() {
        let draft = EventDraft {
            interview_id: 5,
            summary: "Interview — Acme — AI Engineer".into(),
            description: "Company: Acme".into(),
            location: None,
            start_at: 1_790_000_000_000,
            end_at: 1_790_003_600_000,
            timezone: "Europe/Vienna".into(),
            cancelled: false,
        };
        let body = draft.to_google();
        assert_eq!(
            body["extendedProperties"]["private"]["remaInterviewId"],
            "5"
        );
        assert_eq!(body["start"]["timeZone"], "Europe/Vienna");
        assert!(body["start"]["dateTime"].as_str().unwrap().ends_with('Z'));
    }

    #[tokio::test]
    async fn creates_updates_finds_events_and_reads_free_busy() {
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
            if req.method == "POST" && t.ends_with("/freeBusy") {
                return Some((200, r#"{"calendars":{"primary":{"busy":[{"start":"2026-09-28T08:30:00Z","end":"2026-09-28T09:30:00Z"}]}}}"#.into()));
            }
            if t.contains("/events/gone") {
                return Some((410, "{}".into()));
            }
            None
        })
        .await;
        let api = calendar(&server);
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
        let busy = api.get_availability(0, HOUR).await.unwrap();
        assert_eq!(busy.len(), 1);
        assert_eq!(busy[0].end_at - busy[0].start_at, HOUR);
        let created = server
            .requests()
            .into_iter()
            .find(|r| r.method == "POST" && r.target.ends_with("/events"))
            .unwrap();
        assert!(created.body.contains("remaInterviewId"));
        assert!(created.headers.contains("authorization: bearer tok"));
    }
}
