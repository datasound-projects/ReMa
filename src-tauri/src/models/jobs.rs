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
    /// The company made an offer.
    Offer,
}

text_enum!(ApplicationStatus {
    Confirmed => "confirmed",
    InProcess => "in_process",
    NeedsAction => "needs_action",
    UpcomingInterview => "upcoming_interview",
    Rejected => "rejected",
    Offer => "offer",
});

impl ApplicationStatus {
    /// How the status reads in the timeline ("Application changed to …").
    pub fn label(self) -> &'static str {
        match self {
            Self::Confirmed => "Application confirmed",
            Self::InProcess => "In progress",
            Self::NeedsAction => "Action required",
            Self::UpcomingInterview => "Interview confirmed",
            Self::Rejected => "Rejected",
            Self::Offer => "Offer",
        }
    }

    /// The one Applications section an application with this status is in.
    /// An offer waits for the user's answer.
    pub fn section(self) -> ApplicationSection {
        match self {
            Self::UpcomingInterview => ApplicationSection::InterviewsConfirmed,
            Self::Confirmed => ApplicationSection::ApplicationsConfirmed,
            Self::NeedsAction | Self::Offer => ApplicationSection::NeedsAction,
            Self::InProcess => ApplicationSection::InProgress,
            Self::Rejected => ApplicationSection::Rejected,
        }
    }
}

/// The five sections of the Applications page, in page order. Every
/// application is in exactly one, derived from its current status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationSection {
    InterviewsConfirmed,
    ApplicationsConfirmed,
    NeedsAction,
    InProgress,
    Rejected,
}

/// One row of the Applications page: the application's current state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRow {
    pub id: i64,
    pub company: String,
    pub role: Option<String>,
    pub status: ApplicationStatus,
    pub section: ApplicationSection,
    /// The Status column, e.g. "Interview requested" (from the email that
    /// set the status).
    pub status_label: String,
    /// Received time of the latest email about this application.
    pub last_update_at: i64,
    /// The last column: latest update, requested action or rejection reason.
    pub latest_update: String,
    pub next_action: Option<String>,
    /// Only when the rejection email states one.
    pub rejection_reason: Option<String>,
    /// For confirmed interviews: when the next one starts (epoch ms).
    pub interview_at: Option<i64>,
    pub interview_timezone: Option<String>,
    pub meeting_url: Option<String>,
    /// The confirmed interview overlaps existing events and was not added.
    pub calendar_conflict: bool,
    /// "google" / "microsoft": the mailbox of the latest email.
    pub mail_provider: Option<String>,
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
    /// Free: proposed to the user ("Ask before adding").
    Proposed,
    /// Overlaps existing events: nothing was written; the user was told.
    Conflict,
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
    /// The application the interview belongs to (absent in older reports).
    #[serde(default)]
    pub application_id: Option<i64>,
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

/// What a job-related email is about (the classifier's strict schema).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EmailCategory {
    ApplicationConfirmed,
    ApplicationUpdate,
    NeedsAction,
    RecruiterMessage,
    InterviewRequest,
    InterviewConfirmed,
    InterviewRescheduled,
    InterviewCancelled,
    AssessmentRequest,
    Rejection,
    Offer,
    OtherJobRelated,
    NotJobRelated,
}

text_enum!(EmailCategory {
    ApplicationConfirmed => "application_confirmed",
    ApplicationUpdate => "application_update",
    NeedsAction => "needs_action",
    RecruiterMessage => "recruiter_message",
    InterviewRequest => "interview_request",
    InterviewConfirmed => "interview_confirmed",
    InterviewRescheduled => "interview_rescheduled",
    InterviewCancelled => "interview_cancelled",
    AssessmentRequest => "assessment_request",
    Rejection => "rejection",
    Offer => "offer",
    OtherJobRelated => "other_job_related",
    NotJobRelated => "not_job_related",
});

impl EmailCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::ApplicationConfirmed => "Application confirmed",
            Self::ApplicationUpdate => "Application update",
            Self::NeedsAction => "Action requested",
            Self::RecruiterMessage => "Recruiter message",
            Self::InterviewRequest => "Interview requested",
            Self::InterviewConfirmed => "Interview confirmed",
            Self::InterviewRescheduled => "Interview rescheduled",
            Self::InterviewCancelled => "Interview cancelled",
            Self::AssessmentRequest => "Assessment requested",
            Self::Rejection => "Rejected",
            Self::Offer => "Offer received",
            Self::OtherJobRelated => "Job-related email",
            Self::NotJobRelated => "Not job-related",
        }
    }
}

