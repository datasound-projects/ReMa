use serde::{Deserialize, Serialize};
use specta::Type;

use super::{provider::ModelRef, text_enum};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Running,
    Succeeded,
    Failed,
}

text_enum!(ExecutionStatus {
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
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
    pub prompt: String,
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
    pub prompt: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskExecution {
    pub id: i64,
    pub task_id: i64,
    pub trigger: ExecutionTrigger,
    pub scheduled_for: Option<i64>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: ExecutionStatus,
    pub model: ModelRef,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// Tasks or their executions changed (created, edited, ran, finished).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct TasksChanged;
