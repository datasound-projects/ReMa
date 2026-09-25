//! Keeps Google Calendar in step with confirmed interviews. Safe to run
//! any number of times.
//!
//! Per interview: find its event (stored id, or the private property if a
//! previous run lost the id) → create if missing, update only if the content
//! changed, otherwise leave it alone. Cancelled interviews keep their event,
//! renamed "Cancelled: …" and marked free. Events the user deleted are not
//! recreated unless the interview itself changed. Conflicts are reported,
//! never resolved.

use crate::{
    db::jobs::{self as repo, ApplicationRecord, InterviewRecord, InterviewState},
    error::AppResult,
    integrations::google::calendar::{find_conflicts, CalendarApi, EventDraft},
    models::jobs::{CalendarItem, CalendarOutcome, ConflictingEvent},
    state::AppState,
};

/// The event ReMa writes. Contains logistics only, never email content.
pub fn event_draft(interview: &InterviewRecord, app: &ApplicationRecord) -> Option<EventDraft> {
    let (start_at, end_at) = (interview.start_at?, interview.end_at?);
    let title = match &app.role {
        Some(role) => format!("{role} Interview — {}", app.company),
        None => format!("Interview — {}", app.company),
    };
    let cancelled = interview.state == InterviewState::Cancelled;
    let mut lines = Vec::new();
    if cancelled {
        lines.push("This interview was cancelled.".to_string());
        lines.push(String::new());
    }
    if let Some(role) = &app.role {
        lines.push(format!("Role: {role}"));
    }
    lines.push(format!("Company: {}", app.company));
    if let Some(kind) = &interview.interview_type {
        lines.push(format!("Interview type: {kind}"));
    }
    if let Some(url) = &interview.meeting_url {
        lines.push(format!("Meeting: {url}"));
    }
    if !interview.participants.is_empty() {
        lines.push(format!(
            "Participants: {}",
            interview.participants.join(", ")
        ));
    }
    lines.push(String::new());
    lines.push("Added by ReMa from a job application email.".into());
    Some(EventDraft {
        interview_id: interview.id,
        summary: if cancelled {
            format!("Cancelled: {title}")
        } else {
            title
        },
        description: lines.join("\n"),
        location: interview
            .location
            .clone()
            .or_else(|| interview.meeting_url.clone()),
        start_at,
        end_at,
        timezone: interview.timezone.clone().unwrap_or_else(|| "UTC".into()),
        cancelled,
    })
}

fn item(
    app: &ApplicationRecord,
    interview: &InterviewRecord,
    outcome: CalendarOutcome,
) -> CalendarItem {
    CalendarItem {
        company: app.company.clone(),
        role: app.role.clone(),
        outcome,
        start_at: interview.start_at,
        end_at: interview.end_at,
        timezone: interview.timezone.clone(),
        conflicts: Vec::new(),
        note: None,
    }
}

/// Synchronizes one interview and records the result.
pub async fn sync_interview(
    state: &AppState,
    api: &dyn CalendarApi,
    detect_conflicts: bool,
    interview: &InterviewRecord,
    app: &ApplicationRecord,
    now: i64,
) -> AppResult<CalendarItem> {
    let Some(draft) = event_draft(interview, app) else {
        return Ok(item(app, interview, CalendarOutcome::NeedsReview));
    };
    let hash = draft.content_hash();
    let mut record = interview.clone();

    // Recover an event created by a run whose database write was lost.
    if record.calendar_event_id.is_none() {
        if let Some(existing) = api.find_by_interview(record.id).await? {
            record.calendar_event_id = Some(existing.id);
            record.calendar_hash = None; // unknown content: compare below
        }
    }

    let cancelled = record.state == InterviewState::Cancelled;
    let mut result = item(app, &record, CalendarOutcome::Unchanged);

    if !cancelled && detect_conflicts {
        let events = api.list_events(draft.start_at, draft.end_at).await?;
        result.conflicts = find_conflicts(&events, draft.start_at, draft.end_at, record.id)
            .into_iter()
            .filter(|e| Some(&e.id) != record.calendar_event_id.as_ref())
            .map(|e| ConflictingEvent {
                title: e.title,
                start_at: e.start_at.unwrap_or_default(),
                end_at: e.end_at.unwrap_or_default(),
            })
            .collect();
    }

    let unchanged = record.calendar_hash.as_deref() == Some(hash.as_str());
    result.outcome = match record.calendar_event_id.clone() {
        Some(event_id) => match api.get_event(&event_id).await? {
            Some(_) if unchanged => CalendarOutcome::Unchanged,
            Some(_) => {
                api.update_event(&event_id, &draft).await?;
                if cancelled {
                    CalendarOutcome::Cancelled
                } else {
                    CalendarOutcome::Updated
                }
            }
            // Deleted by the user: respect that unless the interview changed.
            None if unchanged || cancelled => {
                result.note =
                    Some("The event was deleted in Calendar; ReMa did not recreate it.".into());
                CalendarOutcome::Removed
            }
            None => {
                record.calendar_event_id = Some(api.create_event(&draft).await?);
                CalendarOutcome::Created
            }
        },
        None if cancelled => CalendarOutcome::Unchanged,
        None => {
            record.calendar_event_id = Some(api.create_event(&draft).await?);
            CalendarOutcome::Created
        }
    };

    if matches!(
        result.outcome,
        CalendarOutcome::Created | CalendarOutcome::Updated | CalendarOutcome::Cancelled
    ) || record.calendar_hash.is_none()
    {
        record.calendar_hash = Some(hash);
        record.calendar_synced_at = Some(now);
        record.updated_at = now;
        let change = match result.outcome {
            CalendarOutcome::Created => Some("calendar_created"),
            CalendarOutcome::Updated => Some("calendar_updated"),
            CalendarOutcome::Cancelled => Some("calendar_cancelled"),
            _ => None,
        };
        state.db.call(|c| {
            repo::save_interview(c, &record)?;
            if let Some(change) = change {
                repo::add_interview_history(
                    c,
                    record.id,
                    change,
                    record.calendar_event_id.as_deref(),
                    None,
                    now,
                )?;
            }
            Ok(())
        })?;
    }
    Ok(result)
}

/// Report entry for an interview that was not written to Calendar.
pub fn review_item(app: &ApplicationRecord, interview: &InterviewRecord) -> CalendarItem {
    let mut entry = item(app, interview, CalendarOutcome::NeedsReview);
    entry.note = interview.review_reason.clone();
    entry
}
