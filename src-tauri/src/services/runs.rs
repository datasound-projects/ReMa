//! Runs of scheduled tasks: the one lifecycle every execution goes through
//! (scheduled firings, Run now and ReMa's built-in automations).
//!
//! ```text
//! create (queued, with a snapshot of the task) → running → stages and
//! outputs as they happen → succeeded | failed | cancelled
//! ```
//!
//! The record is written at the execution boundary, before the task runs,
//! so crashes and failures still leave a run. What a run keeps is safe to
//! show: names and flags, never credentials; errors pass through
//! [`safe_message`].

use std::{
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use regex::Regex;

use crate::{
    db::{
        runs::{self as repo, Finished, NewRun},
        tasks::TaskRow,
    },
    error::{AppError, AppResult},
    models::{
        jobs::JobRunReport,
        task::{
            ExecutionStatus, ExecutionTrigger, RunContext, RunErrorCategory, RunOutputKind,
            RunOutputRef, ScheduleSnapshot, StageStatus, TaskKind, TaskRun, TaskRunPage,
        },
    },
    services::schedule,
    state::AppState,
    time::now_ms,
};

/// Runs listed per page.
pub const PAGE_SIZE: u32 = 30;
/// Longest error message kept.
const MAX_ERROR_CHARS: usize = 1_000;

/// Where a run reports what it does. The scheduler records it in the run;
/// other callers (tests, chat) may ignore it.
pub trait Activity: Send + Sync {
    /// Declares the stages ahead, in order (shown as pending).
    fn plan(&self, _stages: &[(&str, &str)]) {}
    fn stage(&self, _stage: &str, _status: StageStatus, _label: &str) {}
    /// A connected service took part ("Gmail").
    fn used_connector(&self, _name: &str) {}
    fn output(&self, _kind: RunOutputKind, _title: &str, _reference: RunOutputRef) {}

    fn running(&self, stage: &str, label: &str) {
        self.stage(stage, StageStatus::Running, label);
    }

    fn done(&self, stage: &str, label: &str) {
        self.stage(stage, StageStatus::Completed, label);
    }

    fn skipped(&self, stage: &str, label: &str) {
        self.stage(stage, StageStatus::Skipped, label);
    }
}

/// Records nothing.
pub struct NoActivity;

impl Activity for NoActivity {}

/// A shared [`Activity`] to hand through a pipeline's settings (records
/// nothing unless set).
#[derive(Clone)]
pub struct ActivitySink(pub Arc<dyn Activity>);

impl Default for ActivitySink {
    fn default() -> Self {
        Self(Arc::new(NoActivity))
    }
}

impl std::fmt::Debug for ActivitySink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ActivitySink")
    }
}

