//! The deterministic task scheduler.
//!
//! A background loop wakes when the earliest task is due (at least once a
//! minute, and immediately when tasks change), claims every due run and
//! executes it. Claiming advances the schedule *before* the run starts, so a
//! run is never started twice. If ReMa was closed through several
//! occurrences, the task runs once on the next start and then continues
//! with the next upcoming occurrence.
//!
//! The LLM only answers the stored prompt; it never decides when to run.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::{
    db::tasks::{self as repo, Finished, NewExecution, TaskRow},
    error::{AppError, AppResult},
    integrations::google::{self, calendar::CalendarApi, calendar::HttpCalendar, gmail::HttpGmail},
    jobs::{self, JobRunConfig, RunModel, Tools},
    llm::{ChatRequest, Finish, Turn},
    models::{
        chat::MessageRole,
        google::GoogleService,
        jobs::JobRunReport,
        task::{ExecutionStatus, ExecutionTrigger, TaskExecution, TaskKind},
    },
    services::{
        chat::system_prompt,
        providers,
        schedule::{self, Limits},
    },
    state::AppState,
    time::now_ms,
};

/// Longest time a single run may take.
const RUN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Longest sleep between checks (also covers clock changes and sleep/wake).
const MAX_IDLE: Duration = Duration::from_secs(60);

#[derive(Clone, Default)]
pub struct SchedulerHandle {
    wake: Arc<Notify>,
    running: Arc<Mutex<HashSet<i64>>>,
    cancels: Arc<Mutex<HashMap<i64, CancellationToken>>>,
    shutdown: CancellationToken,
}

impl SchedulerHandle {
    /// Re-check tasks now (after they were created or changed).
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    pub fn is_running(&self, task_id: i64) -> bool {
        self.running.lock().unwrap().contains(&task_id)
    }

    fn try_mark_running(&self, task_id: i64) -> Option<CancellationToken> {
        if !self.running.lock().unwrap().insert(task_id) {
            return None;
        }
        let token = self.shutdown.child_token();
        self.cancels.lock().unwrap().insert(task_id, token.clone());
        Some(token)
    }

    fn mark_finished(&self, task_id: i64) {
        self.running.lock().unwrap().remove(&task_id);
        self.cancels.lock().unwrap().remove(&task_id);
    }

    /// Cancels a task's current run (e.g. when the task is deleted).
    pub fn cancel_run(&self, task_id: i64) {
        if let Some(token) = self.cancels.lock().unwrap().get(&task_id) {
            token.cancel();
        }
    }

    /// Stops the loop and cancels running tasks.
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }
}

/// Starts the background loop.
pub fn start(state: AppState) {
    tauri::async_runtime::spawn(async move {
        let handle = state.scheduler.clone();
        loop {
            if let Err(error) = tick(&state, now_ms()) {
                eprintln!("scheduler: {error}");
            }
            let wait = state
                .db
                .call(|c| repo::earliest_next_run(c))
                .ok()
                .flatten()
                .map(|next| Duration::from_millis((next - now_ms()).max(0) as u64))
                .unwrap_or(MAX_IDLE)
                .clamp(Duration::from_millis(250), MAX_IDLE);
            tokio::select! {
                _ = handle.shutdown.cancelled() => break,
                _ = handle.wake.notified() => {}
                _ = tokio::time::sleep(wait) => {}
            }
        }
    });
}

/// A claimed scheduled run.
pub struct Claim {
    pub task: TaskRow,
    pub scheduled_for: i64,
}

/// Claims every run due at `now` and starts it. Returns the claims.
pub fn tick(state: &AppState, now: i64) -> AppResult<Vec<i64>> {
    let claims = claim_due(state, now)?;
    let mut started = Vec::new();
    for claim in claims {
        let id = claim.task.id;
        // A task still busy with its previous run skips this occurrence.
        if spawn_run(
            state,
            claim.task,
            ExecutionTrigger::Scheduled,
            Some(claim.scheduled_for),
        )
        .is_ok()
        {
            started.push(id);
        }
    }
    Ok(started)
}

