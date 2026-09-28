//! Scheduled tasks: validation, creation, editing and history.
//! Timing and execution live in `services::scheduler`.

use crate::{
    connectors,
    db::{
        providers as providers_repo,
        tasks::{self as repo, TaskRow},
    },
    error::{AppError, AppResult},
    models::{
        connectors::ConnectorKind,
        task::{
            BuiltinTask, EndCondition, Schedule, ScheduledTask, TaskInput, TaskKind, TaskStatus,
        },
    },
    services::{
        chat::make_title,
        schedule::{self, Limits},
        scheduler,
    },
    state::AppState,
    time::now_ms,
};

const MAX_PROMPT_CHARS: usize = 20_000;
const MAX_NAME_CHARS: usize = 120;
const MAX_LOOKBACK_DAYS: u32 = crate::jobs::MAX_LOOKBACK_DAYS;
/// "Initial lookback: last N days" when the user does not change it.
pub const DEFAULT_LOOKBACK_DAYS: u32 = 30;
const MAX_RUNS_LIMIT: u32 = 100_000;
/// A start time up to this far in the past still counts as "now".
const START_GRACE_MS: i64 = 60_000;

/// A validated task definition.
struct Definition {
    name: String,
    kind: TaskKind,
    use_profile: bool,
    prompt: String,
    schedule: Schedule,
    timezone: String,
    start_at: i64,
    end_at: Option<i64>,
    max_runs: Option<u32>,
}

fn parse_kind(kind: TaskKind) -> AppResult<TaskKind> {
    match kind {
        TaskKind::Prompt => Ok(kind),
        TaskKind::JobApplications {
            lookback_days,
            sync_calendar,
        } => {
            if !(1..=MAX_LOOKBACK_DAYS).contains(&lookback_days) {
                return Err(AppError::validation(format!(
                    "Choose a lookback of 1 to {MAX_LOOKBACK_DAYS} days."
                )));
            }
            Ok(TaskKind::JobApplications {
                lookback_days,
                sync_calendar,
            })
        }
    }
}

fn parse_input(conn: &rusqlite::Connection, input: &TaskInput) -> AppResult<Definition> {
    let kind = parse_kind(input.kind)?;
    let prompt = input.prompt.trim();
    // Job tasks work without extra instructions.
    if prompt.is_empty() && kind == TaskKind::Prompt {
        return Err(AppError::validation("Enter a prompt for the task."));
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(AppError::validation("The prompt is too long."));
    }
    let name = match input.name.trim() {
        // The built-in task keeps its name.
        _ if kind != TaskKind::Prompt => BuiltinTask::JobMailSync.name().to_string(),
        "" => make_title(prompt),
        name if name.chars().count() > MAX_NAME_CHARS => {
            return Err(AppError::validation("The name is too long."))
        }
        name => name.to_string(),
    };
    if input.model.model_id.trim().is_empty()
        || providers_repo::get(conn, &input.model.provider_id)?.is_none()
    {
        return Err(AppError::validation(
            "Choose a model from a connected provider.",
        ));
    }

    schedule::validate(&input.schedule)?;
    let tz = schedule::timezone(&input.timezone)?;
    let start_at = schedule::local_to_millis(
        schedule::parse_date(&input.start_date)?,
        schedule::parse_time(&input.start_time)?,
        &tz,
    )?;

    let (end_at, max_runs) = match (&input.schedule, &input.end) {
        (Schedule::Once, _) | (_, EndCondition::Never) => (None, None),
        (_, EndCondition::OnDate { date }) => {
            let end_at = schedule::end_of_day_millis(schedule::parse_date(date)?, &tz)?;
            if end_at < start_at {
                return Err(AppError::validation(
                    "The end date must be after the start.",
                ));
            }
            (Some(end_at), None)
        }
        (_, EndCondition::AfterRuns { count }) => {
            if *count == 0 || *count > MAX_RUNS_LIMIT {
                return Err(AppError::validation(
                    "Enter a number of runs of at least 1.",
                ));
            }
            (None, Some(*count))
        }
    };

    Ok(Definition {
        name,
        kind,
        // Job tasks read email, not the profile.
        use_profile: kind == TaskKind::Prompt && input.use_profile,
        prompt: prompt.to_string(),
        schedule: input.schedule.clone(),
        timezone: input.timezone.clone(),
        start_at,
        end_at,
        max_runs,
    })
}

