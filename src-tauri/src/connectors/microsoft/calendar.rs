//! Outlook Calendar (Microsoft Graph, delegated `Calendars.ReadWrite`)
//! behind [`CalendarProvider`]: `calendarView` for events in a window,
//! `getSchedule` for free/busy, and the interview events ReMa manages
//! (tagged with a single-value extended property; created with a
//! `transactionId` so a retried request never creates a second event).

use serde_json::{json, Value};

use crate::{
    connectors::{
        api::ApiClient,
        calendar::{
            https_link, BusyBlock, CalendarEvent, CalendarProvider, EventDraft, INTERVIEW_PROPERTY,
        },
    },
    error::{AppError, AppResult},
    llm::BoxFuture,
    models::connectors::ProviderId,
};

/// ReMa's extended property (a fixed GUID namespace for ReMa).
pub fn property_id() -> String {
    format!("String {{7a1d7b58-5f1b-4a3c-9f0e-2a7c4e9d1b63}} Name {INTERVIEW_PROPERTY}")
}

const SELECT: &str =
    "id,subject,start,end,showAs,isAllDay,isCancelled,location,onlineMeeting,onlineMeetingUrl,webLink";

/// Graph `dateTimeTimeZone` in UTC.
fn graph_time(millis: i64) -> Value {
    let text = jiff::Timestamp::from_millisecond(millis)
        .map(|t| t.strftime("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_default();
    json!({ "dateTime": text, "timeZone": "UTC" })
}

/// Parses a Graph `dateTimeTimeZone` returned in UTC.
fn parse_time(value: &Value) -> Option<i64> {
    let text = value.get("dateTime")?.as_str()?;
    let zone = value
        .get("timeZone")
        .and_then(Value::as_str)
        .unwrap_or("UTC");
    let civil = text
        .split('.')
        .next()?
        .parse::<jiff::civil::DateTime>()
        .ok()?;
    let tz = jiff::tz::TimeZone::get(zone).unwrap_or(jiff::tz::TimeZone::UTC);
    civil
        .to_zoned(tz)
        .ok()
        .map(|z| z.timestamp().as_millisecond())
}

pub fn parse_event(value: &Value) -> Option<CalendarEvent> {
    if value.get("isCancelled").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let property = property_id();
    Some(CalendarEvent {
        id: value.get("id")?.as_str()?.to_string(),
        title: value
            .get("subject")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("(busy)")
            .to_string(),
        start_at: value.get("start").and_then(parse_time),
        end_at: value.get("end").and_then(parse_time),
        all_day: value.get("isAllDay").and_then(Value::as_bool) == Some(true),
        transparent: value.get("showAs").and_then(Value::as_str) == Some("free"),
        interview_id: value
            .get("singleValueExtendedProperties")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|p| {
                p.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id.eq_ignore_ascii_case(&property))
            })
            .and_then(|p| p.get("value")?.as_str()?.parse().ok()),
        location: value
            .pointer("/location/displayName")
            .and_then(Value::as_str)
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string),
        meeting_url: https_link(
            value
                .pointer("/onlineMeeting/joinUrl")
                .and_then(Value::as_str),
        )
        .or_else(|| https_link(value.get("onlineMeetingUrl").and_then(Value::as_str))),
        web_link: https_link(value.get("webLink").and_then(Value::as_str)),
    })
}

impl EventDraft {
    /// The Graph event body (times in UTC; Outlook shows them locally).
    pub fn to_graph(&self) -> Value {
        json!({
            "subject": self.summary,
            "body": { "contentType": "text", "content": self.description },
            "start": graph_time(self.start_at),
            "end": graph_time(self.end_at),
            "location": { "displayName": self.location.clone().unwrap_or_default() },
            "showAs": if self.cancelled { "free" } else { "busy" },
            "singleValueExtendedProperties": [
                { "id": property_id(), "value": self.interview_id.to_string() }
            ],
        })
    }
}

pub struct OutlookCalendar {
    pub api: ApiClient,
    /// The mailbox address (for getSchedule).
    pub email: String,
}

fn event_path(id: &str) -> String {
    let id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '='))
        .collect();
    format!("me/events/{id}")
}

fn expand() -> String {
    format!(
        "singleValueExtendedProperties($filter=id eq '{}')",
        property_id()
    )
}

impl OutlookCalendar {
    fn events(body: &Value) -> Vec<CalendarEvent> {
        body.get("value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_event)
            .collect()
    }
}

impl CalendarProvider for OutlookCalendar {
    fn provider(&self) -> ProviderId {
        ProviderId::Microsoft
    }

