use serde::{Deserialize, Serialize};
use specta::Type;

use super::text_enum;

/// Where a job application stands. Stored as text and validated in Rust;
/// new statuses (Offer, Assessment, Withdrawn, …) can be added as variants
/// without a database migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationStatus {
    /// The company confirmed it received the application.
    Confirmed,
    /// Under review / waiting for news.
    InProcess,
    /// The user has to do something (reply, assessment, pick a slot).
    NeedsAction,
    /// A confirmed interview is coming up.
    UpcomingInterview,
    Rejected,
}

text_enum!(ApplicationStatus {
    Confirmed => "confirmed",
    InProcess => "in_process",
    NeedsAction => "needs_action",
    UpcomingInterview => "upcoming_interview",
    Rejected => "rejected",
});

/// One row of the application overview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRow {
    pub id: i64,
    pub company: String,
    pub role: Option<String>,
    pub status: ApplicationStatus,
    /// Received time of the latest email about this application.
    pub last_update_at: i64,
    pub next_action: Option<String>,
    /// For upcoming interviews: when it starts (epoch ms).
    pub interview_at: Option<i64>,
    pub interview_timezone: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CalendarOutcome {
    Created,
    Updated,
    Unchanged,
    /// The interview was cancelled; the event was marked as cancelled.
    Cancelled,
    /// The user deleted ReMa's event in Calendar; it is not recreated.
    Removed,
    /// Details are missing or unclear; nothing was written to Calendar.
    NeedsReview,
}

/// An existing Calendar event overlapping an interview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConflictingEvent {
    pub title: String,
    pub start_at: i64,
    pub end_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarItem {
    pub company: String,
    pub role: Option<String>,
    pub outcome: CalendarOutcome,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub timezone: Option<String>,
    pub conflicts: Vec<ConflictingEvent>,
    /// Why it needs review, or other context.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarReport {
    pub created: u32,
    pub updated: u32,
    pub unchanged: u32,
    pub cancelled: u32,
    pub conflicts: u32,
    pub needs_review: u32,
    pub items: Vec<CalendarItem>,
}

/// Structured result of one job-application monitoring run. Built from
/// stored state by Rust, never from free-form model text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct JobRunReport {
    pub window_start: i64,
    pub window_end: i64,
    /// Candidate emails found by the Gmail search.
    pub emails_checked: u32,
    /// Candidates not processed in an earlier run.
    pub new_emails: u32,
    /// New emails identified as job-application related.
    pub relevant_emails: u32,
    pub applications_updated: u32,
    pub new_applications: u32,
    pub upcoming_interviews: u32,
    pub needs_action: u32,
    pub new_rejections: u32,
    /// Relevant emails left for the next run (per-run limit).
    pub deferred_emails: u32,
    pub applications: Vec<ApplicationRow>,
    pub calendar: Option<CalendarReport>,
    /// Problems with individual emails (never email content).
    pub issues: Vec<String>,
}
