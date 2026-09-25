//! Scheduled tasks and their execution history.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::{
        jobs::JobRunReport,
        provider::ModelRef,
        task::{ExecutionStatus, ExecutionTrigger, Schedule, TaskExecution, TaskKind},
    },
};

/// A task exactly as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    pub id: i64,
    pub name: String,
    pub kind: TaskKind,
    pub prompt: String,
    pub use_profile: bool,
    pub model: ModelRef,
    pub schedule: Schedule,
    pub timezone: String,
    pub start_at: i64,
    pub end_at: Option<i64>,
    pub max_runs: Option<u32>,
    pub run_count: u32,
    pub enabled: bool,
    /// No further runs (the stored `status` is `completed`).
    pub completed: bool,
    pub last_run_at: Option<i64>,
    pub next_run_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

const TASK_COLUMNS: &str = "id, name, prompt, provider_id, model_id, schedule, timezone, start_at,
    end_at, max_runs, run_count, enabled, status, last_run_at, next_run_at, created_at, updated_at,
    kind, use_profile";

const EXECUTION_COLUMNS: &str = "id, task_id, trigger, scheduled_for, started_at, finished_at,
    status, provider_id, model_id, result, error, report";

fn task_from_row(row: &Row) -> rusqlite::Result<TaskRow> {
    let schedule: String = row.get(5)?;
    let status: String = row.get(12)?;
    let kind: String = row.get(17)?;
    Ok(TaskRow {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: serde_json::from_str(&kind).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(17, rusqlite::types::Type::Text, Box::new(e))
        })?,
        use_profile: row.get(18)?,
        prompt: row.get(2)?,
        model: ModelRef {
            provider_id: row.get(3)?,
            model_id: row.get(4)?,
        },
        schedule: serde_json::from_str(&schedule).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?,
        timezone: row.get(6)?,
        start_at: row.get(7)?,
        end_at: row.get(8)?,
        max_runs: row.get(9)?,
        run_count: row.get(10)?,
        enabled: row.get(11)?,
        completed: status == "completed",
        last_run_at: row.get(13)?,
        next_run_at: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn execution_from_row(row: &Row) -> rusqlite::Result<TaskExecution> {
    let trigger: String = row.get(2)?;
    let status: String = row.get(6)?;
    Ok(TaskExecution {
        id: row.get(0)?,
        task_id: row.get(1)?,
        trigger: ExecutionTrigger::parse(&trigger).unwrap_or(ExecutionTrigger::Scheduled),
        scheduled_for: row.get(3)?,
        started_at: row.get(4)?,
        finished_at: row.get(5)?,
        status: ExecutionStatus::parse(&status).unwrap_or(ExecutionStatus::Failed),
        model: ModelRef {
            provider_id: row.get(7)?,
            model_id: row.get(8)?,
        },
        result: row.get(9)?,
        error: row.get(10)?,
        // An unreadable report (older format) is shown as text only.
        report: row
            .get::<_, Option<String>>(11)?
            .and_then(|r| serde_json::from_str(&r).ok()),
    })
}

fn to_json(value: &impl serde::Serialize) -> AppResult<String> {
    serde_json::to_string(value).map_err(|e| AppError::internal(e.to_string()))
}

/// Inserts a task (its `id` is ignored) and returns the new id.
pub fn insert(conn: &Connection, task: &TaskRow) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO scheduled_tasks (name, prompt, provider_id, model_id, schedule, timezone,
             start_at, end_at, max_runs, run_count, enabled, status, last_run_at, next_run_at,
             created_at, updated_at, kind, use_profile)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            task.name,
            task.prompt,
            task.model.provider_id,
            task.model.model_id,
            to_json(&task.schedule)?,
            task.timezone,
            task.start_at,
            task.end_at,
            task.max_runs,
            task.run_count,
            task.enabled,
            if task.completed {
                "completed"
            } else {
                "active"
            },
            task.last_run_at,
            task.next_run_at,
            task.created_at,
            task.updated_at,
            to_json(&task.kind)?,
            task.use_profile,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Writes every column of an existing task.
pub fn save(conn: &Connection, task: &TaskRow) -> AppResult<()> {
    let updated = conn.execute(
        "UPDATE scheduled_tasks SET name = ?2, prompt = ?3, provider_id = ?4, model_id = ?5,
             schedule = ?6, timezone = ?7, start_at = ?8, end_at = ?9, max_runs = ?10,
             run_count = ?11, enabled = ?12, status = ?13, last_run_at = ?14, next_run_at = ?15,
             updated_at = ?16, kind = ?17, use_profile = ?18
         WHERE id = ?1",
        params![
            task.id,
            task.name,
            task.prompt,
            task.model.provider_id,
            task.model.model_id,
            to_json(&task.schedule)?,
            task.timezone,
            task.start_at,
            task.end_at,
            task.max_runs,
            task.run_count,
            task.enabled,
            if task.completed {
                "completed"
            } else {
                "active"
            },
            task.last_run_at,
            task.next_run_at,
            task.updated_at,
            to_json(&task.kind)?,
            task.use_profile,
        ],
    )?;
    if updated == 0 {
        return Err(AppError::not_found("Task not found"));
    }
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> AppResult<TaskRow> {
    conn.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM scheduled_tasks WHERE id = ?1"),
        [id],
        task_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Task not found"))
}

/// Newest first.
pub fn list(conn: &Connection) -> AppResult<Vec<TaskRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM scheduled_tasks ORDER BY created_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], task_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    let deleted = conn.execute("DELETE FROM scheduled_tasks WHERE id = ?1", [id])?;
    if deleted == 0 {
        return Err(AppError::not_found("Task not found"));
    }
    Ok(())
}