/// The first run of a (new or rescheduled) task.
fn first_run(definition: &Definition, now: i64) -> AppResult<Option<i64>> {
    let tz = schedule::timezone(&definition.timezone)?;
    schedule::next_run(
        &definition.schedule,
        &tz,
        definition.start_at,
        Limits {
            end_at: definition.end_at,
            max_runs: definition.max_runs,
        },
        0,
        Some(now - START_GRACE_MS),
    )
}

fn no_future_runs() -> AppError {
    AppError::validation("This schedule has no upcoming runs. Check the start and end dates.")
}

/// "Job Mail & Interview Sync" needs a connected mailbox. A calendar is
/// optional: without one, confirmed interviews are tracked but not added.
async fn require_tools(state: &AppState, kind: TaskKind) -> AppResult<()> {
    if matches!(kind, TaskKind::JobApplications { .. })
        && connectors::ready(state, ConnectorKind::Mail)
            .await
            .is_empty()
    {
        return Err(AppError::configuration(
            "Connect Gmail or Outlook Mail in Settings → Connectors first.",
        ));
    }
    Ok(())
}

/// Which built-in task a definition is (mail tracking exists only as
/// "Job Mail & Interview Sync").
fn builtin_of(kind: TaskKind) -> Option<BuiltinTask> {
    matches!(kind, TaskKind::JobApplications { .. }).then_some(BuiltinTask::JobMailSync)
}

/// Creates a task. "Job Mail & Interview Sync" is created once, when the
/// user sets it up; it starts turned on because setting it up is the
/// user's explicit choice.
pub async fn create(state: &AppState, input: TaskInput) -> AppResult<ScheduledTask> {
    require_tools(state, input.kind).await?;
    let now = now_ms();
    let builtin = builtin_of(input.kind);
    let id = state.db.call(|conn| {
        if let Some(which) = builtin {
            if repo::builtin(conn, which)?.is_some() {
                return Err(AppError::validation(format!(
                    "{} is already set up. Change its settings instead.",
                    which.name()
                )));
            }
        }
        let definition = parse_input(conn, &input)?;
        let next_run_at = first_run(&definition, now)?.ok_or_else(no_future_runs)?;
        repo::insert(
            conn,
            &TaskRow {
                id: 0,
                name: definition.name,
                kind: definition.kind,
                builtin,
                use_profile: definition.use_profile,
                prompt: definition.prompt,
                model: input.model.clone(),
                schedule: definition.schedule,
                timezone: definition.timezone,
                start_at: definition.start_at,
                end_at: definition.end_at,
                max_runs: definition.max_runs,
                run_count: 0,
                enabled: true,
                completed: false,
                last_run_at: None,
                next_run_at: Some(next_run_at),
                created_at: now,
                updated_at: now,
            },
        )
    })?;
    state.scheduler.wake();
    state.events.tasks_changed();
    get(state, id)
}

/// Edits a task. Changing when it runs restarts its schedule (and run count);
/// editing only the name, prompt or model keeps it.
pub async fn update(state: &AppState, id: i64, input: TaskInput) -> AppResult<ScheduledTask> {
    require_tools(state, input.kind).await?;
    let now = now_ms();
    state.db.call(|conn| {
        let mut task = repo::get(conn, id)?;
        if task.builtin != builtin_of(input.kind) {
            return Err(AppError::validation(
                "A task cannot change between a prompt and Job Mail & Interview Sync.",
            ));
        }
        let definition = parse_input(conn, &input)?;
        let timing_changed = task.schedule != definition.schedule
            || task.timezone != definition.timezone
            || task.start_at != definition.start_at
            || task.end_at != definition.end_at
            || task.max_runs != definition.max_runs;
        if timing_changed {
            let next = first_run(&definition, now)?.ok_or_else(no_future_runs)?;
            task.run_count = 0;
            task.completed = false;
            task.next_run_at = Some(next);
        }
        task.name = definition.name;
        task.kind = definition.kind;
        task.use_profile = definition.use_profile;
        task.prompt = definition.prompt;
        task.model = input.model.clone();
        task.schedule = definition.schedule;
        task.timezone = definition.timezone;
        task.start_at = definition.start_at;
        task.end_at = definition.end_at;
        task.max_runs = definition.max_runs;
        task.updated_at = now;
        repo::save(conn, &task)
    })?;
    state.scheduler.wake();
    state.events.tasks_changed();
    get(state, id)
}

