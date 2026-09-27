//! The Job Application Mail Monitor: synchronizes connected mailboxes,
//! runs the job pipeline and handles interview calendar events.
//!
//! When it runs:
//! - **While ReMa runs** (window open, or in the tray with "Run ReMa in
//!   background"): each mail connector with background sync on is checked at
//!   the configured interval. A background worker wakes at least once a
//!   minute and runs only the connectors that are due.
//! - **Sync now** (Settings → Connectors) runs a connector at once.
//! - **Scheduled tasks** of the "Job Application Mail Monitor" kind run it
//!   on their own schedule with their own model.
//! - **When ReMa is not running, nothing runs.** There is no hidden service;
//!   the user can choose "Start ReMa at login" instead.
//!
//! At most one synchronization per connector runs at a time.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::{
    connectors::{self, calendar::CalendarProvider, sync},
    db::connectors as repo,
    error::{AppError, AppResult},
    jobs::{self, MailSource, RunConfig, RunModel, Sources},
    models::{
        connectors::{ConnectorId, ConnectorKind},
        jobs::JobRunReport,
        provider::ModelRef,
    },
    services::providers,
    state::AppState,
    time::now_ms,
};

/// Longest sleep between checks (also covers sleep/wake and clock changes).
const TICK: Duration = Duration::from_secs(60);
/// Longest a single sync may take.
const SYNC_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Days of mail a first sync reads.
pub const FIRST_SYNC_DAYS: u32 = 14;

/// Starts the background worker.
pub fn start(state: AppState) {
    tauri::async_runtime::spawn(async move {
        // Old Google Workspace settings move to connectors first.
        if let Err(error) = connectors::legacy::migrate(&state).await {
            eprintln!("connectors: the old Google settings could not be moved: {error}");
        }
        let ctx = state.connectors.clone();
        loop {
            tick(&state);
            tokio::select! {
                _ = ctx.shutdown.cancelled() => break,
                _ = ctx.wake.notified() => {}
                _ = tokio::time::sleep(TICK) => {}
            }
        }
    });
}

/// Starts every mail connector whose background sync is due.
pub fn tick(state: &AppState) -> Vec<ConnectorId> {
    let now = now_ms();
    let Ok(records) = state.db.call(|c| repo::connectors(c)) else {
        return Vec::new();
    };
    let mut started = Vec::new();
    for record in records {
        let due = record.enabled
            && record.background_sync
            && record.id.kind() == ConnectorKind::Mail
            && record.next_sync_at.is_some_and(|at| at <= now);
        if due && !state.connectors.is_syncing(record.id) && spawn_sync(state, record.id).is_ok() {
            started.push(record.id);
        }
    }
    started
}

/// "Sync now": runs a connector in the background; the card shows it.
pub fn spawn_sync(state: &AppState, id: ConnectorId) -> AppResult<()> {
    let guard = sync::begin(&state.connectors, id)
        .ok_or_else(|| AppError::validation(format!("{} is already syncing.", id.name())))?;
    let background = state.clone();
    tauri::async_runtime::spawn(async move {
        let _ = run_connector(&background, id, guard).await;
    });
    state.events.connectors_changed();
    Ok(())
}

/// The model for background classification: the default model, else the
/// first enabled one. `None` when no model is set up.
pub fn background_model(state: &AppState) -> Option<ModelRef> {
    let catalog = providers::catalog(state).ok()?;
    catalog
        .default_model
        .or_else(|| catalog.models.first().map(|m| m.model.clone()))
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
        let _ = state
            .db
            .call(|c| repo::sync_finished(c, id, now, None, None));
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
            "The provider is rate limiting ReMa. The next sync tries again.".to_string()
        }
        AppError::Provider(text) if text.contains("could not be reached") => {
            "The provider could not be reached. Check your connection; the next sync tries again."
                .to_string()
        }
        _ => "The last sync failed. Try again.".to_string(),
    };
    (message, detail)
}

