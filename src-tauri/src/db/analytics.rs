//! Job analytics storage: jobs, their identifying keys, search runs and
//! memberships, requirements, dashboard preferences and learning research.

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::analytics::{
        AnalyticsPreferences, DetailsStatus, EmploymentType, Importance, JobSearchRun,
        RequirementCategory, RequirementKind, RequirementSource, ResearchStatus, RunSource,
        SalaryPeriod, Seniority, WorkMode,
    },
    models::provider::ModelRef,
};

fn to_json(value: &impl serde::Serialize) -> AppResult<String> {
    serde_json::to_string(value).map_err(|e| AppError::internal(e.to_string()))
}

// ── Jobs ──────────────────────────────────────────────────────────────

/// Normalized fields of one job listing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobFields {
    pub title: String,
    pub normalized_title: String,
    pub company: Option<String>,
    pub company_key: Option<String>,
    pub location: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub work_mode: Option<WorkMode>,
    pub employment_type: Option<EmploymentType>,
    pub seniority: Option<Seniority>,
    pub salary_text: Option<String>,
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    pub salary_currency: Option<String>,
    pub salary_period: Option<SalaryPeriod>,
    pub date_posted: Option<i64>,
    /// When the search that reported it ran (0: not a new sighting).
    pub seen_at: i64,
    pub source: Option<String>,
    pub source_url: Option<String>,
    pub external_ref: Option<String>,
    pub description: Option<String>,
    pub details: Option<DetailsStatus>,
}

/// A stored job as the analytics engine reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredJob {
    pub id: i64,
    pub title: String,
    pub normalized_title: String,
    pub company: Option<String>,
    pub company_key: Option<String>,
    pub location: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub work_mode: Option<WorkMode>,
    pub employment_type: Option<EmploymentType>,
    pub seniority: Option<Seniority>,
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    pub salary_currency: Option<String>,
    pub salary_period: Option<SalaryPeriod>,
    pub date_posted: Option<i64>,
    pub date_discovered: i64,
    pub source: Option<String>,
    pub source_url: Option<String>,
    pub details: DetailsStatus,
}

const JOB_COLUMNS: &str = "id, title, normalized_title, company, company_key, location, city, country, \
     work_mode, employment_type, seniority, salary_min, salary_max, salary_currency, salary_period, \
     date_posted, date_discovered, source, source_url, details_status";

fn job_from_row(row: &Row) -> rusqlite::Result<StoredJob> {
    let text = |i: usize| row.get::<_, Option<String>>(i);
    Ok(StoredJob {
        id: row.get(0)?,
        title: row.get(1)?,
        normalized_title: row.get(2)?,
        company: text(3)?,
        company_key: text(4)?,
        location: text(5)?,
        city: text(6)?,
        country: text(7)?,
        work_mode: text(8)?.as_deref().and_then(WorkMode::parse),
        employment_type: text(9)?.as_deref().and_then(EmploymentType::parse),
        seniority: text(10)?.as_deref().and_then(Seniority::parse),
        salary_min: row.get(11)?,
        salary_max: row.get(12)?,
        salary_currency: text(13)?,
        salary_period: text(14)?.as_deref().and_then(SalaryPeriod::parse),
        date_posted: row.get(15)?,
        date_discovered: row.get(16)?,
        source: text(17)?,
        source_url: text(18)?,
        details: DetailsStatus::parse(&row.get::<_, String>(19)?).unwrap_or(DetailsStatus::Done),
    })
}

