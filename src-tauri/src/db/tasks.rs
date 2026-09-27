//! Scheduled tasks (their runs are in `db::runs`).

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::{
        provider::ModelRef,
        task::{BuiltinTask, Schedule, TaskKind},
    },
};

/// A task exactly as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    pub id: i64,
    pub name: String,
    pub kind: TaskKind,
    pub builtin: Option<BuiltinTask>,
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
    kind, use_profile, builtin";

fn task_from_row(row: &Row) -> rusqlite::Result<TaskRow> {
    let schedule: String = row.get(5)?;
    let status: String = row.get(12)?;
    let kind: String = row.get(17)?;
    let builtin: Option<String> = row.get(19)?;
    Ok(TaskRow {
        builtin: builtin.as_deref().and_then(BuiltinTask::parse),
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

fn to_json(value: &impl serde::Serialize) -> AppResult<String> {
    serde_json::to_string(value).map_err(|e| AppError::internal(e.to_string()))
}

/// Inserts a task (its `id` is ignored) and returns the new id.
pub fn insert(conn: &Connection, task: &TaskRow) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO scheduled_tasks (name, prompt, provider_id, model_id, schedule, timezone,
             start_at, end_at, max_runs, run_count, enabled, status, last_run_at, next_run_at,
             created_at, updated_at, kind, use_profile, builtin)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
             ?19)",
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
            task.builtin.map(BuiltinTask::as_str),
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

/// The built-in task, if the user set it up.
pub fn builtin(conn: &Connection, which: BuiltinTask) -> AppResult<Option<TaskRow>> {
    Ok(conn
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM scheduled_tasks WHERE builtin = ?1"),
            [which.as_str()],
            task_from_row,
        )
        .optional()?)
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{db::Database, models::task::IntervalUnit};

    pub fn sample_task() -> TaskRow {
        TaskRow {
            id: 0,
            name: "Jobs".into(),
            kind: TaskKind::Prompt,
            builtin: None,
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
}
