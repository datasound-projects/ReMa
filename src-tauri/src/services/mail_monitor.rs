//! "Job Mail & Interview Sync": the only automatic use of connected
//! mailboxes.
//!
//! Connecting Gmail or Outlook Mail gives ReMa the ability to read mail; the
//! built-in scheduled task is the user's permission to use it for tracking
//! applications. Mail is read only when that task runs (on its schedule, or
//! with "Run now"), never merely because a mailbox is connected. When ReMa
//! is not running, nothing runs: there is no hidden service (the user can
//! choose "Start ReMa at login").
//!
//! At most one synchronization per mailbox runs at a time.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::{
    connectors::{self, calendar::CalendarProvider, sync},
    db::connectors as repo,
    error::{AppError, AppResult},
    jobs::{self, MailSource, RunConfig, RunModel, Sources},
    models::{
        connectors::{ConnectorId, ConnectorKind},
        jobs::JobRunReport,
    },
    services::runs::{Activity, ActivitySink},
    state::AppState,
    time::now_ms,
};

/// Moves old settings over once at startup. Reads no mail.
pub fn start(state: AppState) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = connectors::legacy::migrate(&state).await {
            eprintln!("connectors: the old Google settings could not be moved: {error}");
        }
    });
}

/// Records a calendar step on the calendars it used (their "Last sync").
fn calendars_checked(
    state: &AppState,
    calendars: &[Box<dyn CalendarProvider>],
    report: &JobRunReport,
) {
    if report.calendar.is_none() {
        return;
    }
    let now = now_ms();
    for calendar in calendars {
        let id = ConnectorId::of(calendar.provider(), ConnectorKind::Calendar);
        let _ = state.db.call(|c| repo::sync_finished(c, id, now, None));
    }
}

/// A user-readable message and technical detail for a failed sync.
fn describe(error: &AppError) -> (String, String) {
    let detail: String = error.to_string().chars().take(300).collect();
    let message = match error {
        AppError::Authentication(_) => {
            "Access was revoked or expired. Reconnect to continue.".to_string()
        }
        AppError::Provider(text) if text.contains("rate limit") => {
            "The provider is rate limiting ReMa. The next run tries again.".to_string()
        }
        AppError::Network(_) => {
            "The provider could not be reached. Check your connection; the next run tries again."
                .to_string()
        }
        AppError::Provider(text) if text.contains("could not be reached") => {
            "The provider could not be reached. Check your connection; the next run tries again."
                .to_string()
        }
        _ => "The last run failed. The next run tries again.".to_string(),
    };
    (message, detail)
}

/// Why no mailbox can be used, in words the user can act on.
async fn no_mailbox(state: &AppState) -> AppError {
    for id in ConnectorId::ALL {
        if id.kind() != ConnectorKind::Mail {
            continue;
        }
        let enabled = state
            .db
            .call(|c| repo::connector(c, id))
            .is_ok_and(|r| r.enabled);
        if enabled {
            if let Err(error) = connectors::require(state, id).await {
                return error;
            }
        }
    }
    AppError::configuration("Connect Gmail or Outlook Mail in Settings → Connectors first.")
}

/// What "Job Mail & Interview Sync" is set to do.
pub struct MailTask<'a> {
    /// The user's instructions (interpretation only).
    pub instructions: &'a str,
    /// Initial lookback in days.
    pub lookback_days: u32,
    /// Add confirmed interviews to the connected calendars.
    pub calendar: bool,
}

/// One run of "Job Mail & Interview Sync" with the task's model, over every
/// connected mailbox (Gmail and Outlook without duplicates):
///
/// - first run: the last `lookback_days` days;
/// - later runs: only what changed since the provider cursor;
/// - a longer lookback: the newly included range, once; a shorter one
///   reads less old mail but deletes nothing.
///
/// Cursors and coverage are stored only when a mailbox was read
/// successfully; a failed run leaves the known state as it was.
pub async fn run_task(
    state: &AppState,
    model: RunModel<'_>,
    task: MailTask<'_>,
    cancel: &CancellationToken,
    now: i64,
    activity: Arc<dyn Activity>,
) -> AppResult<JobRunReport> {
    let MailTask {
        instructions,
        lookback_days,
        calendar,
    } = task;
    let ready = connectors::ready(state, ConnectorKind::Mail).await;
    if ready.is_empty() {
        return Err(no_mailbox(state).await);
    }
    let mut guards = Vec::new();
    let mut mailboxes = Vec::new();
    let mut busy = Vec::new();
    for id in ready {
        let Some(guard) = sync::begin(&state.connectors, id) else {
            busy.push(id);
            continue;
        };
        let (client, account_id) = sync::mail_client(state, id).await?;
        let provider = id.provider();
        let (last_success, cursor, covered) = state.db.call(|c| {
            Ok((
                repo::connector(c, id)?.last_success_at,
                repo::cursor(c, provider, &account_id, jobs::MAIL_RESOURCE)?,
                repo::cursor(c, provider, &account_id, jobs::COVERAGE_RESOURCE)?
                    .and_then(|v| v.parse::<i64>().ok()),
            ))
        })?;
        let plan = jobs::reading_plan(now, lookback_days, cursor.is_some(), last_success, covered);
        state.db.call(|c| repo::sync_started(c, id, now))?;
        mailboxes.push((id, client, account_id, plan));
        guards.push(guard);
    }
    state.events.connectors_changed();
    let calendars = if calendar {
        sync::ready_calendars(state).await
    } else {
        Vec::new()
    };
    let sources = Sources {
        mail: mailboxes
            .iter()
            .map(|(id, client, account_id, plan)| MailSource {
                connector: *id,
                provider: client.as_ref(),
                account_id: account_id.clone(),
                since: plan.since,
                backfill: plan.backfill,
                covered_from: plan.covered_from,
            })
            .collect(),
        calendars: calendars.iter().map(|c| c.as_ref()).collect(),
    };
    let config = RunConfig {
        activity: ActivitySink(activity),
        ..RunConfig::new(instructions, calendar)
    };
    let result = jobs::run(state, Some(model), sources, config, cancel, now).await;
    let finished = now_ms();
    let error = result.as_ref().err().map(describe);
    for (id, ..) in &mailboxes {
        // A mailbox the run could not read keeps its last success (where
        // reading resumes) and shows why, even when the others were read.
        let own = result
            .as_ref()
            .ok()
            .and_then(|outcome| outcome.unread.iter().find(|(unread, _)| unread == id))
            .map(|(_, e)| describe(e));
        let failed = error.as_ref().or(own.as_ref());
        state.db.call(|c| {
            repo::sync_finished(
                c,
                *id,
                finished,
                failed.map(|(m, d)| (m.as_str(), d.as_str())),
            )
        })?;
    }
    drop(guards);
    // Calendars count as checked only when every request to them worked.
    if let Ok(outcome) = &result {
        if !outcome.calendar_failed && outcome.unread.len() < mailboxes.len() {
            calendars_checked(state, &calendars, &outcome.report);
        }
    }
    state.events.connectors_changed();
    let jobs::RunOutcome {
        mut report, unread, ..
    } = result?;
    // A run that could read none of its mailboxes failed (each mailbox keeps
    // its own error); with one read, the others' failures are notes.
    if !mailboxes.is_empty() && unread.len() == mailboxes.len() {
        if let Some((_, error)) = unread.into_iter().next() {
            return Err(error);
        }
    }
    for id in busy {
        report.issues.push(format!(
            "{} was already syncing; its new mail is handled by that run.",
            id.name()
        ));
    }
    Ok(report)
}
