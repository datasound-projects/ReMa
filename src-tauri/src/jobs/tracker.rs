//! The application tracker as the interface and the assistant see it:
//! overview with dashboard counts, one application's timeline, interviews
//! and correspondence, and changes made by the user or (with approval) the
//! assistant — each leaving an audit entry.

use rusqlite::Connection;

use super::applications;
use crate::{
    db::jobs::{self as repo, ApplicationRecord, InterviewRecord, MailRecord, TimelineRecord},
    error::{AppError, AppResult},
    models::jobs::{
        ApplicationDetail, ApplicationRow, ApplicationStatus, ApplicationsOverview,
        ApplicationsSummary, Correspondence, InterviewView, UpdateSource,
    },
};

const DAY_MS: i64 = 86_400_000;

/// One overview row (next interview for upcoming ones).
pub fn row(conn: &Connection, app: ApplicationRecord, now: i64) -> AppResult<ApplicationRow> {
    let next_interview = repo::interviews_for_application(conn, app.id)?
        .into_iter()
        .filter(|i| {
            i.state == repo::InterviewState::Confirmed && i.end_at.is_some_and(|end| end > now)
        })
        .min_by_key(|i| i.start_at);
    Ok(ApplicationRow {
        id: app.id,
        company: app.company,
        role: app.role,
        status: app.status,
        last_update_at: app.last_update_at,
        next_action: app.next_action,
        interview_at: next_interview.as_ref().and_then(|i| i.start_at),
        interview_timezone: next_interview.and_then(|i| i.timezone),
    })
}

pub fn correspondence(mail: MailRecord) -> Correspondence {
    Correspondence {
        provider: mail.provider.as_str().to_string(),
        message_id: mail.message_id,
        subject: mail.subject,
        sender: mail.sender,
        received_at: mail.received_at,
        category: mail.category,
        confidence: mail.confidence,
        web_link: mail.web_link,
        status: mail.status.as_str().to_string(),
    }
}

pub fn interview_view(i: InterviewRecord) -> InterviewView {
    InterviewView {
        id: i.id,
        state: i.state.as_str().to_string(),
        interview_type: i.interview_type,
        start_at: i.start_at,
        end_at: i.end_at,
        timezone: i.timezone,
        location: i.location,
        meeting_url: i.meeting_url,
        participants: i.participants,
        review_reason: i.review_reason,
        calendar_state: i.calendar_state,
        calendar_provider: i.calendar_provider.map(|p| p.as_str().to_string()),
        conflicts: i.conflicts,
        proposed_slots: i.proposed_slots,
    }
}

/// Midnight (local time) of the day `now` falls on.
fn start_of_today(now: i64) -> i64 {
    jiff::Timestamp::from_millisecond(now)
        .ok()
        .and_then(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .start_of_day()
                .ok()
                .map(|z| z.timestamp().as_millisecond())
        })
        .unwrap_or(now - now.rem_euclid(DAY_MS))
}

pub fn overview(conn: &Connection, now: i64) -> AppResult<ApplicationsOverview> {
    let apps = repo::all_applications(conn)?;
    let total = apps.len() as u32;
    let mut rows = Vec::with_capacity(apps.len());
    for app in apps {
        rows.push(row(conn, app, now)?);
    }
    let summary = ApplicationsSummary {
        updates_today: repo::updates_since(conn, start_of_today(now))?,
        interviews_scheduled: rows.iter().filter(|r| r.interview_at.is_some()).count() as u32,
        action_required: rows
            .iter()
            .filter(|r| r.status == ApplicationStatus::NeedsAction)
            .count() as u32,
        offers: rows
            .iter()
            .filter(|r| r.status == ApplicationStatus::Offer)
            .count() as u32,
        total,
    };
    Ok(ApplicationsOverview {
        summary,
        applications: rows,
        needs_review: repo::ambiguous_mail(conn, 20)?
            .into_iter()
            .map(correspondence)
            .collect(),
    })
}

pub fn detail(conn: &Connection, id: i64, now: i64) -> AppResult<ApplicationDetail> {
    let app = repo::get_application(conn, id)?;
    let reference = app.reference.clone();
    Ok(ApplicationDetail {
        timeline: repo::timeline(conn, id)?,
        interviews: repo::interviews_for_application(conn, id)?
            .into_iter()
            .map(interview_view)
            .collect(),
        correspondence: repo::mail_for_application(conn, id)?
            .into_iter()
            .map(correspondence)
            .collect(),
        application: row(conn, app, now)?,
        reference,
    })
}