/// Advances every due task to its next run and returns the runs to start.
pub fn claim_due(state: &AppState, now: i64) -> AppResult<Vec<Claim>> {
    let claims = state.db.call(|conn| {
        let tx = conn.transaction()?;
        let mut claims = Vec::new();
        for task in repo::due(&tx, now)? {
            let Some(scheduled_for) = task.next_run_at else {
                continue;
            };
            let run_count = task.run_count + 1;
            let next = match schedule::timezone(&task.timezone) {
                Ok(tz) => schedule::next_run(
                    &task.schedule,
                    &tz,
                    task.start_at,
                    Limits {
                        end_at: task.end_at,
                        max_runs: task.max_runs,
                    },
                    run_count,
                    Some(scheduled_for.max(now)),
                )?,
                Err(_) => None, // corrupt timezone: run once, then stop
            };
            repo::record_claim(&tx, task.id, run_count, next, next.is_none(), now)?;
            claims.push(Claim {
                task: TaskRow {
                    run_count,
                    next_run_at: next,
                    completed: next.is_none(),
                    ..task
                },
                scheduled_for,
            });
        }
        tx.commit()?;
        Ok(claims)
    })?;
    if !claims.is_empty() {
        state.events.tasks_changed();
    }
    Ok(claims)
}

/// Starts a run in the background. Fails if the task is already running.
pub fn spawn_run(
    state: &AppState,
    task: TaskRow,
    trigger: ExecutionTrigger,
    scheduled_for: Option<i64>,
) -> AppResult<()> {
    let cancel = state
        .scheduler
        .try_mark_running(task.id)
        .ok_or_else(|| AppError::validation("This task is already running."))?;
    let background = state.clone();
    tauri::async_runtime::spawn(async move {
        let task_id = task.id;
        if let Err(error) = execute(&background, &task, trigger, scheduled_for, cancel).await {
            eprintln!("scheduler: task {task_id} could not be recorded: {error}");
        }
        background.scheduler.mark_finished(task_id);
        background.events.tasks_changed();
    });
    state.events.tasks_changed();
    Ok(())
}

/// Runs a task's prompt once and records the execution.
pub async fn execute(
    state: &AppState,
    task: &TaskRow,
    trigger: ExecutionTrigger,
    scheduled_for: Option<i64>,
    cancel: CancellationToken,
) -> AppResult<TaskExecution> {
    let started_at = now_ms();
    let execution = state.db.call(|conn| {
        let execution = repo::insert_execution(
            conn,
            NewExecution {
                task_id: task.id,
                trigger,
                scheduled_for,
                started_at,
                model: &task.model,
                prompt: &task.prompt,
            },
        )?;
        repo::set_last_run_at(conn, task.id, started_at)?;
        Ok(execution)
    })?;
    state.events.tasks_changed();

    let outcome =
        tokio::time::timeout(RUN_TIMEOUT, run_task(state, task, started_at, cancel)).await;

    let (status, result, error, report) = match outcome {
        Err(_) => (
            ExecutionStatus::Failed,
            None,
            Some("The run timed out.".to_string()),
            None,
        ),
        Ok(Err(error)) => (ExecutionStatus::Failed, None, Some(error.to_string()), None),
        Ok(Ok(Output {
            finish: Finish::Cancelled,
            ..
        })) => (
            ExecutionStatus::Failed,
            None,
            Some("The run was cancelled.".to_string()),
            None,
        ),
        Ok(Ok(Output {
            finish: Finish::Refused,
            text,
            ..
        })) if text.trim().is_empty() => (
            ExecutionStatus::Failed,
            None,
            Some("The model declined to answer this prompt.".to_string()),
            None,
        ),
        Ok(Ok(Output { text, .. })) if text.trim().is_empty() => (
            ExecutionStatus::Failed,
            None,
            Some("The model returned an empty response.".to_string()),
            None,
        ),
        Ok(Ok(Output { text, report, .. })) => {
            (ExecutionStatus::Succeeded, Some(text), None, report)
        }
    };

    let finished_at = now_ms();
    state.db.call(|conn| {
        repo::finish_execution(
            conn,
            execution.id,
            Finished {
                status,
                result: result.as_deref(),
                error: error.as_deref(),
                report: report.as_ref(),
                finished_at,
            },
        )?;
        repo::get_execution(conn, execution.id)
    })
}

/// What one run produced.
struct Output {
    finish: Finish,
    /// The model's answer, or a job run's summary.
    text: String,
    report: Option<JobRunReport>,
}