/// Records that a scheduled run was claimed: advances the schedule.
pub fn record_claim(
    conn: &Connection,
    id: i64,
    run_count: u32,
    next_run_at: Option<i64>,
    completed: bool,
    now: i64,
) -> AppResult<()> {
    conn.execute(
        "UPDATE scheduled_tasks SET run_count = ?2, next_run_at = ?3, status = ?4, updated_at = ?5
         WHERE id = ?1",
        params![
            id,
            run_count,
            next_run_at,
            if completed { "completed" } else { "active" },
            now
        ],
    )?;
    Ok(())
}

pub fn set_last_run_at(conn: &Connection, id: i64, at: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE scheduled_tasks SET last_run_at = ?2 WHERE id = ?1",
        params![id, at],
    )?;
    Ok(())
}

/// Enabled, unfinished tasks whose next run is at or before `now`.
pub fn due(conn: &Connection, now: i64) -> AppResult<Vec<TaskRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM scheduled_tasks
         WHERE enabled = 1 AND status = 'active' AND next_run_at IS NOT NULL AND next_run_at <= ?1
         ORDER BY next_run_at, id"
    ))?;
    let rows = stmt.query_map([now], task_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The earliest upcoming run across all enabled tasks.
pub fn earliest_next_run(conn: &Connection) -> AppResult<Option<i64>> {
    Ok(conn.query_row(
        "SELECT MIN(next_run_at) FROM scheduled_tasks WHERE enabled = 1 AND status = 'active'",
        [],
        |row| row.get(0),
    )?)
}

pub struct NewExecution<'a> {
    pub task_id: i64,
    pub trigger: ExecutionTrigger,
    pub scheduled_for: Option<i64>,
    pub started_at: i64,
    pub model: &'a ModelRef,
    pub prompt: &'a str,
}

pub fn insert_execution(conn: &Connection, execution: NewExecution) -> AppResult<TaskExecution> {
    conn.execute(
        "INSERT INTO task_executions
             (task_id, trigger, scheduled_for, started_at, status, provider_id, model_id, prompt)
         VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?6, ?7)",
        params![
            execution.task_id,
            execution.trigger.as_str(),
            execution.scheduled_for,
            execution.started_at,
            execution.model.provider_id,
            execution.model.model_id,
            execution.prompt,
        ],
    )?;
    get_execution(conn, conn.last_insert_rowid())
}

pub fn get_execution(conn: &Connection, id: i64) -> AppResult<TaskExecution> {
    conn.query_row(
        &format!("SELECT {EXECUTION_COLUMNS} FROM task_executions WHERE id = ?1"),
        [id],
        execution_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Execution not found"))
}

/// How a run ended.
pub struct Finished<'a> {
    pub status: ExecutionStatus,
    pub result: Option<&'a str>,
    pub error: Option<&'a str>,
    pub report: Option<&'a JobRunReport>,
    pub finished_at: i64,
}

pub fn finish_execution(conn: &Connection, id: i64, finished: Finished) -> AppResult<()> {
    let report = finished.report.map(to_json).transpose()?;
    conn.execute(
        "UPDATE task_executions SET status = ?2, result = ?3, error = ?4, report = ?5, finished_at = ?6
         WHERE id = ?1",
        params![
            id,
            finished.status.as_str(),
            finished.result,
            finished.error,
            report,
            finished.finished_at
        ],
    )?;
    Ok(())
}

