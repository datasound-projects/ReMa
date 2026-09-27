//! Runs of scheduled tasks: the record every execution writes, its
//! progress stages and its outputs.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::{
        jobs::JobRunReport,
        provider::ModelRef,
        task::{
            ExecutionStatus, ExecutionTrigger, RunContext, RunErrorCategory, RunOutput,
            RunOutputKind, RunOutputRef, RunProgressEvent, ScheduleSnapshot, StageStatus, TaskKind,
            TaskRun, TaskRunPage, TaskRunSummary,
        },
    },
};

/// The message of a run found unfinished when ReMa starts.
pub const INTERRUPTED: &str = "Execution interrupted before completion.";

const SUMMARY_COLUMNS: &str =
    "id, task_id, trigger, status, scheduled_for, queued_at, started_at, finished_at, error_category";

const RUN_COLUMNS: &str = "id, task_id, trigger, status, scheduled_for, queued_at, started_at,
    finished_at, error_category, task_name, kind, schedule, use_profile, provider_id, model_id,
    prompt, context, result, report, error";

fn json<T: serde::de::DeserializeOwned>(value: Option<String>) -> Option<T> {
    // An unreadable value (an older format) is shown as missing.
    value.and_then(|v| serde_json::from_str(&v).ok())
}

fn to_json(value: &impl serde::Serialize) -> AppResult<String> {
    serde_json::to_string(value).map_err(|e| AppError::internal(e.to_string()))
}

fn summary_from_row(row: &Row) -> rusqlite::Result<TaskRunSummary> {
    let trigger: String = row.get(2)?;
    let status: String = row.get(3)?;
    let category: Option<String> = row.get(8)?;
    Ok(TaskRunSummary {
        id: row.get(0)?,
        task_id: row.get(1)?,
        trigger: ExecutionTrigger::parse(&trigger).unwrap_or(ExecutionTrigger::Scheduled),
        status: ExecutionStatus::parse(&status).unwrap_or(ExecutionStatus::Failed),
        scheduled_for: row.get(4)?,
        queued_at: row.get(5)?,
        started_at: row.get(6)?,
        finished_at: row.get(7)?,
        error_category: category.as_deref().and_then(RunErrorCategory::parse),
    })
}

fn run_from_row(row: &Row) -> rusqlite::Result<TaskRun> {
    let summary = summary_from_row(row)?;
    Ok(TaskRun {
        duration_ms: summary
            .started_at
            .zip(summary.finished_at)
            .map(|(start, end)| (end - start).max(0)),
        id: summary.id,
        task_id: summary.task_id,
        trigger: summary.trigger,
        status: summary.status,
        scheduled_for: summary.scheduled_for,
        queued_at: summary.queued_at,
        started_at: summary.started_at,
        finished_at: summary.finished_at,
        error_category: summary.error_category,
        task_name: row.get(9)?,
        kind: json::<TaskKind>(row.get(10)?),
        schedule: json::<ScheduleSnapshot>(row.get(11)?),
        use_profile: row.get(12)?,
        model: ModelRef {
            provider_id: row.get(13)?,
            model_id: row.get(14)?,
        },
        prompt: row.get(15)?,
        context: json::<RunContext>(row.get(16)?),
        result: row.get(17)?,
        report: json::<JobRunReport>(row.get(18)?),
        error: row.get(19)?,
        progress: Vec::new(),
        outputs: Vec::new(),
    })
}

/// A run as it is created: the task as it is right now.
pub struct NewRun<'a> {
    pub task_id: i64,
    pub trigger: ExecutionTrigger,
    pub scheduled_for: Option<i64>,
    pub queued_at: i64,
    pub task_name: &'a str,
    pub kind: TaskKind,
    pub schedule: &'a ScheduleSnapshot,
    pub use_profile: bool,
    pub model: &'a ModelRef,
    pub prompt: &'a str,
    pub context: &'a RunContext,
}