    fn list_events<'a>(
        &'a self,
        time_min: i64,
        time_max: i64,
    ) -> BoxFuture<'a, AppResult<Vec<CalendarEvent>>> {
        Box::pin(async move {
            let iso = |ms: i64| {
                jiff::Timestamp::from_millisecond(ms)
                    .map(|t| t.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_default()
            };
            let query = [
                ("startDateTime", iso(time_min)),
                ("endDateTime", iso(time_max)),
                ("$select", SELECT.to_string()),
                ("$expand", expand()),
                ("$top", "100".to_string()),
            ];
            let mut events = Vec::new();
            let mut next: Option<String> = None;
            for _ in 0..10 {
                let response = match &next {
                    None => {
                        self.api
                            .send(reqwest::Method::GET, "me/calendarView", |r| {
                                r.query(&query).header("Prefer", "outlook.timezone=\"UTC\"")
                            })
                            .await?
                    }
                    Some(link) => {
                        self.api
                            .send(reqwest::Method::GET, link, |r| {
                                r.header("Prefer", "outlook.timezone=\"UTC\"")
                            })
                            .await?
                    }
                };
                let body = self.api.expect_ok(response)?;
                events.extend(Self::events(&body));
                match body.get("@odata.nextLink").and_then(Value::as_str) {
                    Some(link) => next = Some(link.to_string()),
                    None => break,
                }
            }
            Ok(events)
        })
    }