/// Where a timeline entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdateSource {
    Gmail,
    Outlook,
    GoogleCalendar,
    OutlookCalendar,
    /// Changed by the user in ReMa.
    User,
    /// Changed by the assistant with the user's approval.
    Assistant,
}

text_enum!(UpdateSource {
    Gmail => "gmail",
    Outlook => "outlook",
    GoogleCalendar => "google_calendar",
    OutlookCalendar => "outlook_calendar",
    User => "user",
    Assistant => "assistant",
});

/// One entry of an application's audit timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEntry {
    pub id: i64,
    pub source: UpdateSource,
    pub change: String,
    pub status: ApplicationStatus,
    pub previous_status: Option<ApplicationStatus>,
    pub category: Option<EmailCategory>,
    /// 0–1, for automatic updates.
    pub confidence: Option<f64>,
    pub summary: Option<String>,
    pub occurred_at: i64,
    pub created_at: i64,
}

/// What ReMa did or proposes for an interview's calendar event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CalendarState {
    None,
    /// Free; waiting for the user to add it.
    Proposed,
    /// Overlaps existing events; nothing was written.
    Conflict,
    Created,
    /// The user chose not to add it.
    Declined,
    /// The user deleted ReMa's event in the calendar.
    Removed,
}

text_enum!(CalendarState {
    None => "none",
    Proposed => "proposed",
    Conflict => "conflict",
    Created => "created",
    Declined => "declined",
    Removed => "removed",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InterviewView {
    pub id: i64,
    /// proposed | confirmed | needs_review | cancelled
    pub state: String,
    pub interview_type: Option<String>,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub timezone: Option<String>,
    /// "Sep 29 at 10:00 CEST": the time as the email states it.
    pub time_text: Option<String>,
    pub location: Option<String>,
    pub meeting_url: Option<String>,
    pub participants: Vec<String>,
    pub review_reason: Option<String>,
    pub calendar_state: CalendarState,
    /// "google" / "microsoft" when an event exists or is proposed.
    pub calendar_provider: Option<String>,
    pub conflicts: Vec<ConflictingEvent>,
    /// Times an interview request proposes, with availability.
    pub proposed_slots: Vec<ProposedSlot>,
}

/// A time an interview request proposes, checked against the calendar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProposedSlot {
    pub start_at: i64,
    pub end_at: i64,
    pub timezone: String,
    /// `None` when no calendar is connected.
    pub available: Option<bool>,
    pub conflicts: Vec<ConflictingEvent>,
}

/// A job-related email, shown as correspondence (never its body).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Correspondence {
    /// "google" / "microsoft".
    pub provider: String,
    pub message_id: String,
    pub subject: Option<String>,
    pub sender: Option<String>,
    pub received_at: i64,
    pub category: Option<EmailCategory>,
    pub confidence: Option<f64>,
    /// Opens the message in Gmail / Outlook on the web.
    pub web_link: Option<String>,
    /// processed | ambiguous | failed | pending
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDetail {
    pub application: ApplicationRow,
    pub reference: Option<String>,
    pub timeline: Vec<TimelineEntry>,
    pub interviews: Vec<InterviewView>,
    pub correspondence: Vec<Correspondence>,
}

/// Whether mail is being tracked: only the built-in scheduled task "Job
/// Mail & Interview Sync" monitors mailboxes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackingStatus {
    /// The built-in task, if it was set up.
    pub task_id: Option<i64>,
    /// Set up and turned on.
    pub enabled: bool,
    /// A mail connector (Gmail, Outlook Mail) is connected.
    pub mail_connected: bool,
    pub last_run_at: Option<i64>,
    pub next_run_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationsOverview {
    /// Every application, latest email first; each in one section.
    pub applications: Vec<ApplicationRow>,
    pub tracking: TrackingStatus,
    /// Emails the classifier was not sure about (no status was changed).
    pub needs_review: Vec<Correspondence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationItem {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub application_id: Option<i64>,
    pub created_at: i64,
    pub read: bool,
}

/// Applications or their timeline changed.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ApplicationsChanged;

/// A notification was added or read.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NotificationsChanged;
