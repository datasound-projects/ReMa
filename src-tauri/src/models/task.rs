use serde::{Deserialize, Serialize};
use specta::Type;

use super::{jobs::JobRunReport, provider::ModelRef, text_enum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum IntervalUnit {
    Minutes,
    Hours,
}

/// How often a task repeats. The time of day for `Daily` and `Weekly` is the
/// local time of the task's start (in the task's timezone).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schedule {
    /// Runs once, at the start time.
    Once,
    /// Every N minutes or hours from the start time (at least 15 minutes).
    Interval { every: u32, unit: IntervalUnit },
    /// Every N days at the start's time of day (`every: 1` is daily).
    Daily { every: u32 },
    /// On the selected weekdays at the start's time of day.
    Weekly { days: Vec<Weekday> },
}

/// When a recurring task stops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EndCondition {
    Never,
    /// No runs after the end of this local date (`YYYY-MM-DD`).
    OnDate {
        date: String,
    },
    /// Stop after this many scheduled runs.
    AfterRuns {
        count: u32,
    },
}

/// What a task does when it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskKind {
    /// Sends the prompt to the model and keeps its answer.
    Prompt,
    /// "Job Mail & Interview Sync" (only as the built-in task): reads the
    /// connected mailboxes (Gmail, Outlook Mail) for job-application emails,
    /// keeps Applications up to date and adds confirmed interviews to the
    /// connected calendar (with conflict checks). The prompt adds the user's
    /// instructions.
    #[serde(rename_all = "camelCase")]
    JobApplications {
        /// Initial lookback: the first run reads the last N days (1–365);
        /// later runs read only new mail. A longer lookback backfills the
        /// newly included days once.
        lookback_days: u32,
        /// Add confirmed interviews to the connected calendar.
        sync_calendar: bool,
    },
}

/// Tasks ReMa provides. Each exists at most once and runs only after the
/// user set it up and turned it on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinTask {
    /// "Job Mail & Interview Sync".
    JobMailSync,
}

text_enum!(BuiltinTask { JobMailSync => "job_mail_sync" });

impl BuiltinTask {
    pub fn name(self) -> &'static str {
        match self {
            Self::JobMailSync => "Job Mail & Interview Sync",
        }
    }
}

/// Lifecycle of a task as shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Enabled and has a next run.
    Active,
    /// Disabled by the user.
    Paused,
    /// No runs left (one-time task done, end date passed or run limit hit).
    Completed,
}

/// Where a run is in its lifecycle: created (queued) before the task
/// executes, then running, then one of the three final states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

text_enum!(ExecutionStatus {
    Queued => "queued",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
    Cancelled => "cancelled",
});

impl ExecutionStatus {
    /// Not finished yet.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

/// Why a run did not succeed; decides the guidance shown with its error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunErrorCategory {
    /// The run took longer than the limit.
    Timeout,
    /// ReMa closed or stopped before the run finished.
    Interrupted,
    /// Stopped by the user.
    Cancelled,
    /// A scheduled occurrence came while the previous run was still going.
    Skipped,
    /// The model declined or returned nothing.
    Model,
    /// The model's provider is not connected or its sign-in expired.
    ModelAccess,
    /// The provider reported an error (rate limit, outage, rejected request).
    Provider,
    /// The provider account has no credits left.
    Billing,
    /// A connected service (mail, calendar) needs attention.
    Connector,
    /// The network could not be reached.
    Network,
    /// No search could run.
    Search,
    /// The task cannot run as it is set up.
    Task,
    Internal,
}