pub fn load_jobs(conn: &Connection) -> AppResult<Vec<StoredJob>> {
    let mut stmt = conn.prepare(&format!("SELECT {JOB_COLUMNS} FROM jobs ORDER BY id"))?;
    let jobs = stmt
        .query_map([], job_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(jobs)
}

pub fn get_job(conn: &Connection, id: i64) -> AppResult<StoredJob> {
    conn.query_row(
        &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?1"),
        [id],
        job_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("This job is no longer stored."))
}

/// The job one of `keys` identifies; earlier keys are stronger evidence.
pub fn find_by_keys(conn: &Connection, keys: &[String]) -> AppResult<Option<i64>> {
    let mut stmt = conn.prepare_cached("SELECT job_id FROM job_keys WHERE key = ?1")?;
    for key in keys {
        if let Some(id) = stmt.query_row([key], |r| r.get(0)).optional()? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Owner of a key, if any.
pub fn key_owner(conn: &Connection, key: &str) -> AppResult<Option<i64>> {
    Ok(conn
        .query_row("SELECT job_id FROM job_keys WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

/// Adds keys to a job; keys that already identify a job are kept as they are.
pub fn add_keys(conn: &Connection, job_id: i64, keys: &[String]) -> AppResult<()> {
    let mut stmt =
        conn.prepare_cached("INSERT OR IGNORE INTO job_keys (key, job_id) VALUES (?1, ?2)")?;
    for key in keys {
        stmt.execute(params![key, job_id])?;
    }
    Ok(())
}

pub fn insert_job(conn: &Connection, f: &JobFields, now: i64) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO jobs (title, normalized_title, company, company_key, location, city, country,
             work_mode, employment_type, seniority, salary_text, salary_min, salary_max,
             salary_currency, salary_period, date_posted, date_discovered, last_seen, source,
             source_url, external_ref, description, details_status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17,
                 ?18, ?19, ?20, ?21, ?22, ?23, ?23)",
        params![
            f.title,
            f.normalized_title,
            f.company,
            f.company_key,
            f.location,
            f.city,
            f.country,
            f.work_mode.map(WorkMode::as_str),
            f.employment_type.map(EmploymentType::as_str),
            f.seniority.map(Seniority::as_str),
            f.salary_text,
            f.salary_min,
            f.salary_max,
            f.salary_currency,
            f.salary_period.map(SalaryPeriod::as_str),
            f.date_posted,
            f.seen_at,
            f.source,
            f.source_url,
            f.external_ref,
            f.description,
            f.details.unwrap_or(DetailsStatus::Skipped).as_str(),
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Adds what a new sighting of a known job states. Known values are never
/// overwritten; the salary is only taken as a whole.
pub fn fill_job(conn: &Connection, id: i64, f: &JobFields, now: i64) -> AppResult<()> {
    let has_salary: bool = conn.query_row(
        "SELECT salary_min IS NOT NULL OR salary_max IS NOT NULL FROM jobs WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?;
    conn.execute(
        "UPDATE jobs SET
             company = COALESCE(company, ?2), company_key = COALESCE(company_key, ?3),
             location = COALESCE(location, ?4), city = COALESCE(city, ?5),
             country = COALESCE(country, ?6), work_mode = COALESCE(work_mode, ?7),
             employment_type = COALESCE(employment_type, ?8), seniority = COALESCE(seniority, ?9),
             date_posted = COALESCE(date_posted, ?10),
             date_discovered = CASE WHEN ?11 > 0 THEN MIN(date_discovered, ?11) ELSE date_discovered END,
             last_seen = CASE WHEN ?11 > 0 THEN MAX(last_seen, ?11) ELSE last_seen END,
             source = COALESCE(source, ?12), external_ref = COALESCE(external_ref, ?13),
             description = COALESCE(description, ?14),
             details_status = CASE
                 WHEN source_url IS NULL AND ?15 IS NOT NULL THEN 'pending'
                 WHEN details_status = 'skipped' AND ?17 = 'pending' THEN 'pending'
                 ELSE details_status END,
             source_url = COALESCE(source_url, ?15),
             updated_at = ?16
         WHERE id = ?1",
        params![
            id,
            f.company,
            f.company_key,
            f.location,
            f.city,
            f.country,
            f.work_mode.map(WorkMode::as_str),
            f.employment_type.map(EmploymentType::as_str),
            f.seniority.map(Seniority::as_str),
            f.date_posted,
            f.seen_at,
            f.source,
            f.external_ref,
            f.description,
            f.source_url,
            now,
            f.details.map(DetailsStatus::as_str),
        ],
    )?;
    if !has_salary && (f.salary_min.is_some() || f.salary_max.is_some()) {
        set_salary(conn, id, f)?;
    }
    Ok(())
}

fn set_salary(conn: &Connection, id: i64, f: &JobFields) -> AppResult<()> {
    conn.execute(
        "UPDATE jobs SET salary_text = ?2, salary_min = ?3, salary_max = ?4, salary_currency = ?5,
             salary_period = ?6 WHERE id = ?1",
        params![
            id,
            f.salary_text,
            f.salary_min,
            f.salary_max,
            f.salary_currency,
            f.salary_period.map(SalaryPeriod::as_str)
        ],
    )?;
    Ok(())
}

/// Moves everything of `duplicate` onto `keep` and deletes the duplicate
/// (used when later evidence shows two records are the same job).
pub fn merge_jobs(conn: &Connection, keep: i64, duplicate: i64, now: i64) -> AppResult<()> {
    if keep == duplicate {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO job_search_run_jobs (run_id, job_id, position, raw)
         SELECT run_id, ?1, position, raw FROM job_search_run_jobs WHERE job_id = ?2",
        params![keep, duplicate],
    )?;
    conn.execute(
        "UPDATE OR IGNORE job_keys SET job_id = ?1 WHERE job_id = ?2",
        params![keep, duplicate],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO job_requirements (job_id, kind, category, name, original, importance, years, level, source)
         SELECT ?1, kind, category, name, original, importance, years, level, source
         FROM job_requirements WHERE job_id = ?2",
        params![keep, duplicate],
    )?;
    let other = get_job(conn, duplicate)?;
    let description: Option<String> = conn.query_row(
        "SELECT description FROM jobs WHERE id = ?1",
        [duplicate],
        |r| r.get(0),
    )?;
    fill_job(
        conn,
        keep,
        &JobFields {
            company: other.company,
            company_key: other.company_key,
            location: other.location,
            city: other.city,
            country: other.country,
            work_mode: other.work_mode,
            employment_type: other.employment_type,
            seniority: other.seniority,
            salary_min: other.salary_min,
            salary_max: other.salary_max,
            salary_currency: other.salary_currency,
            salary_period: other.salary_period,
            date_posted: other.date_posted,
            seen_at: other.date_discovered,
            source: other.source,
            source_url: other.source_url,
            description,
            ..JobFields::default()
        },
        now,
    )?;
    conn.execute("DELETE FROM jobs WHERE id = ?1", [duplicate])?;
    Ok(())
}

/// Deletes jobs no search run refers to any more.
pub fn delete_orphans(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute(
        "DELETE FROM jobs WHERE id NOT IN (SELECT job_id FROM job_search_run_jobs)",
        [],
    )?)
}

// ── Requirements ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct RequirementFields<'a> {
    pub kind: RequirementKind,
    pub category: RequirementCategory,
    pub name: &'a str,
    pub original: &'a str,
    pub importance: Importance,
    pub years: Option<u32>,
    pub level: Option<&'a str>,
    pub source: RequirementSource,
}

/// Adds requirements; one stored per (job, kind, name). Returns how many
/// were new.
pub fn add_requirements(
    conn: &Connection,
    job_id: i64,
    reqs: &[RequirementFields],
) -> AppResult<usize> {
    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO job_requirements
             (job_id, kind, category, name, original, importance, years, level, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    let mut added = 0;
    for r in reqs {
        added += stmt.execute(params![
            job_id,
            r.kind.as_str(),
            r.category.as_str(),
            r.name,
            r.original,
            r.importance.as_str(),
            r.years,
            r.level,
            r.source.as_str(),
        ])?;
    }
    Ok(added)
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredRequirement {
    pub job_id: i64,
    pub kind: RequirementKind,
    pub category: RequirementCategory,
    pub name: String,
    pub original: String,
    pub importance: Importance,
    pub years: Option<u32>,
    pub level: Option<String>,
}

pub fn load_requirements(conn: &Connection) -> AppResult<Vec<StoredRequirement>> {
    let mut stmt = conn.prepare(
        "SELECT job_id, kind, category, name, original, importance, years, level
         FROM job_requirements ORDER BY job_id, id",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(StoredRequirement {
                job_id: row.get(0)?,
                kind: RequirementKind::parse(&row.get::<_, String>(1)?)
                    .unwrap_or(RequirementKind::Other),
                category: RequirementCategory::parse(&row.get::<_, String>(2)?)
                    .unwrap_or(RequirementCategory::Other),
                name: row.get(3)?,
                original: row.get(4)?,
                importance: Importance::parse(&row.get::<_, String>(5)?)
                    .unwrap_or(Importance::Required),
                years: row.get(6)?,
                level: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ── Search runs ───────────────────────────────────────────────────────

pub struct RunFields<'a> {
    pub origin: &'a str,
    pub title: &'a str,
    pub query: &'a str,
    pub source: RunSource,
    pub task_id: Option<i64>,
    pub conversation_id: Option<i64>,
    pub message_id: Option<i64>,
    pub execution_id: Option<i64>,
    pub created_at: i64,
}

pub fn run_by_origin(conn: &Connection, origin: &str) -> AppResult<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM job_search_runs WHERE origin = ?1",
            [origin],
            |r| r.get(0),
        )
        .optional()?)
}

/// Creates the run, or clears the results of the run with the same origin
/// (a regenerated answer replaces its own earlier results).
pub fn upsert_run(conn: &Connection, run: &RunFields) -> AppResult<i64> {
    if let Some(id) = run_by_origin(conn, run.origin)? {
        conn.execute("DELETE FROM job_search_run_jobs WHERE run_id = ?1", [id])?;
        conn.execute(
            "UPDATE job_search_runs SET title = ?2, query = ?3, source = ?4 WHERE id = ?1",
            params![id, run.title, run.query, run.source.as_str()],
        )?;
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO job_search_runs (origin, title, query, source, task_id, conversation_id,
             message_id, execution_id, result_count, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
        params![
            run.origin,
            run.title,
            run.query,
            run.source.as_str(),
            run.task_id,
            run.conversation_id,
            run.message_id,
            run.execution_id,
            run.created_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Records that a run found a job. Returns false if the run already listed
/// it (a duplicate row within one result).
pub fn add_member(
    conn: &Connection,
    run_id: i64,
    job_id: i64,
    position: usize,
    raw: &str,
) -> AppResult<bool> {
    Ok(conn.execute(
        "INSERT OR IGNORE INTO job_search_run_jobs (run_id, job_id, position, raw) VALUES (?1, ?2, ?3, ?4)",
        params![run_id, job_id, position as i64, raw],
    )? == 1)
}

/// Stores the run's distinct job count and returns it.
pub fn finish_run(conn: &Connection, run_id: i64) -> AppResult<u32> {
    conn.execute(
        "UPDATE job_search_runs SET result_count =
             (SELECT COUNT(*) FROM job_search_run_jobs WHERE run_id = ?1) WHERE id = ?1",
        [run_id],
    )?;
    Ok(conn.query_row(
        "SELECT result_count FROM job_search_runs WHERE id = ?1",
        [run_id],
        |r| r.get(0),
    )?)
}

pub fn delete_run(conn: &Connection, id: i64) -> AppResult<()> {
    let deleted = conn.execute("DELETE FROM job_search_runs WHERE id = ?1", [id])?;
    if deleted == 0 {
        return Err(AppError::not_found("This search is no longer stored."));
    }
    delete_orphans(conn)?;
    Ok(())
}

pub fn list_runs(conn: &Connection) -> AppResult<Vec<JobSearchRun>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, query, source, task_id, conversation_id, created_at, result_count
         FROM job_search_runs ORDER BY created_at DESC, id DESC",
    )?;
    let runs = stmt
        .query_map([], |row| {
            Ok(JobSearchRun {
                id: row.get(0)?,
                title: row.get(1)?,
                query: row.get(2)?,
                source: RunSource::parse(&row.get::<_, String>(3)?).unwrap_or(RunSource::Tool),
                task_id: row.get(4)?,
                conversation_id: row.get(5)?,
                created_at: row.get(6)?,
                result_count: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(runs)
}

/// (run, job) pairs.
pub fn load_memberships(conn: &Connection) -> AppResult<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare("SELECT run_id, job_id FROM job_search_run_jobs")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Runs created from messages of a conversation: (message id, run id, jobs).
pub fn runs_for_conversation(
    conn: &Connection,
    conversation_id: i64,
) -> AppResult<Vec<(i64, i64, u32)>> {
    let mut stmt = conn.prepare(
        "SELECT message_id, id, result_count FROM job_search_runs
         WHERE conversation_id = ?1 AND message_id IS NOT NULL",
    )?;
    let rows = stmt
        .query_map([conversation_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Runs created from executions of a task: (execution id, run id, jobs).
pub fn runs_for_task(conn: &Connection, task_id: i64) -> AppResult<Vec<(i64, i64, u32)>> {
    let mut stmt = conn.prepare(
        "SELECT execution_id, id, result_count FROM job_search_runs
         WHERE task_id = ?1 AND execution_id IS NOT NULL",
    )?;
    let rows = stmt
        .query_map([task_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ── Background details ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct PendingJob {
    pub id: i64,
    pub title: String,
    pub company_key: Option<String>,
    pub source_url: Option<String>,
    pub description: Option<String>,
    pub attempts: u32,
}

/// The next job whose details are still to be read (oldest first).
pub fn next_pending(conn: &Connection) -> AppResult<Option<PendingJob>> {
    Ok(conn
        .query_row(
            "SELECT id, title, company_key, source_url, description, details_attempts FROM jobs
             WHERE details_status = 'pending' ORDER BY details_attempts, id LIMIT 1",
            [],
            |r| {
                Ok(PendingJob {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    company_key: r.get(2)?,
                    source_url: r.get(3)?,
                    description: r.get(4)?,
                    attempts: r.get(5)?,
                })
            },
        )
        .optional()?)
}

pub fn count_pending(conn: &Connection) -> AppResult<u32> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM jobs WHERE details_status = 'pending'",
        [],
        |r| r.get(0),
    )?)
}

pub fn counts(conn: &Connection) -> AppResult<(u32, u32)> {
    Ok((
        conn.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))?,
        conn.query_row("SELECT COUNT(*) FROM job_search_runs", [], |r| r.get(0))?,
    ))
}

/// What reading a job's page and description found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DetailsUpdate {
    pub description: Option<String>,
    pub fields: JobFields,
    pub benefits: Vec<String>,
    pub status: Option<DetailsStatus>,
    pub note: Option<String>,
}

pub fn save_details(conn: &Connection, id: i64, update: &DetailsUpdate, now: i64) -> AppResult<()> {
    fill_job(conn, id, &update.fields, now)?;
    if let Some(description) = &update.description {
        // A full page description replaces a short search summary.
        conn.execute(
            "UPDATE jobs SET description = ?2
             WHERE id = ?1 AND (description IS NULL OR LENGTH(description) < LENGTH(?2))",
            params![id, description],
        )?;
    }
    if !update.benefits.is_empty() {
        conn.execute(
            "UPDATE jobs SET benefits = ?2 WHERE id = ?1",
            params![id, to_json(&update.benefits)?],
        )?;
    }
    conn.execute(
        "UPDATE jobs SET details_status = COALESCE(?2, details_status), details_note = ?3,
             details_attempts = details_attempts + 1, updated_at = ?4 WHERE id = ?1",
        params![
            id,
            update.status.map(DetailsStatus::as_str),
            update.note,
            now
        ],
    )?;
    Ok(())
}

/// Marks every pending job as done (details reading turned off).
pub fn settle_pending(conn: &Connection, status: DetailsStatus, note: &str) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE jobs SET details_status = ?1, details_note = ?2 WHERE details_status = 'pending'",
        params![status.as_str(), note],
    )?)
}

/// Makes jobs with a link wait for details again (details reading turned on).
pub fn requeue_unread(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE jobs SET details_status = 'pending', details_attempts = 0
         WHERE details_status = 'skipped' AND (source_url IS NOT NULL OR description IS NOT NULL)
           AND details_note IS NOT NULL",
        [],
    )?)
}

// ── Preferences ───────────────────────────────────────────────────────

pub fn get_preferences(conn: &Connection) -> AppResult<Option<AnalyticsPreferences>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT state FROM analytics_preferences WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    // An unreadable state (older format) falls back to the defaults.
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

pub fn save_preferences(
    conn: &Connection,
    prefs: &AnalyticsPreferences,
    now: i64,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO analytics_preferences (id, state, updated_at) VALUES (1, ?1, ?2)
         ON CONFLICT (id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at",
        params![to_json(prefs)?, now],
    )?;
    Ok(())
}

// ── Learning research ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct StoredResearch {
    pub id: i64,
    pub dataset_key: String,
    pub dataset_label: String,
    pub job_count: u32,
    pub gap_snapshot: String,
    pub recommendations: String,
    pub status: ResearchStatus,
    pub error: Option<String>,
    pub model_id: Option<String>,
    pub generated_at: i64,
}

const RESEARCH_COLUMNS: &str = "id, dataset_key, dataset_label, job_count, gap_snapshot, \
     recommendations, status, error, model_id, generated_at";

fn research_from_row(row: &Row) -> rusqlite::Result<StoredResearch> {
    Ok(StoredResearch {
        id: row.get(0)?,
        dataset_key: row.get(1)?,
        dataset_label: row.get(2)?,
        job_count: row.get(3)?,
        gap_snapshot: row.get(4)?,
        recommendations: row.get(5)?,
        status: ResearchStatus::parse(&row.get::<_, String>(6)?).unwrap_or(ResearchStatus::Failed),
        error: row.get(7)?,
        model_id: row.get(8)?,
        generated_at: row.get(9)?,
    })
}

pub struct NewResearch<'a> {
    pub dataset_key: &'a str,
    pub dataset_label: &'a str,
    pub dataset: &'a str,
    pub job_count: u32,
    pub gap_snapshot: &'a str,
    pub provider_id: &'a str,
    pub model_id: &'a str,
    pub generated_at: i64,
}

pub fn insert_research(conn: &Connection, r: &NewResearch) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO learning_recommendations (dataset_key, dataset_label, dataset, job_count,
             gap_snapshot, status, provider_id, model_id, generated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?7, ?8)",
        params![
            r.dataset_key,
            r.dataset_label,
            r.dataset,
            r.job_count,
            r.gap_snapshot,
            r.provider_id,
            r.model_id,
            r.generated_at
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn finish_research(
    conn: &Connection,
    id: i64,
    status: ResearchStatus,
    recommendations: Option<&str>,
    error: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE learning_recommendations SET status = ?2,
             recommendations = COALESCE(?3, recommendations), error = ?4 WHERE id = ?1",
        params![id, status.as_str(), recommendations, error],
    )?;
    Ok(())
}

/// The latest research for a selection, or else the latest of all.
pub fn latest_research(conn: &Connection, dataset_key: &str) -> AppResult<Option<StoredResearch>> {
    let own = conn
        .query_row(
            &format!(
                "SELECT {RESEARCH_COLUMNS} FROM learning_recommendations WHERE dataset_key = ?1
                 ORDER BY generated_at DESC, id DESC LIMIT 1"
            ),
            [dataset_key],
            research_from_row,
        )
        .optional()?;
    if own.is_some() {
        return Ok(own);
    }
    Ok(conn
        .query_row(
            &format!(
                "SELECT {RESEARCH_COLUMNS} FROM learning_recommendations WHERE status = 'done'
                 ORDER BY generated_at DESC, id DESC LIMIT 1"
            ),
            [],
            research_from_row,
        )
        .optional()?)
}

pub fn research_running(conn: &Connection) -> AppResult<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM learning_recommendations WHERE status = 'running')",
        [],
        |r| r.get(0),
    )?)
}

/// Research that was running when ReMa closed never finishes.
pub fn mark_interrupted_research(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "UPDATE learning_recommendations SET status = 'failed',
             error = 'ReMa was closed before the research finished.' WHERE status = 'running'",
        [],
    )?;
    Ok(())
}