/// Newest first.
pub fn list_executions(
    conn: &Connection,
    task_id: i64,
    limit: u32,
) -> AppResult<Vec<TaskExecution>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {EXECUTION_COLUMNS} FROM task_executions WHERE task_id = ?1
         ORDER BY started_at DESC, id DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![task_id, limit], execution_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn last_execution_status(
    conn: &Connection,
    task_id: i64,
) -> AppResult<Option<ExecutionStatus>> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM task_executions WHERE task_id = ?1
             ORDER BY started_at DESC, id DESC LIMIT 1",
            [task_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(status.as_deref().and_then(ExecutionStatus::parse))
}

/// Fails executions left running by a previous session.
pub fn mark_interrupted_executions(conn: &Connection, now: i64) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE task_executions
         SET status = 'failed', finished_at = ?1,
             error = 'Interrupted: ReMa was closed while this task was running.'
         WHERE status = 'running'",
        [now],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::Database, models::task::IntervalUnit};

    pub fn sample_task() -> TaskRow {
        TaskRow {
            id: 0,
            name: "Jobs".into(),
            kind: TaskKind::Prompt,
            prompt: "Find jobs".into(),
            use_profile: true,
            model: ModelRef {
                provider_id: "anthropic".into(),
                model_id: "claude-test".into(),
            },
            schedule: Schedule::Interval {
                every: 4,
                unit: IntervalUnit::Hours,
            },
            timezone: "Europe/Vienna".into(),
            start_at: 1_000,
            end_at: None,
            max_runs: Some(3),
            run_count: 0,
            enabled: true,
            completed: false,
            last_run_at: None,
            next_run_at: Some(1_000),
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn round_trips_tasks() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let id = insert(c, &sample_task())?;
            let mut task = get(c, id)?;
            assert_eq!(
                task,
                TaskRow {
                    id,
                    ..sample_task()
                }
            );

            task.enabled = false;
            task.completed = true;
            task.schedule = Schedule::Once;
            task.kind = TaskKind::JobApplications {
                lookback_days: 7,
                sync_calendar: true,
                detect_conflicts: false,
            };
            save(c, &task)?;
            assert_eq!(get(c, id)?, task);
            assert_eq!(list(c)?.len(), 1);

            delete(c, id)?;
            assert!(matches!(get(c, id), Err(AppError::NotFound(_))));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn finds_due_tasks_only_when_enabled_and_active() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let due_id = insert(c, &sample_task())?;
            insert(
                c,
                &TaskRow {
                    next_run_at: Some(5_000),
                    ..sample_task()
                },
            )?;
            insert(
                c,
                &TaskRow {
                    enabled: false,
                    ..sample_task()
                },
            )?;
            insert(
                c,
                &TaskRow {
                    completed: true,
                    ..sample_task()
                },
            )?;

            let due_now = due(c, 2_000)?;
            assert_eq!(due_now.len(), 1);
            assert_eq!(due_now[0].id, due_id);
            assert_eq!(earliest_next_run(c)?, Some(1_000));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn records_execution_history() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let task = sample_task();
            let id = insert(c, &task)?;
            let first = insert_execution(
                c,
                NewExecution {
                    task_id: id,
                    trigger: ExecutionTrigger::Scheduled,
                    scheduled_for: Some(1_000),
                    started_at: 1_000,
                    model: &task.model,
                    prompt: &task.prompt,
                },
            )?;
            assert_eq!(first.status, ExecutionStatus::Running);
            finish_execution(
                c,
                first.id,
                Finished {
                    status: ExecutionStatus::Succeeded,
                    result: Some("done"),
                    error: None,
                    report: Some(&JobRunReport {
                        emails_checked: 3,
                        ..JobRunReport::default()
                    }),
                    finished_at: 1_500,
                },
            )?;

            let second = insert_execution(
                c,
                NewExecution {
                    task_id: id,
                    trigger: ExecutionTrigger::Manual,
                    scheduled_for: None,
                    started_at: 2_000,
                    model: &task.model,
                    prompt: &task.prompt,
                },
            )?;
            assert_eq!(mark_interrupted_executions(c, 3_000)?, 1);

            let history = list_executions(c, id, 10)?;
            assert_eq!(history.len(), 2);
            assert_eq!(history[0].id, second.id);
            assert_eq!(history[0].status, ExecutionStatus::Failed);
            assert_eq!(history[1].result.as_deref(), Some("done"));
            assert_eq!(
                history[1].report.as_ref().map(|r| r.emails_checked),
                Some(3)
            );
            assert_eq!(history[0].report, None);
            assert_eq!(last_execution_status(c, id)?, Some(ExecutionStatus::Failed));
            Ok(())
        })
        .unwrap();
    }
}