text_enum!(RunErrorCategory {
    Timeout => "timeout",
    Interrupted => "interrupted",
    Cancelled => "cancelled",
    Skipped => "skipped",
    Model => "model",
    ModelAccess => "model_access",
    Provider => "provider",
    Billing => "billing",
    Connector => "connector",
    Network => "network",
    Search => "search",
    Task => "task",
    Internal => "internal",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTrigger {
    Scheduled,
    /// Started with "Run now".
    Manual,
}

text_enum!(ExecutionTrigger { Scheduled => "scheduled", Manual => "manual" });

/// Create or edit a task. Dates and times are local to `timezone`.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub name: String,
    pub kind: TaskKind,
    pub prompt: String,
    /// Give the model the user's Profile (prompt tasks).
    pub use_profile: bool,
    pub model: ModelRef,
    /// IANA timezone, e.g. `Europe/Vienna`.
    pub timezone: String,
    /// `YYYY-MM-DD`
    pub start_date: String,
    /// `HH:MM` (24h)
    pub start_time: String,
    pub schedule: Schedule,
    pub end: EndCondition,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: i64,
    pub name: String,
    pub kind: TaskKind,
    /// Set for ReMa's built-in tasks.
    pub builtin: Option<BuiltinTask>,
    pub prompt: String,
    pub use_profile: bool,
    pub model: ModelRef,
    pub schedule: Schedule,
    pub timezone: String,
    pub start_date: String,
    pub start_time: String,
    pub start_at: i64,
    pub end: EndCondition,
    pub end_at: Option<i64>,
    pub max_runs: Option<u32>,
    /// Scheduled runs started so far (manual runs are not counted).
    pub run_count: u32,
    pub enabled: bool,
    pub status: TaskStatus,
    /// A run is in progress right now.
    pub running: bool,
    pub last_run_at: Option<i64>,
    pub last_run_status: Option<ExecutionStatus>,
    pub next_run_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// The task's schedule when a run was created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleSnapshot {
    pub schedule: Schedule,
    pub timezone: String,
    /// `YYYY-MM-DD` and `HH:MM`, local to `timezone`.
    pub start_date: String,
    pub start_time: String,
}

/// What took part in a run: names and flags only, never credentials.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunContext {
    /// The user's Profile was given to the model.
    pub profile: bool,
    /// Connected services the run used ("Gmail", "Google Calendar").
    pub connectors: Vec<String>,
    /// The run could search the web.
    pub web_search: bool,
    /// Searches that ran.
    pub searches: u32,
    /// What ran them ("ChatGPT web search").
    pub search_engines: Vec<String>,
    /// What the search looked for ("Jobs", "Company"). Runs recorded
    /// before career search have none.
    #[serde(default)]
    pub search_scopes: Vec<String>,
    /// Sources consulted ("ReMa Jobs", "Company career sites", "OpenAI web
    /// search").
    #[serde(default)]
    pub sources_consulted: Vec<String>,
}

/// State of one stage of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

text_enum!(StageStatus {
    Pending => "pending",
    Running => "running",
    Completed => "completed",
    Failed => "failed",
    Skipped => "skipped",
});

/// One stage of a run as it happened ("Searched jobs · 3 searches"):
/// concise activity, never the model's reasoning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunProgressEvent {
    pub id: i64,
    /// Stable key within the run ("search", "mail").
    pub stage: String,
    pub label: String,
    pub status: StageStatus,
    pub started_at: Option<i64>,
    pub updated_at: i64,
}

/// What an output is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunOutputKind {
    /// Listings a job search found (held by Analytics).
    JobSearchResults,
    /// The report of a Job Mail & Interview Sync run.
    ApplicationWatch,
    /// An interview the run added to or changed in a calendar.
    CalendarEvent,
}

text_enum!(RunOutputKind {
    JobSearchResults => "job_search_results",
    ApplicationWatch => "application_watch",
    CalendarEvent => "calendar_event",
});

/// Where an output lives (outputs are references, never copies).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunOutputRef {
    /// The run's own structured report.
    RunReport,
    /// A job search in Analytics.
    JobSearch { id: i64 },
    /// An application in Applications.
    Application { id: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunOutput {
    pub id: i64,
    pub kind: RunOutputKind,
    pub title: String,
    pub reference: RunOutputRef,
    pub created_at: i64,
}

/// One run in a task's history list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunSummary {
    pub id: i64,
    pub task_id: i64,
    pub trigger: ExecutionTrigger,
    pub status: ExecutionStatus,
    /// The occurrence a scheduled run belongs to.
    pub scheduled_for: Option<i64>,
    /// When the run was created.
    pub queued_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub error_category: Option<RunErrorCategory>,
}

/// A page of a task's runs, newest first.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunPage {
    pub runs: Vec<TaskRunSummary>,
    /// Older runs exist (ask again with the last id).
    pub has_more: bool,
}

/// One run with everything it recorded. Its snapshot is the task as it was
/// when the run was created; later edits never change it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub id: i64,
    pub task_id: i64,
    pub trigger: ExecutionTrigger,
    pub status: ExecutionStatus,
    pub scheduled_for: Option<i64>,
    pub queued_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    /// Start to finish, for finished runs.
    pub duration_ms: Option<i64>,
    /// Snapshot fields are `None` for runs recorded before snapshots were
    /// kept (the prompt and model always were).
    pub task_name: Option<String>,
    pub kind: Option<TaskKind>,
    pub prompt: String,
    pub model: ModelRef,
    pub schedule: Option<ScheduleSnapshot>,
    pub use_profile: Option<bool>,
    pub context: Option<RunContext>,
    /// The answer (prompt tasks) or a plain-text summary (job tasks).
    pub result: Option<String>,
    /// Structured result of a Job Mail & Interview Sync run.
    pub report: Option<JobRunReport>,
    /// A safe, user-facing message.
    pub error: Option<String>,
    pub error_category: Option<RunErrorCategory>,
    pub progress: Vec<RunProgressEvent>,
    pub outputs: Vec<RunOutput>,
}

/// Tasks or their runs changed (created, edited, ran, finished).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct TasksChanged;

/// A run was created or changed (status, progress, outputs).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunChanged {
    pub task_id: i64,
    pub run_id: i64,
}