    fn get_availability<'a>(
        &'a self,
        time_min: i64,
        time_max: i64,
    ) -> BoxFuture<'a, AppResult<Vec<BusyBlock>>> {
        Box::pin(async move {
            let body = json!({
                "schedules": [self.email],
                "startTime": graph_time(time_min),
                "endTime": graph_time(time_max),
                "availabilityViewInterval": 15,
            });
            let response = self
                .api
                .send(reqwest::Method::POST, "me/calendar/getSchedule", |r| {
                    r.json(&body).header("Prefer", "outlook.timezone=\"UTC\"")
                })
                .await?;
            let value = self.api.expect_ok(response)?;
            Ok(value
                .pointer("/value/0/scheduleItems")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|item| item.get("status").and_then(Value::as_str) != Some("free"))
                .filter_map(|item| {
                    Some(BusyBlock {
                        start_at: parse_time(item.get("start")?)?,
                        end_at: parse_time(item.get("end")?)?,
                    })
                })
                .collect())
        })
    }

    fn get_event<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<Option<CalendarEvent>>> {
        Box::pin(async move {
            let path = format!("{}?$select={SELECT}&$expand={}", event_path(id), expand());
            let response = self
                .api
                .send(reqwest::Method::GET, &path, |r| {
                    r.header("Prefer", "outlook.timezone=\"UTC\"")
                })
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
            let filter = format!(
                "singleValueExtendedProperties/Any(ep: ep/id eq '{}' and ep/value eq '{interview_id}')",
                property_id()
            );
            let response = self
                .api
                .send(reqwest::Method::GET, "me/events", |r| {
                    r.query(&[
                        ("$filter", filter.clone()),
                        ("$select", SELECT.to_string()),
                        ("$expand", expand()),
                        ("$top", "5".to_string()),
                    ])
                    .header("Prefer", "outlook.timezone=\"UTC\"")
                })
                .await?;
            let body = self.api.expect_ok(response)?;
            Ok(Self::events(&body)
                .into_iter()
                .find(|e| e.interview_id == Some(interview_id)))
        })
    }

    fn create_event<'a>(&'a self, draft: &'a EventDraft) -> BoxFuture<'a, AppResult<String>> {
        Box::pin(async move {
            let mut body = draft.to_graph();
            body["transactionId"] = json!(format!(
                "rema-{}-{}",
                draft.interview_id,
                &draft.content_hash()[..16]
            ));
            let response = self
                .api
                .send(reqwest::Method::POST, "me/events", |r| r.json(&body))
                .await?;
            let value = self.api.expect_ok(response)?;
            value
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| AppError::provider("Outlook Calendar returned no event id."))
        })
    }

    fn update_event<'a>(
        &'a self,
        id: &'a str,
        draft: &'a EventDraft,
    ) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            let body = draft.to_graph();
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

    fn calendar(server: &MockServer) -> OutlookCalendar {
        OutlookCalendar {
            api: ApiClient::new(
                reqwest::Client::new(),
                Arc::new(StaticToken("tok".into())),
                "Outlook Calendar",
                "Outlook Calendar",
                &format!("{}/graph/v1.0", server.base_url),
            ),
            email: "ana@outlook.com".into(),
        }
    }

    #[test]
    fn parses_graph_events_in_utc_and_the_interview_link() {
        let event = parse_event(&json!({
            "id": "AAMk1", "subject": "Dentist", "isAllDay": false, "showAs": "busy",
            "start": {"dateTime": "2026-09-28T08:30:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-09-28T09:30:00.0000000", "timeZone": "UTC"},
            "singleValueExtendedProperties": [{"id": property_id(), "value": "9"}]
        }))
        .unwrap();
        assert_eq!(event.end_at.unwrap() - event.start_at.unwrap(), HOUR);
        assert_eq!(event.interview_id, Some(9));
        assert_eq!(
            jiff::Timestamp::from_millisecond(event.start_at.unwrap())
                .unwrap()
                .to_string(),
            "2026-09-28T08:30:00Z"
        );
        let vienna = parse_event(&json!({
            "id": "x", "start": {"dateTime": "2026-09-28T10:30:00", "timeZone": "Europe/Vienna"},
            "end": {"dateTime": "2026-09-28T11:30:00", "timeZone": "Europe/Vienna"}
        }))
        .unwrap();
        assert_eq!(vienna.start_at, event.start_at, "time zone conversion");
        assert!(parse_event(&json!({"id": "c", "isCancelled": true})).is_none());
    }

    #[tokio::test]
    async fn creates_with_a_transaction_id_finds_by_property_and_reads_schedules() {
        let server = MockServer::start(|req| {
            let t = req.target.as_str();
            if req.method == "POST" && t == "/graph/v1.0/me/events" {
                return Some((201, r#"{"id":"evt-1"}"#.into()));
            }
            if req.method == "PATCH" && t.starts_with("/graph/v1.0/me/events/evt-1") {
                return Some((200, r#"{"id":"evt-1"}"#.into()));
            }
            if req.method == "GET" && t.starts_with("/graph/v1.0/me/events?") {
                return Some((200, json!({"value": [{
                    "id": "evt-1", "subject": "Interview",
                    "start": {"dateTime": "2026-09-28T08:00:00", "timeZone": "UTC"},
                    "end": {"dateTime": "2026-09-28T09:00:00", "timeZone": "UTC"},
                    "singleValueExtendedProperties": [{"id": property_id(), "value": "5"}]
                }]}).to_string()));
            }
            if req.method == "POST" && t.ends_with("/me/calendar/getSchedule") {
                return Some((200, json!({"value": [{"scheduleItems": [
                    {"status": "busy", "start": {"dateTime": "2026-09-28T08:30:00", "timeZone": "UTC"},
                     "end": {"dateTime": "2026-09-28T09:30:00", "timeZone": "UTC"}},
                    {"status": "free", "start": {"dateTime": "2026-09-28T10:00:00", "timeZone": "UTC"},
                     "end": {"dateTime": "2026-09-28T11:00:00", "timeZone": "UTC"}}
                ]}]}).to_string()));
            }
            if t.starts_with("/graph/v1.0/me/events/gone") {
                return Some((404, r#"{"error":{"code":"ErrorItemNotFound"}}"#.into()));
            }
            None
        })
        .await;
        let api = calendar(&server);
        let draft = EventDraft {
            interview_id: 5,
            summary: "Interview — Acme — AI Engineer".into(),
            description: "Company: Acme".into(),
            location: Some("Online".into()),
            start_at: 1_790_000_000_000,
            end_at: 1_790_000_000_000 + HOUR,
            timezone: "Europe/Vienna".into(),
            cancelled: false,
        };
        assert_eq!(api.create_event(&draft).await.unwrap(), "evt-1");
        api.update_event("evt-1", &draft).await.unwrap();
        assert_eq!(api.find_by_interview(5).await.unwrap().unwrap().id, "evt-1");
        assert_eq!(api.get_event("gone").await.unwrap(), None);
        let busy = api.get_availability(0, 10 * HOUR).await.unwrap();
        assert_eq!(busy.len(), 1, "free slots are not busy");

        let created = server
            .requests()
            .into_iter()
            .find(|r| r.method == "POST" && r.target == "/graph/v1.0/me/events")
            .unwrap();
        let body: Value = serde_json::from_str(&created.body).unwrap();
        assert!(body["transactionId"]
            .as_str()
            .unwrap()
            .starts_with("rema-5-"));
        assert_eq!(body["start"]["timeZone"], "UTC");
        assert_eq!(body["singleValueExtendedProperties"][0]["value"], "5");
    }
}