/// Why a mail task other than the built-in one (left from before it
/// existed) never runs.
pub const ONLY_BUILT_IN_READS_MAIL: &str =
    "Only Job Mail & Interview Sync reads mail. Delete this \
     task and set up Job Mail & Interview Sync in Scheduled Tasks.";

pub fn is_old_mail_task(task: &TaskRow) -> bool {
    matches!(task.kind, TaskKind::JobApplications { .. })
        && task.builtin != Some(BuiltinTask::JobMailSync)
}

/// Turns a task on or off. "Job Mail & Interview Sync" can be turned on
/// only with a connected mailbox.
pub async fn set_enabled_checked(
    state: &AppState,
    id: i64,
    enabled: bool,
) -> AppResult<ScheduledTask> {
    if enabled {
        let task = state.db.call(|c| repo::get(c, id))?;
        if is_old_mail_task(&task) {
            return Err(AppError::validation(ONLY_BUILT_IN_READS_MAIL));
        }
        require_tools(state, task.kind).await?;
    }
    set_enabled(state, id, enabled)
}

/// Runs "Job Mail & Interview Sync" now if the user turned it on (the tray
/// menu's action). Does nothing otherwise.
pub fn run_job_mail_sync_now(state: &AppState) -> AppResult<()> {
    match state
        .db
        .call(|c| repo::builtin(c, BuiltinTask::JobMailSync))?
    {
        Some(task) if task.enabled => run_now(state, task.id).map(|_| ()),
        _ => Ok(()),
    }
}

/// The built-in "Job Mail & Interview Sync", if set up.
pub fn job_mail_sync(state: &AppState) -> AppResult<Option<ScheduledTask>> {
    match state
        .db
        .call(|c| repo::builtin(c, BuiltinTask::JobMailSync))?
    {
        Some(row) => Ok(Some(to_view(state, row)?)),
        None => Ok(None),
    }
}

/// Pausing keeps the task; resuming continues from the next upcoming run.
pub fn set_enabled(state: &AppState, id: i64, enabled: bool) -> AppResult<ScheduledTask> {
    let now = now_ms();
    state.db.call(|conn| {
        let mut task = repo::get(conn, id)?;
        if enabled && !task.enabled && !task.completed {
            let tz = schedule::timezone(&task.timezone)?;
            task.next_run_at = schedule::next_run(
                &task.schedule,
                &tz,
                task.start_at,
                Limits {
                    end_at: task.end_at,
                    max_runs: task.max_runs,
                },
                task.run_count,
                Some(now.max(task.next_run_at.unwrap_or(0) - 1)),
            )?;
            task.completed = task.next_run_at.is_none();
        }
        task.enabled = enabled;
        task.updated_at = now;
        repo::save(conn, &task)
    })?;
    state.scheduler.wake();
    state.events.tasks_changed();
    get(state, id)
}

pub fn delete(state: &AppState, id: i64) -> AppResult<()> {
    state.db.call(|c| repo::delete(c, id))?;
    state.scheduler.cancel_run(id);
    state.events.tasks_changed();
    Ok(())
}

/// Starts a run immediately and returns its id: a manual run in the same
/// run history as scheduled ones. Does not change the schedule or run count.
pub fn run_now(state: &AppState, id: i64) -> AppResult<i64> {
    let task = state.db.call(|c| repo::get(c, id))?;
    // Mail is read only by the built-in task while it is on, by hand too.
    if is_old_mail_task(&task) {
        return Err(AppError::validation(ONLY_BUILT_IN_READS_MAIL));
    }
    if task.builtin == Some(BuiltinTask::JobMailSync) && !task.enabled {
        return Err(AppError::validation(
            "Turn on Job Mail & Interview Sync first: ReMa reads your mail only while it is on.",
        ));
    }
    scheduler::spawn_run(
        state,
        task,
        crate::models::task::ExecutionTrigger::Manual,
        None,
    )
}

pub fn get(state: &AppState, id: i64) -> AppResult<ScheduledTask> {
    let row = state.db.call(|c| repo::get(c, id))?;
    to_view(state, row)
}

pub fn list(state: &AppState) -> AppResult<Vec<ScheduledTask>> {
    let rows = state.db.call(|c| repo::list(c))?;
    rows.into_iter().map(|row| to_view(state, row)).collect()
}

