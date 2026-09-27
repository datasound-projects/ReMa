//! Job applications, the mail ReMa has looked at, interviews and the
//! application timeline.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::{
        connectors::ProviderId,
        jobs::{
            ApplicationStatus, CalendarState, ConflictingEvent, EmailCategory, ProposedSlot,
            TimelineEntry, UpdateSource,
        },
        text_enum,
    },
};

// ── Mail ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailStatus {
    /// Rejected by the deterministic prefilter; never shown to a model.
    Filtered,
    /// Some job signals: waiting for the headers-only relevance check.
    Candidate,
    /// Not about a job application.
    Irrelevant,
    /// Relevant, waiting for classification (per-run limit reached).
    Pending,
    Processed,
    Failed,
    /// Classified with low confidence: no state was changed.
    Ambiguous,
}

text_enum!(MailStatus {
    Filtered => "filtered",
    Candidate => "candidate",
    Irrelevant => "irrelevant",
    Pending => "pending",
    Processed => "processed",
    Failed => "failed",
    Ambiguous => "ambiguous",
});

/// How often a failing message is retried in later runs.
pub const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct MailRecord {
    pub provider: ProviderId,
    pub message_id: String,
    pub thread_id: String,
    pub received_at: i64,
    pub sender_domain: Option<String>,
    /// Kept only for job-related messages.
    pub sender: Option<String>,
    pub subject: Option<String>,
    pub web_link: Option<String>,
    pub status: MailStatus,
    pub prefilter_score: Option<i64>,
    pub attempts: u32,
    pub classification: Option<ApplicationStatus>,
    pub category: Option<EmailCategory>,
    pub confidence: Option<f64>,
    pub extraction: Option<String>,
    pub application_id: Option<i64>,
    pub error: Option<String>,
    pub first_seen_at: i64,
    pub processed_at: Option<i64>,
}

const MAIL_COLUMNS: &str = "provider, message_id, thread_id, received_at, sender_domain, sender,
    subject, web_link, status, prefilter_score, attempts, classification, category, confidence,
    extraction, application_id, error, first_seen_at, processed_at";

fn mail_from_row(row: &Row) -> rusqlite::Result<MailRecord> {
    let provider: String = row.get(0)?;
    let status: String = row.get(8)?;
    let classification: Option<String> = row.get(11)?;
    let category: Option<String> = row.get(12)?;
    Ok(MailRecord {
        provider: ProviderId::parse(&provider).unwrap_or(ProviderId::Google),
        message_id: row.get(1)?,
        thread_id: row.get(2)?,
        received_at: row.get(3)?,
        sender_domain: row.get(4)?,
        sender: row.get(5)?,
        subject: row.get(6)?,
        web_link: row.get(7)?,
        status: MailStatus::parse(&status).unwrap_or(MailStatus::Failed),
        prefilter_score: row.get(9)?,
        attempts: row.get(10)?,
        classification: classification.as_deref().and_then(ApplicationStatus::parse),
        category: category.as_deref().and_then(EmailCategory::parse),
        confidence: row.get(13)?,
        extraction: row.get(14)?,
        application_id: row.get(15)?,
        error: row.get(16)?,
        first_seen_at: row.get(17)?,
        processed_at: row.get(18)?,
    })
}

pub fn get_mail(
    conn: &Connection,
    provider: ProviderId,
    message_id: &str,
) -> AppResult<Option<MailRecord>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {MAIL_COLUMNS} FROM mail_messages WHERE provider = ?1 AND message_id = ?2"
            ),
            params![provider.as_str(), message_id],
            mail_from_row,
        )
        .optional()?)
}

/// Of `ids`, the messages recorded earlier (whatever their status).
pub fn known_messages(
    conn: &Connection,
    provider: ProviderId,
    ids: &[String],
) -> AppResult<HashSet<String>> {
    let mut known = HashSet::new();
    let mut stmt =
        conn.prepare("SELECT 1 FROM mail_messages WHERE provider = ?1 AND message_id = ?2")?;
    for id in ids {
        if stmt.exists(params![provider.as_str(), id])? {
            known.insert(id.clone());
        }
    }
    Ok(known)
}