impl std::ops::Deref for ActivitySink {
    type Target = dyn Activity;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

/// "Gmail", "Gmail and Outlook Mail", "A, B and C".
pub fn join_names<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let names: Vec<&str> = names.into_iter().collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// "1 message", "3 messages".
pub fn count(n: impl Into<u64>, one: &str, many: &str) -> String {
    let n = n.into();
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The schedule a run belongs to, as the task has it now.
fn schedule_of(task: &TaskRow) -> ScheduleSnapshot {
    let (start_date, start_time) = schedule::timezone(&task.timezone)
        .and_then(|tz| schedule::millis_to_local(task.start_at, &tz))
        .unwrap_or_default();
    ScheduleSnapshot {
        schedule: task.schedule.clone(),
        timezone: task.timezone.clone(),
        start_date,
        start_time,
    }
}

/// What a run is expected to use before it starts; the run adds what it
/// actually used.
fn initial_context(task: &TaskRow) -> RunContext {
    RunContext {
        profile: task.use_profile && task.kind == TaskKind::Prompt,
        ..RunContext::default()
    }
}

/// Creates a queued run with a snapshot of the task as it is now.
pub fn create(
    state: &AppState,
    task: &TaskRow,
    trigger: ExecutionTrigger,
    scheduled_for: Option<i64>,
) -> AppResult<i64> {
    let schedule = schedule_of(task);
    let context = initial_context(task);
    let id = state.db.call(|c| {
        repo::insert(
            c,
            NewRun {
                task_id: task.id,
                trigger,
                scheduled_for,
                queued_at: now_ms(),
                task_name: &task.name,
                kind: task.kind,
                schedule: &schedule,
                use_profile: task.use_profile,
                model: &task.model,
                prompt: &task.prompt,
                context: &context,
            },
        )
    })?;
    state.events.task_run_changed(task.id, id);
    Ok(id)
}

/// A scheduled occurrence that came while the previous run was still going:
/// one run, recorded as skipped.
pub fn record_skipped(state: &AppState, task: &TaskRow, scheduled_for: i64) -> AppResult<i64> {
    let id = create(
        state,
        task,
        ExecutionTrigger::Scheduled,
        Some(scheduled_for),
    )?;
    finish(state, task.id, id, Ending::Skipped)?;
    Ok(id)
}

/// Queued → running.
pub fn start(state: &AppState, task_id: i64, id: i64) -> AppResult<i64> {
    let started_at = now_ms();
    state.db.call(|c| {
        repo::mark_running(c, id, started_at)?;
        crate::db::tasks::set_last_run_at(c, task_id, started_at)
    })?;
    state.events.task_run_changed(task_id, id);
    Ok(started_at)
}

/// Why a run failed, in words that are safe to keep and show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    pub category: RunErrorCategory,
}

impl Failure {
    pub fn new(category: RunErrorCategory, message: impl Into<String>) -> Self {
        Self {
            message: safe_message(&message.into()),
            category,
        }
    }

    /// An application error. `access` is the category of sign-in and setup
    /// problems in this step (the model's provider or a connector).
    pub fn from_error(error: &AppError, access: RunErrorCategory) -> Self {
        let category = match error {
            AppError::Validation(_) | AppError::NotFound(_) => RunErrorCategory::Task,
            AppError::Configuration(_) | AppError::Authentication(_) => access,
            AppError::Provider(_) => RunErrorCategory::Provider,
            AppError::Billing(_) => RunErrorCategory::Billing,
            AppError::Network(_) => RunErrorCategory::Network,
            AppError::Io(_) | AppError::Database(_) | AppError::Internal(_) => {
                RunErrorCategory::Internal
            }
        };
        Self::new(category, error.to_string())
    }
}

/// How a run ended.
pub enum Ending<'a> {
    Succeeded {
        result: &'a str,
        report: Option<&'a JobRunReport>,
    },
    Failed(Failure),
    Cancelled,
    /// A scheduled occurrence that never started (the last run was busy).
    Skipped,
}

/// Records the end of a run (once; a run that already ended stays as it is).
pub fn finish(state: &AppState, task_id: i64, id: i64, ending: Ending) -> AppResult<()> {
    let finished_at = now_ms();
    let finished = match &ending {
        Ending::Succeeded { result, report } => Finished {
            status: ExecutionStatus::Succeeded,
            result: Some(result),
            report: *report,
            error: None,
            error_category: None,
            finished_at,
        },
        Ending::Failed(failure) => Finished {
            status: ExecutionStatus::Failed,
            result: None,
            report: None,
            error: Some(&failure.message),
            error_category: Some(failure.category),
            finished_at,
        },
        Ending::Cancelled => Finished {
            status: ExecutionStatus::Cancelled,
            result: None,
            report: None,
            error: Some("The run was stopped before it finished."),
            error_category: Some(RunErrorCategory::Cancelled),
            finished_at,
        },
        Ending::Skipped => Finished {
            status: ExecutionStatus::Cancelled,
            result: None,
            report: None,
            error: Some("Skipped: the previous run was still in progress."),
            error_category: Some(RunErrorCategory::Skipped),
            finished_at,
        },
    };
    state.db.call(|c| repo::finish(c, id, finished))?;
    state.events.task_run_changed(task_id, id);
    Ok(())
}

pub fn get(state: &AppState, id: i64) -> AppResult<TaskRun> {
    state.db.call(|c| repo::get(c, id))
}

/// A task's runs, newest first; `before` is the last id already shown.
pub fn list(state: &AppState, task_id: i64, before: Option<i64>) -> AppResult<TaskRunPage> {
    state.db.call(|c| {
        crate::db::tasks::get(c, task_id)?; // 404 for unknown tasks
        repo::list(c, task_id, before, PAGE_SIZE)
    })
}