async fn run_task(
    state: &AppState,
    task: &TaskRow,
    started_at: i64,
    cancel: CancellationToken,
) -> AppResult<Output> {
    let endpoint = providers::resolve_endpoint(state, &task.model.provider_id).await?;
    let max_output_tokens = providers::max_output_tokens(state, &task.model)?;
    match task.kind {
        TaskKind::Prompt => {
            let request = ChatRequest {
                system: Some(format!(
                    "{} This request is a scheduled task running automatically; \
                     reply with the finished result.",
                    system_prompt(started_at)
                )),
                turns: vec![Turn {
                    role: MessageRole::User,
                    content: task.prompt.clone(),
                }],
                max_output_tokens,
            };
            let mut text = String::new();
            let mut on_delta = |delta: &str| text.push_str(delta);
            let finish = state
                .llm
                .stream_chat(
                    &endpoint,
                    &task.model.model_id,
                    &request,
                    cancel,
                    &mut on_delta,
                )
                .await?;
            Ok(Output {
                finish,
                text,
                report: None,
            })
        }
        TaskKind::JobApplications {
            lookback_days,
            sync_calendar,
            detect_conflicts,
        } => {
            google::require(state, GoogleService::Gmail).await?;
            // Without Calendar the run still updates the overview.
            let calendar_ready = if sync_calendar {
                google::require(state, GoogleService::Calendar)
                    .await
                    .map_err(|e| e.to_string())
            } else {
                Err("Calendar sync is off.".into())
            };
            let token = google::access_token(state).await?;
            let endpoints = &state.google.endpoints;
            let gmail = HttpGmail::new(state.google.http.clone(), &endpoints.gmail, token.clone());
            let calendar = HttpCalendar::new(state.google.http.clone(), &endpoints.calendar, token);
            let report = jobs::run(
                state,
                task.id,
                &task.prompt,
                RunModel {
                    endpoint: &endpoint,
                    model: &task.model,
                    max_output_tokens,
                },
                JobRunConfig {
                    lookback_days,
                    sync_calendar,
                    detect_conflicts,
                },
                Tools {
                    gmail: &gmail,
                    calendar: calendar_ready.map(|()| &calendar as &dyn CalendarApi),
                },
                &cancel,
                started_at,
            )
            .await?;
            Ok(Output {
                finish: Finish::Complete,
                text: jobs::report::summary(&report),
                report: Some(report),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        llm::fake::FakeLanguageModel,
        models::{
            provider::{ModelRef, ProviderKind},
            task::{EndCondition, IntervalUnit, Schedule, TaskInput},
        },
        services::{providers, tasks},
        state::testing,
    };

    async fn state(llm: FakeLanguageModel) -> AppState {
        let (state, _) = testing::state(Arc::new(llm));
        providers::connect(&state, ProviderKind::Anthropic, "k")
            .await
            .unwrap();
        state
    }

    fn every_4_hours(max_runs: Option<u32>) -> TaskInput {
        TaskInput {
            name: "Market summary".into(),
            kind: crate::models::task::TaskKind::Prompt,
            prompt: "Summarize the market".into(),
            model: ModelRef {
                provider_id: "anthropic".into(),
                model_id: "model-a".into(),
            },
            timezone: "UTC".into(),
            start_date: jiff::Zoned::now().date().tomorrow().unwrap().to_string(),
            start_time: "08:00".into(),
            schedule: Schedule::Interval {
                every: 4,
                unit: IntervalUnit::Hours,
            },
            end: max_runs.map_or(EndCondition::Never, |count| EndCondition::AfterRuns {
                count,
            }),
        }
    }

    const HOUR: i64 = 3_600_000;

    #[tokio::test]
    async fn claims_due_runs_once_and_advances_the_schedule() {
        let state = state(FakeLanguageModel::replying(&["ok"])).await;
        let task = tasks::create(&state, every_4_hours(None)).await.unwrap();
        let start = task.start_at;

        assert!(
            claim_due(&state, start - 1).unwrap().is_empty(),
            "not due yet"
        );

        let claims = claim_due(&state, start).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].scheduled_for, start);
        assert!(
            claim_due(&state, start).unwrap().is_empty(),
            "never claimed twice"
        );

        let after = tasks::get(&state, task.id).unwrap();
        assert_eq!(after.run_count, 1);
        assert_eq!(after.next_run_at, Some(start + 4 * HOUR));
    }

    #[tokio::test]
    async fn missed_runs_execute_once_then_continue_on_schedule() {
        let state = state(FakeLanguageModel::replying(&["ok"])).await;
        let task = tasks::create(&state, every_4_hours(None)).await.unwrap();
        // ReMa was closed for a day.
        let now = task.start_at + 25 * HOUR;
        assert_eq!(claim_due(&state, now).unwrap().len(), 1);
        assert_eq!(
            tasks::get(&state, task.id).unwrap().next_run_at,
            Some(task.start_at + 28 * HOUR)
        );
    }

    #[tokio::test]
    async fn stops_after_the_run_limit() {
        let state = state(FakeLanguageModel::replying(&["ok"])).await;
        let task = tasks::create(&state, every_4_hours(Some(2))).await.unwrap();
        claim_due(&state, task.start_at).unwrap();
        claim_due(&state, task.start_at + 4 * HOUR).unwrap();
        let done = tasks::get(&state, task.id).unwrap();
        assert_eq!(done.status, crate::models::task::TaskStatus::Completed);
        assert_eq!(done.next_run_at, None);
        assert!(claim_due(&state, task.start_at + 100 * HOUR)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn paused_tasks_are_not_claimed() {
        let state = state(FakeLanguageModel::replying(&["ok"])).await;
        let task = tasks::create(&state, every_4_hours(None)).await.unwrap();
        tasks::set_enabled(&state, task.id, false).unwrap();
        assert!(claim_due(&state, task.start_at + HOUR).unwrap().is_empty());
    }

    #[tokio::test]
    async fn executes_the_stored_prompt_with_the_stored_model() {
        let llm = Arc::new(FakeLanguageModel::replying(&["Three ", "new roles"]));
        let (state, _) = testing::state(llm.clone());
        providers::connect(&state, ProviderKind::Anthropic, "k")
            .await
            .unwrap();
        let task = tasks::create(&state, every_4_hours(None)).await.unwrap();
        let row = state.db.call(|c| repo::get(c, task.id)).unwrap();

        let execution = execute(
            &state,
            &row,
            ExecutionTrigger::Manual,
            None,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(execution.status, ExecutionStatus::Succeeded);
        assert_eq!(execution.result.as_deref(), Some("Three new roles"));
        assert_eq!(execution.model.model_id, "model-a");
        assert!(execution.finished_at.is_some());

        let (model_id, request) = llm.requests.lock().unwrap()[0].clone();
        assert_eq!(model_id, "model-a");
        assert_eq!(request.turns[0].content, "Summarize the market");

        let history = tasks::executions(&state, task.id).unwrap();
        assert_eq!(history.len(), 1);
        assert!(tasks::get(&state, task.id).unwrap().last_run_at.is_some());
    }

    #[tokio::test]
    async fn failed_runs_are_recorded_without_breaking_the_task() {
        let mut llm = FakeLanguageModel::replying(&[]);
        llm.fail_with = Some("Anthropic rate limit or quota reached".into());
        let state = state(llm).await;
        let task = tasks::create(&state, every_4_hours(None)).await.unwrap();
        let row = state.db.call(|c| repo::get(c, task.id)).unwrap();

        let execution = execute(
            &state,
            &row,
            ExecutionTrigger::Scheduled,
            Some(1),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(execution.status, ExecutionStatus::Failed);
        assert!(execution.error.unwrap().contains("rate limit"));
        let after = tasks::get(&state, task.id).unwrap();
        assert_eq!(after.status, crate::models::task::TaskStatus::Active);
        assert_eq!(after.last_run_status, Some(ExecutionStatus::Failed));
    }

    #[tokio::test]
    async fn a_running_task_cannot_start_twice() {
        let mut llm = FakeLanguageModel::replying(&["a", "b"]);
        llm.delay = Duration::from_millis(200);
        let state = state(llm).await;
        let task = tasks::create(&state, every_4_hours(None)).await.unwrap();

        tasks::run_now(&state, task.id).unwrap();
        assert!(matches!(
            tasks::run_now(&state, task.id),
            Err(AppError::Validation(_))
        ));
        assert!(tasks::get(&state, task.id).unwrap().running);

        for _ in 0..100 {
            if !state.scheduler.is_running(task.id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let history = tasks::executions(&state, task.id).unwrap();
        assert_eq!(history[0].status, ExecutionStatus::Succeeded);
        assert_eq!(history[0].trigger, ExecutionTrigger::Manual);
    }
}