// ── History for ingestion ─────────────────────────────────────────────

/// A completed assistant answer that may list jobs.
pub struct AnswerRow {
    pub message_id: i64,
    pub conversation_id: i64,
    pub content: String,
    pub created_at: i64,
    /// The user message it answered.
    pub prompt: Option<String>,
    /// The model that wrote it.
    pub model: Option<ModelRef>,
}

fn model_ref(provider_id: Option<String>, model_id: Option<String>) -> Option<ModelRef> {
    provider_id
        .zip(model_id)
        .map(|(provider_id, model_id)| ModelRef {
            provider_id,
            model_id,
        })
}

fn answer_from_row(r: &Row) -> rusqlite::Result<AnswerRow> {
    Ok(AnswerRow {
        message_id: r.get(0)?,
        conversation_id: r.get(1)?,
        content: r.get(2)?,
        created_at: r.get(3)?,
        prompt: r.get(4)?,
        model: model_ref(r.get(5)?, r.get(6)?),
    })
}

const ANSWER_SELECT: &str = "SELECT m.id, m.conversation_id, m.content, m.created_at,
     (SELECT u.content FROM messages u WHERE u.conversation_id = m.conversation_id
        AND u.id < m.id AND u.role = 'user' ORDER BY u.id DESC LIMIT 1),
     m.provider_id, m.model_id
     FROM messages m";