/// Stops a running run (Run now's counterpart).
pub fn cancel(state: &AppState, id: i64) -> AppResult<()> {
    let run = get(state, id)?;
    if !run.status.is_active() {
        return Err(AppError::validation("This run has already finished."));
    }
    state.scheduler.cancel_run(run.task_id);
    Ok(())
}

/// Removes anything that looks like a credential and shortens the text.
/// Provider errors are scrubbed where they arise; this is the last guard
/// before a message is stored with a run.
pub fn safe_message(text: &str) -> String {
    static PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
            // `Authorization: Bearer …`, `api_key=…`, `"refresh_token": "…"`
            (
                r#"(?i)\b(authorization|api[_-]?key|x-api-key|access[_-]?token|refresh[_-]?token|id[_-]?token|client[_-]?secret|code[_-]?verifier|password|secret|token)(["']?\s*[:=]\s*)(bearer\s+)?["']?[^\s"',;&]{4,}"#,
                "$1$2[redacted]",
            ),
            (r"(?i)\bbearer\s+[A-Za-z0-9_\-.~+/]{8,}=*", "Bearer [redacted]"),
            // Well-known key and token shapes.
            (r"\bsk-[A-Za-z0-9_\-]{12,}", "[redacted]"),
            (r"\bya29\.[A-Za-z0-9_\-.]+", "[redacted]"),
            (r"\b1//[A-Za-z0-9_\-]{12,}", "[redacted]"),
            (r"\beyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+", "[redacted]"),
            (r"\bgh[pousr]_[A-Za-z0-9]{20,}", "[redacted]"),
        ]
        .into_iter()
        .map(|(pattern, replacement)| (Regex::new(pattern).expect("valid pattern"), replacement))
        .collect()
    });
    let mut text = text.trim().to_string();
    for (pattern, replacement) in PATTERNS.iter() {
        text = pattern.replace_all(&text, *replacement).into_owned();
    }
    if text.chars().count() > MAX_ERROR_CHARS {
        text = text.chars().take(MAX_ERROR_CHARS).collect::<String>() + "…";
    }
    text
}

/// Writes one run's activity as it happens and tells the interface.
pub struct RunRecorder {
    state: AppState,
    task_id: i64,
    run_id: i64,
    context: Mutex<RunContext>,
    /// When a label-only change was last written (stages that tick often).
    last_tick: Mutex<Option<std::time::Instant>>,
}

/// Label updates within a running stage are written at most this often.
const TICK: Duration = Duration::from_millis(400);

impl RunRecorder {
    pub fn new(state: &AppState, task: &TaskRow, run_id: i64) -> Arc<Self> {
        let context = state
            .db
            .call(|c| repo::get(c, run_id))
            .ok()
            .and_then(|run| run.context)
            .unwrap_or_else(|| initial_context(task));
        Arc::new(Self {
            state: state.clone(),
            task_id: task.id,
            run_id,
            context: Mutex::new(context),
            last_tick: Mutex::new(None),
        })
    }

    pub fn run_id(&self) -> i64 {
        self.run_id
    }

    fn changed(&self) {
        self.state
            .events
            .task_run_changed(self.task_id, self.run_id);
    }

    fn write(&self, what: &str, f: impl FnOnce(&mut rusqlite::Connection) -> AppResult<()>) {
        // Recording must never break the run itself (the task may have been
        // deleted meanwhile, which deletes its runs).
        match self.state.db.call(f) {
            Ok(()) => self.changed(),
            Err(error) => eprintln!("runs: {what} for run {} not recorded: {error}", self.run_id),
        }
    }

    /// Changes what the run records as having taken part.
    pub fn update_context(&self, update: impl FnOnce(&mut RunContext)) {
        let context = {
            let mut context = self.context.lock().unwrap();
            update(&mut context);
            context.clone()
        };
        self.write("context", |c| repo::set_context(c, self.run_id, &context));
    }