/// Creates a queued run and returns its id.
pub fn insert(conn: &Connection, run: NewRun) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO task_executions
             (task_id, trigger, status, scheduled_for, queued_at, task_name, kind, schedule,
              use_profile, provider_id, model_id, prompt, context)
         VALUES (?1, ?2, 'queued', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            run.task_id,
            run.trigger.as_str(),
            run.scheduled_for,
            run.queued_at,
            run.task_name,
            to_json(&run.kind)?,
            to_json(run.schedule)?,
            run.use_profile,
            run.model.provider_id,
            run.model.model_id,
            run.prompt,
            to_json(run.context)?,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Queued → running. False when the run is no longer queued.
pub fn mark_running(conn: &Connection, id: i64, started_at: i64) -> AppResult<bool> {
    Ok(conn.execute(
        "UPDATE task_executions SET status = 'running', started_at = ?2
         WHERE id = ?1 AND status = 'queued'",
        params![id, started_at],
    )? == 1)
}

pub fn set_context(conn: &Connection, id: i64, context: &RunContext) -> AppResult<()> {
    conn.execute(
        "UPDATE task_executions SET context = ?2 WHERE id = ?1",
        params![id, to_json(context)?],
    )?;
    Ok(())
}

/// How a run ended.
pub struct Finished<'a> {
    pub status: ExecutionStatus,
    pub result: Option<&'a str>,
    pub report: Option<&'a JobRunReport>,
    pub error: Option<&'a str>,
    pub error_category: Option<RunErrorCategory>,
    pub finished_at: i64,
}

/// Records the end of a run that has not ended yet. Returns false for a
/// run that already ended (or no longer exists).
pub fn finish(conn: &Connection, id: i64, finished: Finished) -> AppResult<bool> {
    let report = finished.report.map(to_json).transpose()?;
    let changed = conn.execute(
        "UPDATE task_executions
         SET status = ?2, result = ?3, report = ?4, error = ?5, error_category = ?6,
             finished_at = ?7
         WHERE id = ?1 AND status IN ('queued', 'running')",
        params![
            id,
            finished.status.as_str(),
            finished.result,
            report,
            finished.error,
            finished.error_category.map(RunErrorCategory::as_str),
            finished.finished_at,
        ],
    )?;
    if changed == 1 {
        close_stages(conn, id, finished.status, finished.finished_at)?;
    }
    Ok(changed == 1)
}

