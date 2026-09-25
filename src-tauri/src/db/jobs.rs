//! Job applications, processed Gmail messages and interviews.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::{jobs::ApplicationStatus, text_enum},
};

// ── Mail ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailStatus {
    /// Not about a job application.
    Irrelevant,
    /// Relevant, waiting for extraction (per-run limit reached).
    Pending,
    Processed,
    Failed,
}

text_enum!(MailStatus {
    Irrelevant => "irrelevant",
    Pending => "pending",
    Processed => "processed",
    Failed => "failed",
});

/// How often a failing message is retried in later runs.
pub const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailRecord {
    pub message_id: String,
    pub thread_id: String,
    pub received_at: i64,
    pub sender_domain: Option<String>,
    pub status: MailStatus,
    pub attempts: u32,
    pub classification: Option<ApplicationStatus>,
    pub extraction: Option<String>,
    pub application_id: Option<i64>,
    pub error: Option<String>,
    pub first_seen_at: i64,
    pub processed_at: Option<i64>,
}

const MAIL_COLUMNS: &str = "message_id, thread_id, received_at, sender_domain, status, attempts,
    classification, extraction, application_id, error, first_seen_at, processed_at";

fn mail_from_row(row: &Row) -> rusqlite::Result<MailRecord> {
    let status: String = row.get(4)?;
    let classification: Option<String> = row.get(6)?;
    Ok(MailRecord {
        message_id: row.get(0)?,
        thread_id: row.get(1)?,
        received_at: row.get(2)?,
        sender_domain: row.get(3)?,
        status: MailStatus::parse(&status).unwrap_or(MailStatus::Failed),
        attempts: row.get(5)?,
        classification: classification.as_deref().and_then(ApplicationStatus::parse),
        extraction: row.get(7)?,
        application_id: row.get(8)?,
        error: row.get(9)?,
        first_seen_at: row.get(10)?,
        processed_at: row.get(11)?,
    })
}

pub fn get_mail(conn: &Connection, message_id: &str) -> AppResult<Option<MailRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {MAIL_COLUMNS} FROM mail_messages WHERE message_id = ?1"),
            [message_id],
            mail_from_row,
        )
        .optional()?)
}

/// Of `ids`, the messages recorded by an earlier run (whatever their status;
/// pending and retryable ones are picked up by [`pending_mail`]).
pub fn known_messages(conn: &Connection, ids: &[String]) -> AppResult<HashSet<String>> {
    let mut known = HashSet::new();
    let mut stmt = conn.prepare("SELECT 1 FROM mail_messages WHERE message_id = ?1")?;
    for id in ids {
        if stmt.exists([id])? {
            known.insert(id.clone());
        }
    }
    Ok(known)
}

pub fn upsert_mail(conn: &Connection, mail: &MailRecord) -> AppResult<()> {
    conn.execute(
        "INSERT INTO mail_messages (message_id, thread_id, received_at, sender_domain, status,
             attempts, classification, extraction, application_id, error, first_seen_at, processed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT (message_id) DO UPDATE SET
             status = excluded.status, attempts = excluded.attempts,
             classification = excluded.classification, extraction = excluded.extraction,
             application_id = excluded.application_id, error = excluded.error,
             processed_at = excluded.processed_at",
        params![
            mail.message_id,
            mail.thread_id,
            mail.received_at,
            mail.sender_domain,
            mail.status.as_str(),
            mail.attempts,
            mail.classification.map(ApplicationStatus::as_str),
            mail.extraction,
            mail.application_id,
            mail.error,
            mail.first_seen_at,
            mail.processed_at,
        ],
    )?;
    Ok(())
}