    /// A running stage's label changed (a count went up): written now and
    /// then, not on every tick.
    pub fn tick(&self, stage: &str, label: &str) {
        {
            let mut last = self.last_tick.lock().unwrap();
            if last.is_some_and(|at| at.elapsed() < TICK) {
                return;
            }
            *last = Some(std::time::Instant::now());
        }
        self.stage(stage, StageStatus::Running, label);
    }
}

impl Activity for RunRecorder {
    fn plan(&self, stages: &[(&str, &str)]) {
        let now = now_ms();
        self.write("stages", |c| {
            for (stage, label) in stages {
                repo::set_stage(c, self.run_id, stage, label, StageStatus::Pending, now)?;
            }
            Ok(())
        });
    }

    fn stage(&self, stage: &str, status: StageStatus, label: &str) {
        let now = now_ms();
        self.write("a stage", |c| {
            repo::set_stage(c, self.run_id, stage, label, status, now)
        });
    }

    fn used_connector(&self, name: &str) {
        if self
            .context
            .lock()
            .unwrap()
            .connectors
            .iter()
            .any(|n| n == name)
        {
            return;
        }
        self.update_context(|c| c.connectors.push(name.to_string()));
    }

    fn output(&self, kind: RunOutputKind, title: &str, reference: RunOutputRef) {
        let now = now_ms();
        self.write("an output", |c| {
            repo::add_output(c, self.run_id, kind, title, &reference, now).map(|_| ())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_messages_carry_no_credentials() {
        let cases = [
            "HTTP 401: Authorization: Bearer ya29.a0AfH6SMBx-secret-token",
            "request failed: api_key=sk-proj-abcdefghijklmnop1234",
            r#"{"refresh_token": "1//0gAbCdEfGhIjKlMnOp", "access_token":"EwBAbc123456789"}"#,
            "token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl rejected",
            "client_secret=GOCSPX-very-secret-value&code_verifier=abcdefghijklmnopqrstuvwxyz0123456789",
            "x-api-key: sk-ant-api03-abcdefghijklmnopqrstuvwxyz",
            "password: hunter2hunter2",
        ];
        for case in cases {
            let safe = safe_message(case);
            for secret in [
                "ya29.",
                "sk-proj",
                "sk-ant",
                "1//0g",
                "EwBAbc",
                "eyJhbGci",
                "GOCSPX",
                "abcdefghijklmnopqrstuvwxyz0123456789",
                "hunter2",
            ] {
                assert!(!safe.contains(secret), "{secret} left in {safe:?}");
            }
            assert!(safe.contains("[redacted]"), "{safe}");
        }
        // Ordinary messages are left as they are.
        let plain = "Gmail access was revoked or has expired. Reconnect in Settings → Connectors.";
        assert_eq!(safe_message(plain), plain);
        assert_eq!(
            safe_message("The model's token limit was reached."),
            "The model's token limit was reached."
        );
        assert!(safe_message(&"x".repeat(5_000)).chars().count() <= MAX_ERROR_CHARS + 1);
    }

    #[test]
    fn errors_get_the_category_of_their_step() {
        let cases = [
            (
                AppError::authentication("Gmail needs to be reconnected"),
                RunErrorCategory::Connector,
                RunErrorCategory::Connector,
            ),
            (
                AppError::configuration("Anthropic is not connected"),
                RunErrorCategory::ModelAccess,
                RunErrorCategory::ModelAccess,
            ),
            (
                AppError::provider("rate limit"),
                RunErrorCategory::Connector,
                RunErrorCategory::Provider,
            ),
            (
                AppError::Billing("no credits".into()),
                RunErrorCategory::ModelAccess,
                RunErrorCategory::Billing,
            ),
            (
                AppError::network("offline"),
                RunErrorCategory::ModelAccess,
                RunErrorCategory::Network,
            ),
            (
                AppError::validation("paused"),
                RunErrorCategory::ModelAccess,
                RunErrorCategory::Task,
            ),
            (
                AppError::internal("bug"),
                RunErrorCategory::ModelAccess,
                RunErrorCategory::Internal,
            ),
        ];
        for (error, access, expected) in cases {
            assert_eq!(
                Failure::from_error(&error, access).category,
                expected,
                "{error}"
            );
        }
    }
}