pub fn answer(conn: &Connection, message_id: i64) -> AppResult<AnswerRow> {
    conn.query_row(
        &format!("{ANSWER_SELECT} WHERE m.id = ?1 AND m.role = 'assistant'"),
        [message_id],
        answer_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("This answer no longer exists."))
}

/// Completed answers containing a table, after `after_id`, in order.
pub fn answers_with_tables(
    conn: &Connection,
    after_id: i64,
    limit: usize,
) -> AppResult<Vec<AnswerRow>> {
    let mut stmt = conn.prepare(&format!(
        "{ANSWER_SELECT} WHERE m.role = 'assistant' AND m.status = 'complete' AND m.id > ?1
             AND m.content LIKE '%|%' ORDER BY m.id LIMIT ?2"
    ))?;
    let rows = stmt
        .query_map(params![after_id, limit as i64], answer_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// A successful run of a prompt task.
pub struct TaskResultRow {
    pub execution_id: i64,
    pub task_id: i64,
    pub task_name: String,
    pub prompt: String,
    pub result: String,
    pub started_at: i64,
    /// The model that ran it.
    pub model: Option<ModelRef>,
}

const RESULT_SELECT: &str =
    "SELECT e.id, e.task_id, t.name, e.prompt, e.result, e.started_at, e.provider_id, e.model_id
     FROM task_executions e JOIN scheduled_tasks t ON t.id = e.task_id";

fn result_from_row(r: &Row) -> rusqlite::Result<TaskResultRow> {
    Ok(TaskResultRow {
        execution_id: r.get(0)?,
        task_id: r.get(1)?,
        task_name: r.get(2)?,
        prompt: r.get(3)?,
        result: r.get(4)?,
        started_at: r.get(5)?,
        model: model_ref(r.get(6)?, r.get(7)?),
    })
}

pub fn task_result(conn: &Connection, execution_id: i64) -> AppResult<TaskResultRow> {
    conn.query_row(
        &format!(
            "{RESULT_SELECT} WHERE e.id = ?1 AND e.status = 'succeeded' AND e.result IS NOT NULL"
        ),
        [execution_id],
        result_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("This run has no result."))
}

/// Successful prompt-task results containing a table, after `after_id`.
pub fn task_results_with_tables(
    conn: &Connection,
    after_id: i64,
    limit: usize,
) -> AppResult<Vec<TaskResultRow>> {
    let mut stmt = conn.prepare(&format!(
        "{RESULT_SELECT} WHERE e.status = 'succeeded' AND e.result LIKE '%|%' AND e.id > ?1
             AND t.kind LIKE '%\"prompt\"%' ORDER BY e.id LIMIT ?2"
    ))?;
    let rows = stmt
        .query_map(params![after_id, limit as i64], result_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Ids of jobs among `ids` that still exist.
pub fn existing_jobs(conn: &Connection, ids: &[i64]) -> AppResult<Vec<i64>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; ids.len()].join(",");
    let mut stmt = conn.prepare(&format!("SELECT id FROM jobs WHERE id IN ({marks})"))?;
    let rows = stmt
        .query_map(params_from_iter(ids), |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