/// Relevant messages still waiting for extraction, oldest first.
pub fn pending_mail(conn: &Connection) -> AppResult<Vec<MailRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MAIL_COLUMNS} FROM mail_messages
         WHERE status = 'pending' OR (status = 'failed' AND attempts < ?1)
         ORDER BY received_at, message_id"
    ))?;
    let rows = stmt.query_map([MAX_ATTEMPTS], mail_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

// ── Applications ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationRecord {
    pub id: i64,
    pub company: String,
    pub company_key: String,
    pub role: Option<String>,
    pub role_key: Option<String>,
    pub reference: Option<String>,
    pub sender_domain: Option<String>,
    pub status: ApplicationStatus,
    pub requires_action: bool,
    pub next_action: Option<String>,
    pub last_update_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

const APP_COLUMNS: &str = "id, company, company_key, role, role_key, reference, sender_domain,
    status, requires_action, next_action, last_update_at, created_at, updated_at";

fn app_from_row(row: &Row) -> rusqlite::Result<ApplicationRecord> {
    let status: String = row.get(7)?;
    Ok(ApplicationRecord {
        id: row.get(0)?,
        company: row.get(1)?,
        company_key: row.get(2)?,
        role: row.get(3)?,
        role_key: row.get(4)?,
        reference: row.get(5)?,
        sender_domain: row.get(6)?,
        status: ApplicationStatus::parse(&status).unwrap_or(ApplicationStatus::InProcess),
        requires_action: row.get(8)?,
        next_action: row.get(9)?,
        last_update_at: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn query_apps(
    conn: &Connection,
    sql_where: &str,
    args: impl rusqlite::Params,
) -> AppResult<Vec<ApplicationRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {APP_COLUMNS} FROM job_applications {sql_where}"
    ))?;
    let rows = stmt.query_map(args, app_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get_application(conn: &Connection, id: i64) -> AppResult<ApplicationRecord> {
    query_apps(conn, "WHERE id = ?1", [id])?
        .pop()
        .ok_or_else(|| AppError::not_found("Application not found"))
}

pub fn application_for_thread(conn: &Connection, thread_id: &str) -> AppResult<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT application_id FROM application_threads WHERE thread_id = ?1",
            [thread_id],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn applications_by_reference(
    conn: &Connection,
    reference: &str,
) -> AppResult<Vec<ApplicationRecord>> {
    query_apps(
        conn,
        "WHERE reference = ?1 ORDER BY last_update_at DESC",
        [reference],
    )
}

pub fn applications_by_company(
    conn: &Connection,
    company_key: &str,
) -> AppResult<Vec<ApplicationRecord>> {
    query_apps(
        conn,
        "WHERE company_key = ?1 ORDER BY last_update_at DESC",
        [company_key],
    )
}

pub fn applications_by_domain(
    conn: &Connection,
    domain: &str,
) -> AppResult<Vec<ApplicationRecord>> {
    query_apps(
        conn,
        "WHERE sender_domain = ?1 ORDER BY last_update_at DESC",
        [domain],
    )
}

/// Updated since `since`, or still needing attention; newest first.
pub fn overview_applications(conn: &Connection, since: i64) -> AppResult<Vec<ApplicationRecord>> {
    query_apps(
        conn,
        "WHERE last_update_at >= ?1 OR status IN ('needs_action', 'upcoming_interview')
         ORDER BY last_update_at DESC, id DESC",
        [since],
    )
}

pub fn applications_with_status(
    conn: &Connection,
    status: ApplicationStatus,
) -> AppResult<Vec<ApplicationRecord>> {
    query_apps(conn, "WHERE status = ?1", [status.as_str()])
}

pub fn insert_application(conn: &Connection, app: &ApplicationRecord) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO job_applications (company, company_key, role, role_key, reference,
             sender_domain, status, requires_action, next_action, last_update_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            app.company,
            app.company_key,
            app.role,
            app.role_key,
            app.reference,
            app.sender_domain,
            app.status.as_str(),
            app.requires_action,
            app.next_action,
            app.last_update_at,
            app.created_at,
            app.updated_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn save_application(conn: &Connection, app: &ApplicationRecord) -> AppResult<()> {
    conn.execute(
        "UPDATE job_applications SET company = ?2, company_key = ?3, role = ?4, role_key = ?5,
             reference = ?6, sender_domain = ?7, status = ?8, requires_action = ?9,
             next_action = ?10, last_update_at = ?11, updated_at = ?12
         WHERE id = ?1",
        params![
            app.id,
            app.company,
            app.company_key,
            app.role,
            app.role_key,
            app.reference,
            app.sender_domain,
            app.status.as_str(),
            app.requires_action,
            app.next_action,
            app.last_update_at,
            app.updated_at,
        ],
    )?;
    Ok(())
}

/// Links a Gmail thread to an application (first link wins).
pub fn link_thread(conn: &Connection, thread_id: &str, application_id: i64) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO application_threads (thread_id, application_id) VALUES (?1, ?2)",
        params![thread_id, application_id],
    )?;
    Ok(())
}

pub fn record_update(
    conn: &Connection,
    application_id: i64,
    message_id: &str,
    status: ApplicationStatus,
    summary: Option<&str>,
    occurred_at: i64,
    now: i64,
) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO application_updates
             (application_id, message_id, status, summary, occurred_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            application_id,
            message_id,
            status.as_str(),
            summary,
            occurred_at,
            now
        ],
    )?;
    Ok(())
}

// ── Interviews ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterviewState {
    Proposed,
    Confirmed,
    NeedsReview,
    Cancelled,
}