pub fn upsert_mail(conn: &Connection, mail: &MailRecord) -> AppResult<()> {
    conn.execute(
        "INSERT INTO mail_messages (provider, message_id, thread_id, received_at, sender_domain,
             sender, subject, web_link, status, prefilter_score, attempts, classification, category,
             confidence, extraction, application_id, error, first_seen_at, processed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
         ON CONFLICT (provider, message_id) DO UPDATE SET
             sender = excluded.sender, subject = excluded.subject, web_link = excluded.web_link,
             status = excluded.status, prefilter_score = excluded.prefilter_score,
             attempts = excluded.attempts, classification = excluded.classification,
             category = excluded.category, confidence = excluded.confidence,
             extraction = excluded.extraction, application_id = excluded.application_id,
             error = excluded.error, processed_at = excluded.processed_at",
        params![
            mail.provider.as_str(),
            mail.message_id,
            mail.thread_id,
            mail.received_at,
            mail.sender_domain,
            mail.sender,
            mail.subject,
            mail.web_link,
            mail.status.as_str(),
            mail.prefilter_score,
            mail.attempts,
            mail.classification.map(ApplicationStatus::as_str),
            mail.category.map(EmailCategory::as_str),
            mail.confidence,
            mail.extraction,
            mail.application_id,
            mail.error,
            mail.first_seen_at,
            mail.processed_at,
        ],
    )?;
    Ok(())
}