/// Applications whose company or role matches (assistant: "What happened
/// with my SAP application?").
pub fn find(
    conn: &Connection,
    company: &str,
    role: Option<&str>,
) -> AppResult<Vec<ApplicationRecord>> {
    let key = applications::company_key(company);
    let role_key = role.map(applications::role_key);
    let mut apps: Vec<ApplicationRecord> = repo::all_applications(conn)?
        .into_iter()
        .filter(|a| {
            !key.is_empty()
                && (a.company_key == key
                    || a.company_key.contains(&key)
                    || key.contains(&a.company_key))
        })
        .collect();
    if let Some(role_key) = role_key.filter(|r| !r.is_empty()) {
        let matching: Vec<_> = apps
            .iter()
            .filter(|a| {
                a.role_key
                    .as_deref()
                    .is_some_and(|r| r.contains(&role_key) || role_key.contains(r))
            })
            .cloned()
            .collect();
        if !matching.is_empty() {
            apps = matching;
        }
    }
    Ok(apps)
}

/// A status change by the user or (approved) the assistant, with an audit entry.
pub fn set_status(
    conn: &mut Connection,
    id: i64,
    status: ApplicationStatus,
    note: Option<&str>,
    source: UpdateSource,
    now: i64,
) -> AppResult<ApplicationRecord> {
    let tx = conn.transaction()?;
    let mut app = repo::get_application(&tx, id)?;
    let previous = app.status;
    app.status = status;
    app.requires_action = status == ApplicationStatus::NeedsAction;
    if status != ApplicationStatus::NeedsAction {
        app.next_action = None;
    }
    app.last_update_at = app.last_update_at.max(now);
    app.updated_at = now;
    repo::save_application(&tx, &app)?;
    let change = applications::change_text(Some(previous), status, "Status confirmed");
    let summary = note.map(|n| n.chars().take(240).collect::<String>());
    repo::add_timeline(
        &tx,
        &TimelineRecord {
            application_id: id,
            source,
            provider: None,
            message_id: None,
            category: None,
            confidence: None,
            status,
            previous_status: Some(previous),
            change: &change,
            summary: summary.as_deref(),
            occurred_at: now,
            created_at: now,
        },
    )?;
    tx.commit()?;
    Ok(app)
}

/// A note on the timeline (no status change).
pub fn append_note(
    conn: &Connection,
    id: i64,
    note: &str,
    source: UpdateSource,
    now: i64,
) -> AppResult<()> {
    let note: String = note.trim().chars().take(240).collect();
    if note.is_empty() {
        return Err(AppError::validation("Write a note for the timeline."));
    }
    let app = repo::get_application(conn, id)?;
    repo::add_timeline(
        conn,
        &TimelineRecord {
            application_id: id,
            source,
            provider: None,
            message_id: None,
            category: None,
            confidence: None,
            status: app.status,
            previous_status: Some(app.status),
            change: "Note",
            summary: Some(&note),
            occurred_at: now,
            created_at: now,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn seed(c: &Connection) -> AppResult<i64> {
        repo::insert_application(
            c,
            &ApplicationRecord {
                id: 0,
                company: "SAP SE".into(),
                company_key: applications::company_key("SAP SE"),
                role: Some("AI Engineer".into()),
                role_key: Some(applications::role_key("AI Engineer")),
                reference: None,
                sender_domain: Some("sap.com".into()),
                status: ApplicationStatus::InProcess,
                requires_action: false,
                next_action: None,
                last_update_at: 10,
                created_at: 10,
                updated_at: 10,
            },
        )
    }

    #[test]
    fn user_changes_are_audited_and_found_by_company() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let id = seed(c)?;
            assert_eq!(find(c, "sap", None)?.len(), 1);
            assert_eq!(find(c, "SAP", Some("ai engineer"))?.len(), 1);
            assert!(find(c, "Stripe", None)?.is_empty());
            set_status(
                c,
                id,
                ApplicationStatus::Offer,
                Some("Call on Monday"),
                UpdateSource::User,
                20,
            )?;
            append_note(c, id, "Negotiate start date", UpdateSource::Assistant, 30)?;
            let detail = detail(c, id, 40)?;
            assert_eq!(detail.application.status, ApplicationStatus::Offer);
            assert_eq!(detail.timeline[0].change, "Note");
            assert_eq!(detail.timeline[0].source, UpdateSource::Assistant);
            assert_eq!(detail.timeline[1].change, "Application changed to Offer");
            assert_eq!(detail.timeline[1].source, UpdateSource::User);
            let overview = overview(c, 40)?;
            assert_eq!(overview.summary.offers, 1);
            assert_eq!(overview.summary.total, 1);
            Ok(())
        })
        .unwrap();
    }
}