/// Stages still open when a run ends: a running stage ends with the run
/// (completed or failed); a stage that never started was skipped.
fn close_stages(conn: &Connection, id: i64, status: ExecutionStatus, now: i64) -> AppResult<()> {
    let running_to = if status == ExecutionStatus::Succeeded {
        StageStatus::Completed
    } else {
        StageStatus::Failed
    };
    conn.execute(
        "UPDATE task_run_events SET status = ?2, updated_at = ?3
         WHERE execution_id = ?1 AND status = 'running'",
        params![id, running_to.as_str(), now],
    )?;
    conn.execute(
        "UPDATE task_run_events SET status = 'skipped', updated_at = ?2
         WHERE execution_id = ?1 AND status = 'pending'",
        params![id, now],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> AppResult<TaskRun> {
    let mut run = conn
        .query_row(
            &format!("SELECT {RUN_COLUMNS} FROM task_executions WHERE id = ?1"),
            [id],
            run_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("This run no longer exists."))?;
    run.progress = stages(conn, id)?;
    run.outputs = outputs(conn, id)?;
    Ok(run)
}

pub fn task_of(conn: &Connection, id: i64) -> AppResult<i64> {
    conn.query_row(
        "SELECT task_id FROM task_executions WHERE id = ?1",
        [id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("This run no longer exists."))
}

/// A task's runs, newest first, `limit` at a time: `before` is the last id
/// of the previous page.
pub fn list(
    conn: &Connection,
    task_id: i64,
    before: Option<i64>,
    limit: u32,
) -> AppResult<TaskRunPage> {
    let limit = limit.clamp(1, 200);
    let mut stmt = conn.prepare(&format!(
        "SELECT {SUMMARY_COLUMNS} FROM task_executions
         WHERE task_id = ?1 AND id < ?2 ORDER BY id DESC LIMIT ?3"
    ))?;
    let mut runs = stmt
        .query_map(
            params![task_id, before.unwrap_or(i64::MAX), limit + 1],
            summary_from_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let has_more = runs.len() > limit as usize;
    runs.truncate(limit as usize);
    Ok(TaskRunPage { runs, has_more })
}

pub fn last_status(conn: &Connection, task_id: i64) -> AppResult<Option<ExecutionStatus>> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM task_executions WHERE task_id = ?1 ORDER BY id DESC LIMIT 1",
            [task_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(status.as_deref().and_then(ExecutionStatus::parse))
}

/// Ends runs a previous session left queued or running: they failed, with
/// no output made up for them.
pub fn mark_interrupted(conn: &Connection, now: i64) -> AppResult<usize> {
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM task_executions WHERE status IN ('queued', 'running')")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for &id in &ids {
        finish(
            conn,
            id,
            Finished {
                status: ExecutionStatus::Failed,
                result: None,
                report: None,
                error: Some(INTERRUPTED),
                error_category: Some(RunErrorCategory::Interrupted),
                finished_at: now,
            },
        )?;
    }
    Ok(ids.len())
}

// ── Progress ───────────────────────────────────────────────────────────

fn stages(conn: &Connection, id: i64) -> AppResult<Vec<RunProgressEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, stage, label, status, started_at, updated_at FROM task_run_events
         WHERE execution_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([id], |r| {
        let status: String = r.get(3)?;
        Ok(RunProgressEvent {
            id: r.get(0)?,
            stage: r.get(1)?,
            label: r.get(2)?,
            status: StageStatus::parse(&status).unwrap_or(StageStatus::Completed),
            started_at: r.get(4)?,
            updated_at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Records a stage (or its new state). Stages keep the order in which they
/// were first recorded; a stage keeps the time it started.
pub fn set_stage(
    conn: &Connection,
    id: i64,
    stage: &str,
    label: &str,
    status: StageStatus,
    now: i64,
) -> AppResult<()> {
    let started = (status != StageStatus::Pending && status != StageStatus::Skipped).then_some(now);
    conn.execute(
        "INSERT INTO task_run_events (execution_id, stage, label, status, started_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (execution_id, stage) DO UPDATE SET
             label = excluded.label,
             status = excluded.status,
             started_at = COALESCE(task_run_events.started_at, excluded.started_at),
             updated_at = excluded.updated_at",
        params![id, stage, label, status.as_str(), started, now],
    )?;
    Ok(())
}

// ── Outputs ────────────────────────────────────────────────────────────

fn outputs(conn: &Connection, id: i64) -> AppResult<Vec<RunOutput>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, title, reference, created_at FROM task_run_outputs
         WHERE execution_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
        ))
    })?;
    let mut outputs = Vec::new();
    for row in rows {
        let (output_id, kind, title, reference, created_at) = row?;
        // Outputs of a kind this version does not know are left out.
        let (Some(kind), Ok(reference)) = (
            RunOutputKind::parse(&kind),
            serde_json::from_str::<RunOutputRef>(&reference),
        ) else {
            continue;
        };
        outputs.push(RunOutput {
            id: output_id,
            kind,
            title,
            reference,
            created_at,
        });
    }
    Ok(outputs)
}

pub fn add_output(
    conn: &Connection,
    id: i64,
    kind: RunOutputKind,
    title: &str,
    reference: &RunOutputRef,
    now: i64,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO task_run_outputs (execution_id, kind, title, reference, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, kind.as_str(), title, to_json(reference)?, now],
    )?;
    Ok(conn.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::{tasks, Database},
        models::task::{IntervalUnit, Schedule},
    };

    fn snapshot() -> ScheduleSnapshot {
        ScheduleSnapshot {
            schedule: Schedule::Interval {
                every: 4,
                unit: IntervalUnit::Hours,
            },
            timezone: "Europe/Vienna".into(),
            start_date: "2026-09-27".into(),
            start_time: "08:00".into(),
        }
    }

    fn new_run<'a>(
        task: &'a tasks::TaskRow,
        schedule: &'a ScheduleSnapshot,
        context: &'a RunContext,
        trigger: ExecutionTrigger,
        at: i64,
    ) -> NewRun<'a> {
        NewRun {
            task_id: task.id,
            trigger,
            scheduled_for: (trigger == ExecutionTrigger::Scheduled).then_some(at),
            queued_at: at,
            task_name: &task.name,
            kind: task.kind,
            schedule,
            use_profile: task.use_profile,
            model: &task.model,
            prompt: &task.prompt,
            context,
        }
    }

    #[test]
    fn records_runs_with_snapshots_progress_and_outputs() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let mut task = tasks::tests::sample_task();
            task.id = tasks::insert(c, &task)?;
            let schedule = snapshot();
            let context = RunContext {
                profile: true,
                ..RunContext::default()
            };
            let first = insert(
                c,
                new_run(
                    &task,
                    &schedule,
                    &context,
                    ExecutionTrigger::Scheduled,
                    1_000,
                ),
            )?;
            let queued = get(c, first)?;
            assert_eq!(queued.status, ExecutionStatus::Queued);
            assert_eq!(queued.started_at, None);
            assert_eq!(queued.task_name.as_deref(), Some("Jobs"));
            assert_eq!(queued.schedule, Some(snapshot()));
            assert_eq!(queued.use_profile, Some(true));
            assert_eq!(queued.context.as_ref().map(|c| c.profile), Some(true));

            assert!(mark_running(c, first, 1_004)?);
            assert!(!mark_running(c, first, 1_005)?, "only a queued run starts");
            set_stage(c, first, "search", "Searching", StageStatus::Pending, 1_004)?;
            set_stage(c, first, "answer", "Writing", StageStatus::Pending, 1_004)?;
            set_stage(c, first, "search", "Searching", StageStatus::Running, 1_010)?;
            set_stage(
                c,
                first,
                "search",
                "Searched · 3 searches",
                StageStatus::Completed,
                1_020,
            )?;
            add_output(
                c,
                first,
                RunOutputKind::JobSearchResults,
                "Job Search Results",
                &RunOutputRef::JobSearch { id: 9 },
                1_030,
            )?;
            assert!(finish(
                c,
                first,
                Finished {
                    status: ExecutionStatus::Succeeded,
                    result: Some("14 jobs"),
                    report: None,
                    error: None,
                    error_category: None,
                    finished_at: 1_050,
                },
            )?);
            assert!(
                !finish(
                    c,
                    first,
                    Finished {
                        status: ExecutionStatus::Failed,
                        result: None,
                        report: None,
                        error: Some("late"),
                        error_category: None,
                        finished_at: 2_000,
                    },
                )?,
                "a finished run is never rewritten"
            );

            let run = get(c, first)?;
            assert_eq!(run.status, ExecutionStatus::Succeeded);
            assert_eq!(run.result.as_deref(), Some("14 jobs"));
            assert_eq!(run.duration_ms, Some(46));
            let stages: Vec<_> = run
                .progress
                .iter()
                .map(|s| (s.stage.as_str(), s.label.as_str(), s.status, s.started_at))
                .collect();
            assert_eq!(
                stages,
                [
                    (
                        "search",
                        "Searched · 3 searches",
                        StageStatus::Completed,
                        Some(1_010)
                    ),
                    ("answer", "Writing", StageStatus::Skipped, None),
                ]
            );
            assert_eq!(run.outputs.len(), 1);
            assert_eq!(run.outputs[0].reference, RunOutputRef::JobSearch { id: 9 });

            // Newest first, a page at a time.
            let mut ids = vec![first];
            for at in [2_000, 3_000, 4_000] {
                ids.push(insert(
                    c,
                    new_run(&task, &schedule, &context, ExecutionTrigger::Manual, at),
                )?);
            }
            let page = list(c, task.id, None, 3)?;
            assert!(page.has_more);
            assert_eq!(
                page.runs.iter().map(|r| r.id).collect::<Vec<_>>(),
                [ids[3], ids[2], ids[1]]
            );
            let rest = list(c, task.id, page.runs.last().map(|r| r.id), 3)?;
            assert!(!rest.has_more);
            assert_eq!(rest.runs.iter().map(|r| r.id).collect::<Vec<_>>(), [first]);
            assert_eq!(last_status(c, task.id)?, Some(ExecutionStatus::Queued));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn unfinished_runs_are_marked_interrupted_without_an_output() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let mut task = tasks::tests::sample_task();
            task.id = tasks::insert(c, &task)?;
            let schedule = snapshot();
            let context = RunContext::default();
            let running = insert(
                c,
                new_run(
                    &task,
                    &schedule,
                    &context,
                    ExecutionTrigger::Scheduled,
                    1_000,
                ),
            )?;
            mark_running(c, running, 1_001)?;
            set_stage(
                c,
                running,
                "mail",
                "Reading Gmail",
                StageStatus::Running,
                1_002,
            )?;
            set_stage(
                c,
                running,
                "calendar",
                "Calendar",
                StageStatus::Pending,
                1_002,
            )?;
            let queued = insert(
                c,
                new_run(&task, &schedule, &context, ExecutionTrigger::Manual, 1_100),
            )?;

            assert_eq!(mark_interrupted(c, 5_000)?, 2);
            for id in [running, queued] {
                let run = get(c, id)?;
                assert_eq!(run.status, ExecutionStatus::Failed);
                assert_eq!(run.error.as_deref(), Some(INTERRUPTED));
                assert_eq!(run.error_category, Some(RunErrorCategory::Interrupted));
                assert_eq!(run.result, None);
                assert_eq!(run.finished_at, Some(5_000));
            }
            let stages: Vec<_> = get(c, running)?
                .progress
                .into_iter()
                .map(|s| s.status)
                .collect();
            assert_eq!(stages, [StageStatus::Failed, StageStatus::Skipped]);
            assert_eq!(mark_interrupted(c, 6_000)?, 0);
            Ok(())
        })
        .unwrap();
    }
}