/// Runs one connector: a mailbox through the job pipeline (with the
/// connected calendars), or a calendar through the interview step.
async fn run_connector(
    state: &AppState,
    id: ConnectorId,
    guard: sync::SyncGuard,
) -> AppResult<Option<JobRunReport>> {
    let now = now_ms();
    state.db.call(|c| repo::sync_started(c, id, now))?;
    state.events.connectors_changed();

    let outcome = tokio::time::timeout(SYNC_TIMEOUT, async {
        match id.kind() {
            ConnectorKind::Mail => run_mailbox(state, id, &guard.cancel, now).await.map(Some),
            ConnectorKind::Calendar => {
                run_calendar(state, id, now).await?;
                Ok(None)
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        Err(AppError::provider(
            "The sync took too long and was stopped.",
        ))
    });

    let finished = now_ms();
    let (record, prefs) = state
        .db
        .call(|c| Ok((repo::connector(c, id)?, repo::preferences(c)?)))?;
    let next = (record.enabled && record.background_sync && id.kind() == ConnectorKind::Mail)
        .then(|| finished + i64::from(prefs.sync_interval_minutes) * 60_000);
    let error = outcome.as_ref().err().map(describe);
    state.db.call(|c| {
        repo::sync_finished(
            c,
            id,
            finished,
            error.as_ref().map(|(m, d)| (m.as_str(), d.as_str())),
            next,
        )
    })?;
    drop(guard);
    state.events.connectors_changed();
    state.events.applications_changed();
    outcome
}

async fn run_mailbox(
    state: &AppState,
    id: ConnectorId,
    cancel: &CancellationToken,
    now: i64,
) -> AppResult<JobRunReport> {
    let (mailbox, account_id) = sync::mail_client(state, id).await?;
    let last_success = state.db.call(|c| repo::connector(c, id))?.last_success_at;
    let calendars = sync::ready_calendars(state).await;
    let model_ref = background_model(state);
    let endpoint = match &model_ref {
        Some(m) => providers::resolve_endpoint(state, &m.provider_id)
            .await
            .ok(),
        None => None,
    };
    let max_output_tokens = match &model_ref {
        Some(m) => providers::max_output_tokens(state, m).unwrap_or(None),
        None => None,
    };
    let model = match (&model_ref, &endpoint) {
        (Some(model), Some(endpoint)) => Some(RunModel {
            endpoint,
            model,
            max_output_tokens,
        }),
        _ => None,
    };
    let report = jobs::run(
        state,
        model,
        Sources {
            mail: vec![MailSource {
                connector: id,
                provider: mailbox.as_ref(),
                account_id,
                since: jobs::window_start(now, FIRST_SYNC_DAYS, last_success),
            }],
            calendars: calendars.iter().map(|c| c.as_ref()).collect(),
        },
        RunConfig::load(state, "", true)?,
        cancel,
        now,
    )
    .await?;
    calendars_checked(state, &calendars, &report);
    Ok(report)
}

/// A calendar "sync": checks access and runs the interview step with it.
async fn run_calendar(state: &AppState, id: ConnectorId, now: i64) -> AppResult<()> {
    let calendar = sync::calendar_client(state, id).await?;
    // A cheap read confirms access before anything else.
    calendar.list_events(now, now + 86_400_000).await?;
    let config = RunConfig::load(state, "", true)?;
    let mut issues = Vec::new();
    jobs::sync_calendars(
        state,
        &[calendar.as_ref()],
        config.policy,
        now,
        now,
        &mut issues,
    )
    .await?;
    Ok(())
}

/// A scheduled "Job Application Mail Monitor" run with the task's model.
pub async fn run_task(
    state: &AppState,
    model: RunModel<'_>,
    instructions: &str,
    lookback_days: u32,
    calendar: bool,
    cancel: &CancellationToken,
    now: i64,
) -> AppResult<JobRunReport> {
    let ready = connectors::ready(state, ConnectorKind::Mail).await;
    if ready.is_empty() {
        return Err(AppError::configuration(
            "Connect Gmail or Outlook Mail in Settings → Connectors first.",
        ));
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
        let last_success = state.db.call(|c| repo::connector(c, id))?.last_success_at;
        state.db.call(|c| repo::sync_started(c, id, now))?;
        mailboxes.push((id, client, account_id, last_success));
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
            .map(|(id, client, account_id, last)| MailSource {
                connector: *id,
                provider: client.as_ref(),
                account_id: account_id.clone(),
                since: jobs::window_start(now, lookback_days, *last),
            })
            .collect(),
        calendars: calendars.iter().map(|c| c.as_ref()).collect(),
    };
    let result = jobs::run(
        state,
        Some(model),
        sources,
        RunConfig::load(state, instructions, calendar)?,
        cancel,
        now,
    )
    .await;
    let finished = now_ms();
    let error = result.as_ref().err().map(describe);
    let prefs = state.db.call(|c| repo::preferences(c))?;
    for (id, ..) in &mailboxes {
        let record = state.db.call(|c| repo::connector(c, *id))?;
        let next = (record.enabled && record.background_sync)
            .then(|| finished + i64::from(prefs.sync_interval_minutes) * 60_000);
        state.db.call(|c| {
            repo::sync_finished(
                c,
                *id,
                finished,
                error.as_ref().map(|(m, d)| (m.as_str(), d.as_str())),
                next,
            )
        })?;
    }
    drop(guards);
    if let Ok(report) = &result {
        calendars_checked(state, &calendars, report);
    }
    state.events.connectors_changed();
    let mut report = result?;
    for id in busy {
        report.issues.push(format!(
            "{} was already syncing; its new mail is handled by that sync.",
            id.name()
        ));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing};

    #[tokio::test]
    async fn the_worker_starts_only_due_mail_connectors_once() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let now = now_ms();
        state
            .db
            .call(|c| {
                // Enabling schedules a sync right away.
                for id in ConnectorId::ALL {
                    repo::set_enabled(c, id, true, now)?;
                }
                repo::set_background_sync(c, ConnectorId::OutlookMail, false)?;
                repo::schedule_sync(c, ConnectorId::OutlookMail, Some(now - 1))
            })
            .unwrap();

        // Calendars are not synced on their own; Outlook Mail has
        // background sync off.
        assert_eq!(tick(&state), [ConnectorId::Gmail]);
        assert!(tick(&state).is_empty(), "never twice at the same time");

        // The run ends (here: no account, so it fails) and the next one is
        // scheduled an interval later.
        for _ in 0..200 {
            if !state.connectors.is_syncing(ConnectorId::Gmail) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let record = state
            .db
            .call(|c| repo::connector(c, ConnectorId::Gmail))
            .unwrap();
        assert!(record.last_error.is_some());
        assert!(record.next_sync_at.unwrap() > now + 10 * 60_000);
        assert!(tick(&state).is_empty());
    }
}