fn to_view(state: &AppState, row: TaskRow) -> AppResult<ScheduledTask> {
    let tz = schedule::timezone(&row.timezone)?;
    let (start_date, start_time) = schedule::millis_to_local(row.start_at, &tz)?;
    let end = match (row.end_at, row.max_runs) {
        (Some(end_at), _) => EndCondition::OnDate {
            date: schedule::millis_to_local(end_at, &tz)?.0,
        },
        (None, Some(count)) => EndCondition::AfterRuns { count },
        (None, None) => EndCondition::Never,
    };
    let status = if row.completed {
        TaskStatus::Completed
    } else if !row.enabled {
        TaskStatus::Paused
    } else {
        TaskStatus::Active
    };
    let last_run_status = state.db.call(|c| crate::db::runs::last_status(c, row.id))?;
    Ok(ScheduledTask {
        id: row.id,
        running: state.scheduler.is_running(row.id),
        name: row.name,
        kind: row.kind,
        builtin: row.builtin,
        prompt: row.prompt,
        use_profile: row.use_profile,
        model: row.model,
        schedule: row.schedule,
        timezone: row.timezone,
        start_date,
        start_time,
        start_at: row.start_at,
        end,
        end_at: row.end_at,
        max_runs: row.max_runs,
        run_count: row.run_count,
        enabled: row.enabled,
        status,
        last_run_at: row.last_run_at,
        last_run_status,
        next_run_at: if row.enabled { row.next_run_at } else { None },
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        llm::fake::FakeLanguageModel,
        models::{
            provider::{ModelRef, ProviderKind},
            task::{IntervalUnit, Weekday},
        },
        services::providers,
        state::testing,
    };

    pub async fn state() -> AppState {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&["done"])));
        providers::connect(&state, ProviderKind::Anthropic, "k")
            .await
            .unwrap();
        state
    }

    pub fn input(schedule: Schedule, end: EndCondition) -> TaskInput {
        let tomorrow = jiff::Zoned::now().date().tomorrow().unwrap();
        TaskInput {
            name: String::new(),
            kind: TaskKind::Prompt,
            prompt: "Research new AI engineering jobs in Vienna".into(),
            use_profile: false,
            model: ModelRef {
                provider_id: "anthropic".into(),
                model_id: "model-a".into(),
            },
            timezone: "Europe/Vienna".into(),
            start_date: tomorrow.to_string(),
            start_time: "08:00".into(),
            schedule,
            end,
        }
    }

    #[tokio::test]
    async fn creates_tasks_with_a_first_run() {
        let state = state().await;
        let task = create(
            &state,
            input(Schedule::Daily { every: 1 }, EndCondition::Never),
        )
        .await
        .unwrap();
        assert_eq!(task.name, "Research new AI engineering jobs in Vienna");
        assert_eq!(task.status, TaskStatus::Active);
        assert_eq!(task.next_run_at, Some(task.start_at));
        assert_eq!(task.start_time, "08:00");
        assert_eq!(list(&state).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rejects_invalid_tasks_in_rust() {
        let state = state().await;
        let too_often = input(
            Schedule::Interval {
                every: 5,
                unit: IntervalUnit::Minutes,
            },
            EndCondition::Never,
        );
        assert!(matches!(
            create(&state, too_often).await,
            Err(AppError::Validation(_))
        ));

        let mut past = input(Schedule::Once, EndCondition::Never);
        past.start_date = "2020-01-01".into();
        assert!(matches!(
            create(&state, past).await,
            Err(AppError::Validation(_))
        ));

        let mut unknown_model = input(Schedule::Once, EndCondition::Never);
        unknown_model.model.provider_id = "gemini".into();
        assert!(matches!(
            create(&state, unknown_model).await,
            Err(AppError::Validation(_))
        ));

        let mut empty = input(Schedule::Once, EndCondition::Never);
        empty.prompt = " ".into();
        assert!(matches!(
            create(&state, empty).await,
            Err(AppError::Validation(_))
        ));

        let ends_before_start = input(
            Schedule::Daily { every: 1 },
            EndCondition::OnDate {
                date: "2020-01-01".into(),
            },
        );
        assert!(matches!(
            create(&state, ends_before_start).await,
            Err(AppError::Validation(_))
        ));
        assert!(list(&state).unwrap().is_empty());
    }

    #[tokio::test]
    async fn stores_end_conditions() {
        let state = state().await;
        let limited = create(
            &state,
            input(
                Schedule::Weekly {
                    days: vec![Weekday::Mon],
                },
                EndCondition::AfterRuns { count: 3 },
            ),
        )
        .await
        .unwrap();
        assert_eq!(limited.max_runs, Some(3));
        assert_eq!(limited.end, EndCondition::AfterRuns { count: 3 });

        let end_date = jiff::Zoned::now()
            .date()
            .checked_add(jiff::ToSpan::days(30))
            .unwrap()
            .to_string();
        let dated = create(
            &state,
            input(
                Schedule::Interval {
                    every: 4,
                    unit: IntervalUnit::Hours,
                },
                EndCondition::OnDate {
                    date: end_date.clone(),
                },
            ),
        )
        .await
        .unwrap();
        assert_eq!(dated.end, EndCondition::OnDate { date: end_date });
    }

    #[tokio::test]
    async fn editing_the_prompt_keeps_the_schedule_but_rescheduling_resets_it() {
        let state = state().await;
        let task = create(
            &state,
            input(Schedule::Daily { every: 1 }, EndCondition::Never),
        )
        .await
        .unwrap();
        state
            .db
            .call(|c| {
                let mut row = repo::get(c, task.id)?;
                row.run_count = 2;
                repo::save(c, &row)
            })
            .unwrap();

        let mut renamed = input(Schedule::Daily { every: 1 }, EndCondition::Never);
        renamed.name = "Vienna jobs".into();
        renamed.model.model_id = "model-b".into();
        let edited = update(&state, task.id, renamed).await.unwrap();
        assert_eq!((edited.name.as_str(), edited.run_count), ("Vienna jobs", 2));
        assert_eq!(edited.model.model_id, "model-b");

        let rescheduled = update(
            &state,
            task.id,
            input(Schedule::Daily { every: 2 }, EndCondition::Never),
        )
        .await
        .unwrap();
        assert_eq!(rescheduled.run_count, 0);
    }

    /// Gmail connected (a token in the vault, granted scopes).
    async fn connect_gmail(state: &AppState) {
        use crate::{
            db::connectors::{self as connector_repo, AccountRecord, AccountStatus},
            models::connectors::{ConnectorId, ProviderId},
            secrets::Credential,
        };
        state
            .vault
            .set(
                &crate::connectors::tokens::legacy_key(ProviderId::Google),
                Credential::OAuth {
                    access_token: "at".into(),
                    refresh_token: Some("rt".into()),
                    expires_at: Some(now_ms() + 3_600_000),
                },
            )
            .await
            .unwrap();
        let now = now_ms();
        state
            .db
            .call(|c| {
                connector_repo::save_account(
                    c,
                    &AccountRecord {
                        provider: ProviderId::Google,
                        account_id: Some("g-1".into()),
                        email: Some("ana@gmail.com".into()),
                        display_name: None,
                        granted_scopes: crate::connectors::google::scopes(&[ConnectorId::Gmail]),
                        status: AccountStatus::Connected,
                        status_reason: None,
                        status_cause: None,
                        connected_at: now,
                        updated_at: now,
                    },
                )?;
                connector_repo::set_enabled(c, ConnectorId::Gmail, true, now)
            })
            .unwrap();
    }

    fn mail_task(lookback_days: u32) -> TaskInput {
        TaskInput {
            name: "anything".into(),
            kind: TaskKind::JobApplications {
                lookback_days,
                sync_calendar: true,
            },
            prompt: String::new(),
            ..input(Schedule::Daily { every: 1 }, EndCondition::Never)
        }
    }

    #[tokio::test]
    async fn job_mail_sync_is_a_single_built_in_task_that_needs_a_mailbox() {
        let state = state().await;
        // Nothing is set up: nothing runs, and "Run now" from the tray is a no-op.
        assert!(job_mail_sync(&state).unwrap().is_none());
        run_job_mail_sync_now(&state).unwrap();
        assert!(list(&state).unwrap().is_empty());

        // A mailbox is required.
        let error = create(&state, mail_task(30)).await.unwrap_err();
        assert!(error.to_string().contains("Connect Gmail or Outlook Mail"));

        connect_gmail(&state).await;
        let task = create(&state, mail_task(30)).await.unwrap();
        assert_eq!(task.builtin, Some(BuiltinTask::JobMailSync));
        assert_eq!(task.name, "Job Mail & Interview Sync", "the name is fixed");
        assert!(task.enabled, "set up means turned on");
        assert!(!task.use_profile, "mail tracking never sends the Profile");
        assert!(matches!(
            task.kind,
            TaskKind::JobApplications {
                lookback_days: 30,
                ..
            }
        ));

        // Once only.
        assert!(create(&state, mail_task(14)).await.is_err());
        // Lookback bounds.
        assert!(update(&state, task.id, mail_task(0)).await.is_err());
        assert!(update(&state, task.id, mail_task(400)).await.is_err());
        let longer = update(&state, task.id, mail_task(60)).await.unwrap();
        assert!(matches!(
            longer.kind,
            TaskKind::JobApplications {
                lookback_days: 60,
                ..
            }
        ));
        // It stays what it is.
        let mut prompt = input(Schedule::Daily { every: 1 }, EndCondition::Never);
        prompt.name = "x".into();
        assert!(update(&state, task.id, prompt).await.is_err());

        // Turned off: no scheduled runs and the tray action does nothing.
        let off = set_enabled_checked(&state, task.id, false).await.unwrap();
        assert_eq!(off.next_run_at, None);
        run_job_mail_sync_now(&state).unwrap();
        assert!(!state.scheduler.is_running(task.id));
    }

    #[tokio::test]
    async fn a_mail_task_other_than_the_built_in_one_never_runs() {
        let state = state().await;
        connect_gmail(&state).await;
        // A mail task left from before the built-in one existed (the
        // migration keeps it, paused).
        let task = create(&state, mail_task(30)).await.unwrap();
        state
            .db
            .call(|c| {
                c.execute(
                    "UPDATE scheduled_tasks SET builtin = NULL, enabled = 0 WHERE id = ?1",
                    [task.id],
                )?;
                Ok(())
            })
            .unwrap();
        let error = set_enabled_checked(&state, task.id, true)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), ONLY_BUILT_IN_READS_MAIL);
        assert_eq!(
            run_now(&state, task.id).unwrap_err().to_string(),
            ONLY_BUILT_IN_READS_MAIL
        );
        assert!(job_mail_sync(&state).unwrap().is_none());
    }

    #[tokio::test]
    async fn turning_job_mail_sync_on_needs_a_connected_mailbox() {
        let state = state().await;
        connect_gmail(&state).await;
        let task = create(&state, mail_task(30)).await.unwrap();
        set_enabled_checked(&state, task.id, false).await.unwrap();
        // Paused: not even "Run now" reads mail.
        let error = run_now(&state, task.id).unwrap_err();
        assert!(error
            .to_string()
            .contains("Turn on Job Mail & Interview Sync"));
        crate::connectors::disconnect(&state, crate::models::connectors::ConnectorId::Gmail)
            .await
            .unwrap();
        let error = set_enabled_checked(&state, task.id, true)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Connect Gmail or Outlook Mail"));
        // A run while no mailbox is connected fails with the same advice and
        // reads nothing.
        let error = crate::services::mail_monitor::run_task(
            &state,
            crate::jobs::RunModel {
                endpoint: &crate::llm::Endpoint {
                    kind: ProviderKind::Anthropic,
                    name: "Anthropic".into(),
                    connection: crate::models::provider::ConnectionMethod::ApiKey,
                    base_url: "http://localhost".into(),
                    credential: None,
                    server_web_search: false,
                },
                model: &task.model,
                max_output_tokens: None,
            },
            crate::services::mail_monitor::MailTask {
                instructions: "",
                lookback_days: 30,
                calendar: true,
            },
            &tokio_util::sync::CancellationToken::new(),
            now_ms(),
            std::sync::Arc::new(crate::services::runs::NoActivity),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Connect Gmail or Outlook Mail"));
    }

    #[tokio::test]
    async fn pausing_hides_the_next_run() {
        let state = state().await;
        let task = create(
            &state,
            input(Schedule::Daily { every: 1 }, EndCondition::Never),
        )
        .await
        .unwrap();
        let paused = set_enabled(&state, task.id, false).unwrap();
        assert_eq!(
            (paused.status, paused.next_run_at),
            (TaskStatus::Paused, None)
        );
        let resumed = set_enabled(&state, task.id, true).unwrap();
        assert_eq!(resumed.status, TaskStatus::Active);
        assert_eq!(resumed.next_run_at, task.next_run_at);

        delete(&state, task.id).unwrap();
        assert!(matches!(get(&state, task.id), Err(AppError::NotFound(_))));
    }
}
