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
        ApplicationDetail, ApplicationRow, ApplicationStatus, ApplicationsOverview, CalendarState,
        Correspondence, InterviewView, TrackingStatus, UpdateSource,
    },
};

/// One row of the Applications page: the current state only.
pub fn row(conn: &Connection, app: ApplicationRecord, now: i64) -> AppResult<ApplicationRow> {
    let upcoming = app.status == ApplicationStatus::UpcomingInterview;
    let next_interview = repo::interviews_for_application(conn, app.id)?
        .into_iter()
        .filter(|i| {
            i.state == repo::InterviewState::Confirmed && i.end_at.is_some_and(|end| end > now)
        })
        .min_by_key(|i| i.start_at)
        .filter(|_| upcoming);
    let latest_update = applications::latest_update_text(&app, next_interview.as_ref());
    let status_label = applications::status_label(&app);
    Ok(ApplicationRow {
        id: app.id,
        section: app.status.section(),
        status_label,
        latest_update,
        company: app.company,
        role: app.role,
        status: app.status,
        last_update_at: app.last_update_at,
        next_action: app.next_action.filter(|_| !upcoming),
        rejection_reason: app.rejection_reason,
        interview_at: next_interview.as_ref().and_then(|i| i.start_at),
        interview_timezone: next_interview.as_ref().and_then(|i| i.timezone.clone()),
        meeting_url: next_interview.as_ref().and_then(|i| i.meeting_url.clone()),
        calendar_conflict: next_interview
            .as_ref()
            .is_some_and(|i| i.calendar_state == CalendarState::Conflict),
        mail_provider: app.mail_provider.map(|p| p.as_str().to_string()),
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
        time_text: i
            .start_at
            .map(|start| applications::interview_time(start, i.timezone.as_deref())),
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

/// Every application in its one current section, latest email first.
/// `tracking` is filled in by the caller (it depends on tasks and
/// connectors).
pub fn overview(conn: &Connection, now: i64) -> AppResult<ApplicationsOverview> {
    let mut rows = Vec::new();
    for app in repo::all_applications(conn)? {
        rows.push(row(conn, app, now)?);
    }
    rows.sort_by_key(|r| (std::cmp::Reverse(r.last_update_at), std::cmp::Reverse(r.id)));
    Ok(ApplicationsOverview {
        applications: rows,
        tracking: TrackingStatus {
            task_id: None,
            enabled: false,
            mail_connected: false,
            last_run_at: None,
            next_run_at: None,
        },
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
    // The user's change is the newest state: older mail read later never
    // overrides it.
    app.status_at = app.status_at.max(now);
    app.status_category = None;
    app.latest_update = note.map(|n| n.chars().take(160).collect());
    if status != ApplicationStatus::Rejected {
        app.rejection_reason = None;
    }
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
        let mut app = ApplicationRecord::new("SAP SE", ApplicationStatus::InProcess, 10);
        app.company_key = applications::company_key("SAP SE");
        app.role = Some("AI Engineer".into());
        app.role_key = Some(applications::role_key("AI Engineer"));
        app.sender_domain = Some("sap.com".into());
        repo::insert_application(c, &app)
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
            assert_eq!(overview.applications.len(), 1);
            let row = &overview.applications[0];
            assert_eq!(
                row.section,
                crate::models::jobs::ApplicationSection::NeedsAction
            );
            assert_eq!(row.latest_update, "Call on Monday");
            Ok(())
        })
        .unwrap();
    }
}