fn query_mail(
    conn: &Connection,
    sql_where: &str,
    args: impl rusqlite::Params,
) -> AppResult<Vec<MailRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MAIL_COLUMNS} FROM mail_messages {sql_where}"
    ))?;
    let rows = stmt.query_map(args, mail_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Messages waiting for the headers-only relevance check, oldest first.
pub fn candidate_mail(conn: &Connection, provider: ProviderId) -> AppResult<Vec<MailRecord>> {
    query_mail(
        conn,
        "WHERE provider = ?1 AND status = 'candidate' ORDER BY received_at, message_id",
        [provider.as_str()],
    )
}

/// Relevant messages still waiting for classification, oldest first.
pub fn pending_mail(conn: &Connection, provider: ProviderId) -> AppResult<Vec<MailRecord>> {
    query_mail(
        conn,
        "WHERE provider = ?1 AND (status = 'pending' OR (status = 'failed' AND attempts < ?2))
         ORDER BY received_at, message_id",
        params![provider.as_str(), MAX_ATTEMPTS],
    )
}

/// Relevant messages of every mailbox waiting for classification, oldest
/// first: history is replayed in the order it happened.
pub fn pending_mail_all(conn: &Connection) -> AppResult<Vec<MailRecord>> {
    query_mail(
        conn,
        "WHERE status = 'pending' OR (status = 'failed' AND attempts < ?1)
         ORDER BY received_at, provider, message_id",
        [MAX_ATTEMPTS],
    )
}

/// The same email already processed from another mailbox (the user's Gmail
/// and Outlook both received it): same sender and subject, received within
/// ten minutes.
pub fn processed_twin(
    conn: &Connection,
    provider: ProviderId,
    sender: &str,
    subject: &str,
    received_at: i64,
) -> AppResult<Option<MailRecord>> {
    Ok(query_mail(
        conn,
        "WHERE provider != ?1 AND lower(sender) = lower(?2) AND subject = ?3
             AND abs(received_at - ?4) <= 600000 AND status IN ('processed', 'ambiguous')
         ORDER BY abs(received_at - ?4) LIMIT 1",
        params![provider.as_str(), sender, subject, received_at],
    )?
    .pop())
}

/// When a message was received, if ReMa has seen it.
pub fn received_at(
    conn: &Connection,
    provider: ProviderId,
    message_id: &str,
) -> AppResult<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT received_at FROM mail_messages WHERE provider = ?1 AND message_id = ?2",
            params![provider.as_str(), message_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Job-related messages of an application (correspondence), newest first.
pub fn mail_for_application(conn: &Connection, application_id: i64) -> AppResult<Vec<MailRecord>> {
    query_mail(
        conn,
        "WHERE application_id = ?1 ORDER BY received_at DESC LIMIT 50",
        [application_id],
    )
}

/// Low-confidence classifications, newest first.
pub fn ambiguous_mail(conn: &Connection, limit: usize) -> AppResult<Vec<MailRecord>> {
    query_mail(
        conn,
        "WHERE status = 'ambiguous' ORDER BY received_at DESC LIMIT ?1",
        [limit as i64],
    )
}

/// Whether a message is known to be job-related (the assistant's mail
/// tools only return such messages).
pub fn is_job_mail(conn: &Connection, provider: ProviderId, message_id: &str) -> AppResult<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM mail_messages WHERE provider = ?1 AND message_id = ?2
                 AND (application_id IS NOT NULL OR status IN ('pending', 'processed', 'ambiguous'))",
            params![provider.as_str(), message_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Addresses that sent job-related mail before (known recruiters).
pub fn known_job_senders(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT lower(sender) FROM mail_messages
         WHERE sender IS NOT NULL AND status IN ('processed', 'pending', 'ambiguous')",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut senders = HashSet::new();
    for sender in rows {
        let sender = sender?;
        let address = match (sender.rfind('<'), sender.rfind('>')) {
            (Some(s), Some(e)) if s < e => sender[s + 1..e].to_string(),
            _ => sender,
        };
        senders.insert(address.trim().to_string());
    }
    Ok(senders)
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
    /// One line for the Applications table (from the email that set the
    /// status).
    pub latest_update: Option<String>,
    /// Only when the rejection email states one.
    pub rejection_reason: Option<String>,
    /// The category of the email that set the current status.
    pub status_category: Option<EmailCategory>,
    /// Mailbox of the latest email.
    pub mail_provider: Option<ProviderId>,
    pub mail_account: Option<String>,
    /// When the email (or user change) that set the status happened.
    pub status_at: i64,
}

impl ApplicationRecord {
    /// A new application (tests and the tracker fill in the rest).
    pub fn new(company: &str, status: ApplicationStatus, now: i64) -> Self {
        Self {
            id: 0,
            company: company.to_string(),
            company_key: company.to_lowercase(),
            role: None,
            role_key: None,
            reference: None,
            sender_domain: None,
            status,
            requires_action: false,
            next_action: None,
            last_update_at: now,
            created_at: now,
            updated_at: now,
            latest_update: None,
            rejection_reason: None,
            status_category: None,
            mail_provider: None,
            mail_account: None,
            status_at: now,
        }
    }
}

const APP_COLUMNS: &str = "id, company, company_key, role, role_key, reference, sender_domain,
    status, requires_action, next_action, last_update_at, created_at, updated_at, latest_update,
    rejection_reason, status_category, mail_provider, mail_account, status_at";

fn app_from_row(row: &Row) -> rusqlite::Result<ApplicationRecord> {
    let status: String = row.get(7)?;
    let category: Option<String> = row.get(15)?;
    let provider: Option<String> = row.get(16)?;
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
        latest_update: row.get(13)?,
        rejection_reason: row.get(14)?,
        status_category: category.as_deref().and_then(EmailCategory::parse),
        mail_provider: provider.as_deref().and_then(ProviderId::parse),
        mail_account: row.get(17)?,
        status_at: row.get::<_, Option<i64>>(18)?.unwrap_or(row.get(10)?),
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

pub fn application_for_thread(
    conn: &Connection,
    provider: ProviderId,
    thread_id: &str,
) -> AppResult<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT application_id FROM application_threads WHERE provider = ?1 AND thread_id = ?2",
            params![provider.as_str(), thread_id],
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

/// Every application, most recently updated first.
pub fn all_applications(conn: &Connection) -> AppResult<Vec<ApplicationRecord>> {
    query_apps(conn, "ORDER BY last_update_at DESC, id DESC", [])
}

/// Updated since `since`, or still needing attention; newest first.
pub fn overview_applications(conn: &Connection, since: i64) -> AppResult<Vec<ApplicationRecord>> {
    query_apps(
        conn,
        "WHERE last_update_at >= ?1 OR status IN ('needs_action', 'upcoming_interview', 'offer')
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
             sender_domain, status, requires_action, next_action, last_update_at, created_at,
             updated_at, latest_update, rejection_reason, status_category, mail_provider,
             mail_account, status_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
            app.latest_update,
            app.rejection_reason,
            app.status_category.map(EmailCategory::as_str),
            app.mail_provider.map(ProviderId::as_str),
            app.mail_account,
            app.status_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn save_application(conn: &Connection, app: &ApplicationRecord) -> AppResult<()> {
    conn.execute(
        "UPDATE job_applications SET company = ?2, company_key = ?3, role = ?4, role_key = ?5,
             reference = ?6, sender_domain = ?7, status = ?8, requires_action = ?9,
             next_action = ?10, last_update_at = ?11, updated_at = ?12, latest_update = ?13,
             rejection_reason = ?14, status_category = ?15, mail_provider = ?16,
             mail_account = ?17, status_at = ?18
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
            app.latest_update,
            app.rejection_reason,
            app.status_category.map(EmailCategory::as_str),
            app.mail_provider.map(ProviderId::as_str),
            app.mail_account,
            app.status_at,
        ],
    )?;
    Ok(())
}

/// Links a mail thread to an application (first link wins).
pub fn link_thread(
    conn: &Connection,
    provider: ProviderId,
    thread_id: &str,
    application_id: i64,
) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO application_threads (provider, thread_id, application_id)
         VALUES (?1, ?2, ?3)",
        params![provider.as_str(), thread_id, application_id],
    )?;
    Ok(())
}

/// Thread ids known to belong to applications.
pub fn application_threads(conn: &Connection, provider: ProviderId) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT thread_id FROM application_threads WHERE provider = ?1")?;
    let rows = stmt.query_map([provider.as_str()], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

// ── Timeline ────────────────────────────────────────────────────────

/// One audit entry for the application timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineRecord<'a> {
    pub application_id: i64,
    pub source: UpdateSource,
    pub provider: Option<ProviderId>,
    pub message_id: Option<&'a str>,
    pub category: Option<EmailCategory>,
    pub confidence: Option<f64>,
    pub status: ApplicationStatus,
    pub previous_status: Option<ApplicationStatus>,
    pub change: &'a str,
    pub summary: Option<&'a str>,
    pub occurred_at: i64,
    pub created_at: i64,
}

/// Adds a timeline entry. One entry per message and application.
pub fn add_timeline(conn: &Connection, entry: &TimelineRecord<'_>) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO application_updates (application_id, source, provider, message_id,
             category, confidence, status, previous_status, change, summary, occurred_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            entry.application_id,
            entry.source.as_str(),
            entry.provider.map(ProviderId::as_str),
            entry.message_id,
            entry.category.map(EmailCategory::as_str),
            entry.confidence,
            entry.status.as_str(),
            entry.previous_status.map(ApplicationStatus::as_str),
            entry.change,
            entry.summary,
            entry.occurred_at,
            entry.created_at,
        ],
    )?;
    Ok(())
}

pub fn timeline(conn: &Connection, application_id: i64) -> AppResult<Vec<TimelineEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, source, change, status, previous_status, category, confidence, summary,
             occurred_at, created_at
         FROM application_updates WHERE application_id = ?1 ORDER BY occurred_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([application_id], |r| {
        let source: String = r.get(1)?;
        let status: String = r.get(3)?;
        let previous: Option<String> = r.get(4)?;
        let category: Option<String> = r.get(5)?;
        Ok(TimelineEntry {
            id: r.get(0)?,
            source: UpdateSource::parse(&source).unwrap_or(UpdateSource::Gmail),
            change: r.get(2)?,
            status: ApplicationStatus::parse(&status).unwrap_or(ApplicationStatus::InProcess),
            previous_status: previous.as_deref().and_then(ApplicationStatus::parse),
            category: category.as_deref().and_then(EmailCategory::parse),
            confidence: r.get(6)?,
            summary: r.get(7)?,
            occurred_at: r.get(8)?,
            created_at: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Timeline entries created since `since` (dashboard "updates today").
pub fn updates_since(conn: &Connection, since: i64) -> AppResult<u32> {
    Ok(conn.query_row(
        "SELECT COUNT(DISTINCT application_id) FROM application_updates WHERE created_at >= ?1",
        [since],
        |r| r.get(0),
    )?)
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

#[derive(Debug, Clone, PartialEq)]
pub struct InterviewRecord {
    pub id: i64,
    pub application_id: i64,
    /// The mail provider the interview came from.
    pub provider: ProviderId,
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
    pub confidence: Option<f64>,
    /// company | role | start | conversation, to recognize the same interview.
    pub fingerprint: Option<String>,
    pub calendar_provider: Option<ProviderId>,
    pub calendar_event_id: Option<String>,
    pub calendar_hash: Option<String>,
    pub calendar_synced_at: Option<i64>,
    pub calendar_state: CalendarState,
    pub conflicts: Vec<ConflictingEvent>,
    pub proposed_slots: Vec<ProposedSlot>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl InterviewRecord {
    pub fn blank(
        application_id: i64,
        provider: ProviderId,
        thread: &str,
        message: &str,
        now: i64,
    ) -> Self {
        Self {
            id: 0,
            application_id,
            provider,
            source_thread_id: thread.to_string(),
            source_message_id: message.to_string(),
            state: InterviewState::Proposed,
            interview_type: None,
            start_at: None,
            end_at: None,
            timezone: None,
            location: None,
            meeting_url: None,
            participants: Vec::new(),
            review_reason: None,
            confidence: None,
            fingerprint: None,
            calendar_provider: None,
            calendar_event_id: None,
            calendar_hash: None,
            calendar_synced_at: None,
            calendar_state: CalendarState::None,
            conflicts: Vec::new(),
            proposed_slots: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

const INTERVIEW_COLUMNS: &str = "id, application_id, provider, source_thread_id, source_message_id,
    state, interview_type, start_at, end_at, timezone, location, meeting_url, participants,
    review_reason, confidence, fingerprint, calendar_provider, calendar_event_id, calendar_hash,
    calendar_synced_at, calendar_state, conflicts, proposed_slots, created_at, updated_at";

fn interview_from_row(row: &Row) -> rusqlite::Result<InterviewRecord> {
    let provider: String = row.get(2)?;
    let state: String = row.get(5)?;
    let participants: Option<String> = row.get(12)?;
    let calendar_provider: Option<String> = row.get(16)?;
    let calendar_state: String = row.get(20)?;
    let conflicts: Option<String> = row.get(21)?;
    Ok(InterviewRecord {
        id: row.get(0)?,
        application_id: row.get(1)?,
        provider: ProviderId::parse(&provider).unwrap_or(ProviderId::Google),
        source_thread_id: row.get(3)?,
        source_message_id: row.get(4)?,
        state: InterviewState::parse(&state).unwrap_or(InterviewState::NeedsReview),
        interview_type: row.get(6)?,
        start_at: row.get(7)?,
        end_at: row.get(8)?,
        timezone: row.get(9)?,
        location: row.get(10)?,
        meeting_url: row.get(11)?,
        participants: participants
            .and_then(|p| serde_json::from_str(&p).ok())
            .unwrap_or_default(),
        review_reason: row.get(13)?,
        confidence: row.get(14)?,
        fingerprint: row.get(15)?,
        calendar_provider: calendar_provider.as_deref().and_then(ProviderId::parse),
        calendar_event_id: row.get(17)?,
        calendar_hash: row.get(18)?,
        calendar_synced_at: row.get(19)?,
        calendar_state: CalendarState::parse(&calendar_state).unwrap_or(CalendarState::None),
        conflicts: conflicts
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default(),
        proposed_slots: row
            .get::<_, Option<String>>(22)?
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default(),
        created_at: row.get(23)?,
        updated_at: row.get(24)?,
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

pub fn get_interview(conn: &Connection, id: i64) -> AppResult<InterviewRecord> {
    query_interviews(conn, "WHERE id = ?1", [id])?
        .pop()
        .ok_or_else(|| AppError::not_found("Interview not found"))
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

/// Another interview (e.g. from a second thread) with the same fingerprint
/// that already has a calendar event.
pub fn interview_with_fingerprint(
    conn: &Connection,
    fingerprint: &str,
    except_id: i64,
) -> AppResult<Option<InterviewRecord>> {
    Ok(query_interviews(
        conn,
        "WHERE fingerprint = ?1 AND id != ?2 AND calendar_event_id IS NOT NULL LIMIT 1",
        params![fingerprint, except_id],
    )?
    .pop())
}

/// Confirmed interviews that have not ended, plus cancelled ones still
/// linked to a calendar event that ReMa may need to mark.
pub fn interviews_to_sync(conn: &Connection, now: i64) -> AppResult<Vec<InterviewRecord>> {
    query_interviews(
        conn,
        "WHERE (state = 'confirmed' AND end_at > ?1)
            OR (state = 'cancelled' AND calendar_event_id IS NOT NULL AND end_at > ?1)
         ORDER BY start_at, id",
        [now],
    )
}

/// Confirmed interviews in a time window (the assistant's "interviews next week").
pub fn interviews_between(
    conn: &Connection,
    from: i64,
    to: i64,
) -> AppResult<Vec<InterviewRecord>> {
    query_interviews(
        conn,
        "WHERE state = 'confirmed' AND start_at >= ?1 AND start_at < ?2 ORDER BY start_at",
        params![from, to],
    )
}

/// Confirmed or cancelled interviews overlapping `[from, to)` (the in-app
/// calendar).
pub fn interviews_overlapping(
    conn: &Connection,
    from: i64,
    to: i64,
) -> AppResult<Vec<InterviewRecord>> {
    query_interviews(
        conn,
        "WHERE state IN ('confirmed', 'cancelled') AND start_at < ?2 AND end_at > ?1
         ORDER BY start_at, id",
        params![from, to],
    )
}

pub fn interviews_needing_review(conn: &Connection, since: i64) -> AppResult<Vec<InterviewRecord>> {
    query_interviews(
        conn,
        "WHERE state = 'needs_review' AND updated_at >= ?1 ORDER BY id",
        [since],
    )
}

fn json_list<T: serde::Serialize>(items: &[T]) -> Option<String> {
    (!items.is_empty()).then(|| serde_json::to_string(items).unwrap_or_default())
}

pub fn insert_interview(conn: &Connection, i: &InterviewRecord) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO interviews (application_id, provider, source_thread_id, source_message_id,
             state, interview_type, start_at, end_at, timezone, location, meeting_url, participants,
             review_reason, confidence, fingerprint, calendar_provider, calendar_event_id,
             calendar_hash, calendar_synced_at, calendar_state, conflicts, proposed_slots,
             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
             ?19, ?20, ?21, ?22, ?23, ?24)",
        params![
            i.application_id,
            i.provider.as_str(),
            i.source_thread_id,
            i.source_message_id,
            i.state.as_str(),
            i.interview_type,
            i.start_at,
            i.end_at,
            i.timezone,
            i.location,
            i.meeting_url,
            json_list(&i.participants),
            i.review_reason,
            i.confidence,
            i.fingerprint,
            i.calendar_provider.map(ProviderId::as_str),
            i.calendar_event_id,
            i.calendar_hash,
            i.calendar_synced_at,
            i.calendar_state.as_str(),
            json_list(&i.conflicts),
            json_list(&i.proposed_slots),
            i.created_at,
            i.updated_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn save_interview(conn: &Connection, i: &InterviewRecord) -> AppResult<()> {
    conn.execute(
        "UPDATE interviews SET provider = ?2, source_thread_id = ?3, source_message_id = ?4,
             state = ?5, interview_type = ?6, start_at = ?7, end_at = ?8, timezone = ?9,
             location = ?10, meeting_url = ?11, participants = ?12, review_reason = ?13,
             confidence = ?14, fingerprint = ?15, calendar_provider = ?16, calendar_event_id = ?17,
             calendar_hash = ?18, calendar_synced_at = ?19, calendar_state = ?20, conflicts = ?21,
             proposed_slots = ?22, updated_at = ?23
         WHERE id = ?1",
        params![
            i.id,
            i.provider.as_str(),
            i.source_thread_id,
            i.source_message_id,
            i.state.as_str(),
            i.interview_type,
            i.start_at,
            i.end_at,
            i.timezone,
            i.location,
            i.meeting_url,
            json_list(&i.participants),
            i.review_reason,
            i.confidence,
            i.fingerprint,
            i.calendar_provider.map(ProviderId::as_str),
            i.calendar_event_id,
            i.calendar_hash,
            i.calendar_synced_at,
            i.calendar_state.as_str(),
            json_list(&i.conflicts),
            json_list(&i.proposed_slots),
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
