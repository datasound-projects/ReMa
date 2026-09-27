//! The in-app calendar: a view over the connected Google and Outlook
//! calendars (they stay the source of truth; ReMa keeps no calendar of its
//! own).

use serde::{Deserialize, Serialize};
use specta::Type;

use super::connectors::{ConnectorId, ProviderId};

/// A connected calendar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSource {
    pub connector: ConnectorId,
    pub provider: ProviderId,
    /// "Google Calendar" / "Outlook Calendar".
    pub name: String,
    pub account_email: Option<String>,
    /// Could be read now (not waiting for a reconnect).
    pub ready: bool,
}

/// The application a ReMa interview belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InterviewLink {
    pub application_id: i64,
    pub interview_id: i64,
    pub company: String,
    pub role: Option<String>,
    /// The event exists in a connected calendar.
    pub in_calendar: bool,
    /// It overlaps other events and was not added.
    pub conflict: bool,
    pub cancelled: bool,
}

/// One entry of the in-app calendar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEntry {
    /// Unique in the view.
    pub key: String,
    /// The calendar it is in; `None` for a confirmed interview that is in no
    /// calendar (a conflict, or no calendar connected).
    pub provider: Option<ProviderId>,
    pub title: String,
    pub start_at: i64,
    pub end_at: i64,
    pub all_day: bool,
    pub location: Option<String>,
    /// https only.
    pub meeting_url: Option<String>,
    /// Opens the event in Google Calendar / Outlook on the web.
    pub web_link: Option<String>,
    /// Set for ReMa interviews.
    pub interview: Option<InterviewLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarView {
    /// Connected calendar connectors (empty: no calendar connected).
    pub calendars: Vec<CalendarSource>,
    /// Events overlapping the requested range, by start.
    pub entries: Vec<CalendarEntry>,
    /// Calendars that could not be read, in words the user can act on.
    pub problems: Vec<String>,
}