text_enum!(InterviewState {
    Proposed => "proposed",
    Confirmed => "confirmed",
    NeedsReview => "needs_review",
    Cancelled => "cancelled",
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterviewRecord {
    pub id: i64,
    pub application_id: i64,
    pub source_thread_id: String,
    pub source_message_id: String,
    pub state: InterviewState,
    pub interview_type: Option<String>,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub timezone: Option<String>,
    pub location: Option<String>,
    pub meeting_url: Option<String>,
    pub participants: Vec<String>,
    pub review_reason: Option<String>,
    pub calendar_event_id: Option<String>,
    pub calendar_hash: Option<String>,
    pub calendar_synced_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

const INTERVIEW_COLUMNS: &str = "id, application_id, source_thread_id, source_message_id, state,
    interview_type, start_at, end_at, timezone, location, meeting_url, participants, review_reason,
    calendar_event_id, calendar_hash, calendar_synced_at, created_at, updated_at";

fn interview_from_row(row: &Row) -> rusqlite::Result<InterviewRecord> {
    let state: String = row.get(4)?;
    let participants: Option<String> = row.get(11)?;
    Ok(InterviewRecord {
        id: row.get(0)?,
        application_id: row.get(1)?,
        source_thread_id: row.get(2)?,
        source_message_id: row.get(3)?,
        state: InterviewState::parse(&state).unwrap_or(InterviewState::NeedsReview),
        interview_type: row.get(5)?,
        start_at: row.get(6)?,
        end_at: row.get(7)?,
        timezone: row.get(8)?,
        location: row.get(9)?,
        meeting_url: row.get(10)?,
        participants: participants
            .and_then(|p| serde_json::from_str(&p).ok())
            .unwrap_or_default(),
        review_reason: row.get(12)?,
        calendar_event_id: row.get(13)?,
        calendar_hash: row.get(14)?,
        calendar_synced_at: row.get(15)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}

fn query_interviews(
    conn: &Connection,
    sql_where: &str,
    args: impl rusqlite::Params,
) -> AppResult<Vec<InterviewRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {INTERVIEW_COLUMNS} FROM interviews {sql_where}"
    ))?;
    let rows = stmt.query_map(args, interview_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn interviews_for_application(
    conn: &Connection,
    application_id: i64,
) -> AppResult<Vec<InterviewRecord>> {
    query_interviews(
        conn,
        "WHERE application_id = ?1 ORDER BY id",
        [application_id],
    )
}

pub fn interview_from_message(
    conn: &Connection,
    message_id: &str,
) -> AppResult<Option<InterviewRecord>> {
    Ok(query_interviews(conn, "WHERE source_message_id = ?1", [message_id])?.pop())
}

/// Confirmed interviews that have not ended, plus cancelled ones still
/// linked to a Calendar event that ReMa may need to mark.
pub fn interviews_to_sync(conn: &Connection, now: i64) -> AppResult<Vec<InterviewRecord>> {
    query_interviews(
        conn,
        "WHERE (state = 'confirmed' AND end_at > ?1)
            OR (state = 'cancelled' AND calendar_event_id IS NOT NULL AND end_at > ?1)
         ORDER BY start_at, id",
        [now],
    )
}

pub fn interviews_needing_review(conn: &Connection, since: i64) -> AppResult<Vec<InterviewRecord>> {
    query_interviews(
        conn,
        "WHERE state = 'needs_review' AND updated_at >= ?1 ORDER BY id",
        [since],
    )
}

fn participants_json(participants: &[String]) -> Option<String> {
    (!participants.is_empty()).then(|| serde_json::to_string(participants).unwrap_or_default())
}

pub fn insert_interview(conn: &Connection, i: &InterviewRecord) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO interviews (application_id, source_thread_id, source_message_id, state,
             interview_type, start_at, end_at, timezone, location, meeting_url, participants,
             review_reason, calendar_event_id, calendar_hash, calendar_synced_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            i.application_id,
            i.source_thread_id,
            i.source_message_id,
            i.state.as_str(),
            i.interview_type,
            i.start_at,
            i.end_at,
            i.timezone,
            i.location,
            i.meeting_url,
            participants_json(&i.participants),
            i.review_reason,
            i.calendar_event_id,
            i.calendar_hash,
            i.calendar_synced_at,
            i.created_at,
            i.updated_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn save_interview(conn: &Connection, i: &InterviewRecord) -> AppResult<()> {
    conn.execute(
        "UPDATE interviews SET source_thread_id = ?2, source_message_id = ?3, state = ?4,
             interview_type = ?5, start_at = ?6, end_at = ?7, timezone = ?8, location = ?9,
             meeting_url = ?10, participants = ?11, review_reason = ?12, calendar_event_id = ?13,
             calendar_hash = ?14, calendar_synced_at = ?15, updated_at = ?16
         WHERE id = ?1",
        params![
            i.id,
            i.source_thread_id,
            i.source_message_id,
            i.state.as_str(),
            i.interview_type,
            i.start_at,
            i.end_at,
            i.timezone,
            i.location,
            i.meeting_url,
            participants_json(&i.participants),
            i.review_reason,
            i.calendar_event_id,
            i.calendar_hash,
            i.calendar_synced_at,
            i.updated_at,
        ],
    )?;
    Ok(())
}

pub fn add_interview_history(
    conn: &Connection,
    interview_id: i64,
    change: &str,
    details: Option<&str>,
    message_id: Option<&str>,
    now: i64,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO interview_history (interview_id, change, details, message_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![interview_id, change, details, message_id, now],
    )?;
    Ok(())
}

pub fn interview_history(
    conn: &Connection,
    interview_id: i64,
) -> AppResult<Vec<(String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT change, details FROM interview_history WHERE interview_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([interview_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn count_rows(conn: &Connection, table: &str) -> AppResult<i64> {
    // Only called with fixed table names from tests and reports.
    Ok(conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
}
