//! The provider-independent calendar interface (Google Calendar and Outlook
//! Calendar behind the same trait) and conflict detection.
//!
//! Every event ReMa writes carries its interview id in a private property
//! (Google extended property / Graph single-value extended property), so a
//! lost database write never leads to a duplicate event.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{error::AppResult, llm::BoxFuture, models::connectors::ProviderId};

/// Private property linking an event to a ReMa interview.
pub const INTERVIEW_PROPERTY: &str = "remaInterviewId";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub all_day: bool,
    /// Marked as free (does not block time).
    pub transparent: bool,
    pub interview_id: Option<i64>,
}

/// A busy period from a free/busy query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusyBlock {
    pub start_at: i64,
    pub end_at: i64,
}

/// The content of an interview event ReMa writes. Logistics only, never
/// email content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDraft {
    pub interview_id: i64,
    pub summary: String,
    pub description: String,
    pub location: Option<String>,
    pub start_at: i64,
    pub end_at: i64,
    /// IANA name shown by Google; Outlook events are written in UTC.
    pub timezone: String,
    /// The interview was cancelled: keep the event but mark it and free the time.
    pub cancelled: bool,
}

pub fn rfc3339(millis: i64) -> String {
    jiff::Timestamp::from_millisecond(millis)
        .map(|t| t.to_string())
        .unwrap_or_default()
}

impl EventDraft {
    /// Canonical content, for detecting whether an update is needed.
    fn canonical(&self) -> Value {
        json!({
            "interview": self.interview_id,
            "summary": self.summary,
            "description": self.description,
            "location": self.location,
            "start": self.start_at,
            "end": self.end_at,
            "timezone": self.timezone,
            "cancelled": self.cancelled,
        })
    }

    pub fn content_hash(&self) -> String {
        let digest = Sha256::digest(self.canonical().to_string().as_bytes());
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }
}

pub trait CalendarProvider: Send + Sync {
    fn provider(&self) -> ProviderId;
    /// Events overlapping `[time_min, time_max)` on the primary calendar.
    fn list_events<'a>(
        &'a self,
        time_min: i64,
        time_max: i64,
    ) -> BoxFuture<'a, AppResult<Vec<CalendarEvent>>>;
    /// Busy periods (free/busy, no event details).
    fn get_availability<'a>(
        &'a self,
        time_min: i64,
        time_max: i64,
    ) -> BoxFuture<'a, AppResult<Vec<BusyBlock>>>;
    /// `None` if the event no longer exists (deleted in the calendar).
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

/// Existing events that overlap an interview (with `buffer_ms` of
/// preparation time on both sides) and block time. ReMa's own event for the
/// interview, free events and all-day events do not count. Nothing is moved:
/// conflicts are only reported.
pub fn find_conflicts(
    events: &[CalendarEvent],
    start_at: i64,
    end_at: i64,
    interview_id: i64,
    buffer_ms: i64,
) -> Vec<CalendarEvent> {
    let (start, end) = (start_at - buffer_ms, end_at + buffer_ms);
    events
        .iter()
        .filter(|e| e.interview_id != Some(interview_id))
        .filter(|e| !e.all_day && !e.transparent)
        .filter(|e| match (e.start_at, e.end_at) {
            (Some(s), Some(e_end)) => s < end && start < e_end,
            _ => false,
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            event("dentist", 10 * HOUR + HOUR / 2, 11 * HOUR + HOUR / 2),
            event("before", 9 * HOUR, 10 * HOUR),
            event("after", 11 * HOUR, 12 * HOUR),
            event("inside", 10 * HOUR + 10, 10 * HOUR + 20),
            own,
            free,
            all_day,
        ];
        let ids: Vec<_> = find_conflicts(&events, start, end, 7, 0)
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["dentist", "inside"]);
    }

    #[test]
    fn a_preparation_buffer_turns_back_to_back_events_into_conflicts() {
        let events = vec![event("before", 9 * HOUR, 10 * HOUR)];
        assert!(find_conflicts(&events, 10 * HOUR, 11 * HOUR, 1, 0).is_empty());
        assert_eq!(
            find_conflicts(&events, 10 * HOUR, 11 * HOUR, 1, 15 * 60_000).len(),
            1
        );
    }

    #[test]
    fn content_hash_changes_with_the_time_only() {
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
        assert_eq!(draft.content_hash(), draft.clone().content_hash());
        let moved = EventDraft {
            start_at: draft.start_at + HOUR,
            ..draft.clone()
        };
        assert_ne!(moved.content_hash(), draft.content_hash());
    }
}
