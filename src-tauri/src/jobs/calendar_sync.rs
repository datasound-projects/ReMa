//! Keeps the connected calendar in step with confirmed interviews. Safe to
//! run any number of times.
//!
//! Per confirmed, upcoming interview:
//! - An event ReMa already manages is updated when the interview changes
//!   (reschedule) — unless the new time conflicts, which is reported instead
//!   of moved. Cancelled interviews keep their event, renamed "Cancelled: …"
//!   and marked free. Events the user deleted are not recreated unless the
//!   interview itself changed.
//! - Without an event: the calendar is checked (with the preparation buffer).
//!   A conflict is reported and nothing is written. Otherwise, in "Ask
//!   before adding" mode the event is proposed to the user; in automatic mode
//!   it is created when the confirmation was classified with high confidence.
//! - Never twice: the stored event id, then the event's private interview
//!   property, then the interview fingerprint (company, role, start and
//!   conversation) are checked before anything is created.

use crate::{
    connectors::calendar::{find_conflicts, CalendarProvider, EventDraft},
    db::jobs::{self as repo, ApplicationRecord, InterviewRecord, InterviewState, TimelineRecord},
    error::{AppError, AppResult},
    models::{
        connectors::{InterviewMode, ProviderId},
        jobs::{CalendarItem, CalendarOutcome, CalendarState, ConflictingEvent, UpdateSource},
    },
    services::notifications::{self, Notice},
    state::AppState,
};

/// Automatic creation needs at least this classifier confidence.
pub const AUTO_MIN_CONFIDENCE: f64 = 0.85;

/// How the calendar step runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarPolicy {
    pub mode: InterviewMode,
    /// Preparation time required before and after (ms).
    pub buffer_ms: i64,
}

pub fn calendar_source(provider: ProviderId) -> UpdateSource {
    match provider {
        ProviderId::Google => UpdateSource::GoogleCalendar,
        ProviderId::Microsoft => UpdateSource::OutlookCalendar,
    }
}

pub fn calendar_name(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Google => "Google Calendar",
        ProviderId::Microsoft => "Outlook Calendar",
    }
}

/// "Interview — Company — Role".
pub fn event_title(app: &ApplicationRecord) -> String {
    match &app.role {
        Some(role) => format!("Interview — {} — {role}", app.company),
        None => format!("Interview — {}", app.company),
    }
}

/// The event ReMa writes. Logistics only, never email content.
pub fn event_draft(interview: &InterviewRecord, app: &ApplicationRecord) -> Option<EventDraft> {
    let (start_at, end_at) = (interview.start_at?, interview.end_at?);
    let title = event_title(app);
    let cancelled = interview.state == InterviewState::Cancelled;
    let mut lines = Vec::new();
    if cancelled {
        lines.push("This interview was cancelled.".to_string());
        lines.push(String::new());
    }
    if let Some(role) = &app.role {
        lines.push(format!("Application: {role}"));
    }
    lines.push(format!("Company: {}", app.company));
    if let Some(kind) = &interview.interview_type {
        lines.push(format!("Interview type: {kind}"));
    }
    if !interview.participants.is_empty() {
        lines.push(format!("Recruiter: {}", interview.participants.join(", ")));
    }
    if let Some(url) = &interview.meeting_url {
        lines.push(format!("Meeting: {url}"));
    }
    lines.push(String::new());
    lines.push("Source: ReMa job application tracker".into());
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

/// "Thursday 14:00" in the interview's own time zone.
pub fn when(start_at: i64, timezone: Option<&str>) -> String {
    let tz = timezone
        .and_then(|t| jiff::tz::TimeZone::get(t).ok())
        .unwrap_or(jiff::tz::TimeZone::UTC);
    jiff::Timestamp::from_millisecond(start_at)
        .map(|t| t.to_zoned(tz).strftime("%A %d %b %H:%M").to_string())
        .unwrap_or_default()
}

fn describe_conflicts(conflicts: &[ConflictingEvent], timezone: Option<&str>) -> String {
    let tz = timezone
        .and_then(|t| jiff::tz::TimeZone::get(t).ok())
        .unwrap_or(jiff::tz::TimeZone::UTC);
    let time = |ms: i64| {
        jiff::Timestamp::from_millisecond(ms)
            .map(|t| t.to_zoned(tz.clone()).strftime("%H:%M").to_string())
            .unwrap_or_default()
    };
    conflicts
        .iter()
        .take(3)
        .map(|c| format!("{} {}–{}", c.title, time(c.start_at), time(c.end_at)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Existing events overlapping the interview (plus buffer).
pub async fn conflicts_for(
    api: &dyn CalendarProvider,
    interview: &InterviewRecord,
    start_at: i64,
    end_at: i64,
    buffer_ms: i64,
) -> AppResult<Vec<ConflictingEvent>> {
    let events = api
        .list_events(start_at - buffer_ms, end_at + buffer_ms)
        .await?;
    Ok(
        find_conflicts(&events, start_at, end_at, interview.id, buffer_ms)
            .into_iter()
            .filter(|e| Some(&e.id) != interview.calendar_event_id.as_ref())
            .map(|e| ConflictingEvent {
                title: e.title,
                start_at: e.start_at.unwrap_or_default(),
                end_at: e.end_at.unwrap_or_default(),
            })
            .collect(),
    )
}

fn save_with_history(
    state: &AppState,
    record: &InterviewRecord,
    app: &ApplicationRecord,
    change: Option<&str>,
    timeline: Option<(UpdateSource, String)>,
    now: i64,
) -> AppResult<()> {
    state.db.call(|c| {
        let tx = c.transaction()?;
        repo::save_interview(&tx, record)?;
        if let Some(change) = change {
            repo::add_interview_history(
                &tx,
                record.id,
                change,
                record.calendar_event_id.as_deref(),
                None,
                now,
            )?;
        }
        if let Some((source, text)) = &timeline {
            repo::add_timeline(
                &tx,
                &TimelineRecord {
                    application_id: app.id,
                    source: *source,
                    provider: None,
                    message_id: None,
                    category: None,
                    confidence: None,
                    status: app.status,
                    previous_status: Some(app.status),
                    change: text,
                    summary: None,
                    occurred_at: now,
                    created_at: now,
                },
            )?;
        }
        tx.commit()?;
        Ok(())
    })
}

/// Synchronizes one interview with a calendar and records the result.
pub async fn sync_interview(
    state: &AppState,
    api: &dyn CalendarProvider,
    policy: CalendarPolicy,
    interview: &InterviewRecord,
    app: &ApplicationRecord,
    now: i64,
) -> AppResult<CalendarItem> {
    let Some(draft) = event_draft(interview, app) else {
        return Ok(item(app, interview, CalendarOutcome::NeedsReview));
    };
    let provider = api.provider();
    // An event lives in exactly one calendar.
    if interview.calendar_event_id.is_some() && interview.calendar_provider != Some(provider) {
        return Ok(item(app, interview, CalendarOutcome::Unchanged));
    }
    let hash = draft.content_hash();
    let mut record = interview.clone();
    let calendar = calendar_name(provider);

    // Recover an event created by a run whose database write was lost.
    if record.calendar_event_id.is_none() {
        if let Some(existing) = api.find_by_interview(record.id).await? {
            record.calendar_event_id = Some(existing.id);
            record.calendar_provider = Some(provider);
            record.calendar_state = CalendarState::Created;
            record.calendar_hash = None; // unknown content: compared below
        }
    }
    // The same interview, known from another email, already has an event.
    if record.calendar_event_id.is_none() {
        if let Some(fingerprint) = &record.fingerprint {
            let twin = state
                .db
                .call(|c| repo::interview_with_fingerprint(c, fingerprint, record.id))?;
            if twin.is_some() {
                let mut result = item(app, &record, CalendarOutcome::Unchanged);
                result.note = Some("Already in the calendar (same interview).".into());
                return Ok(result);
            }
        }
    }

    let cancelled = record.state == InterviewState::Cancelled;
    let unchanged = record.calendar_hash.as_deref() == Some(hash.as_str());
    let mut result = item(app, &record, CalendarOutcome::Unchanged);
    // Checked whenever an event may be written: a changed interview, or one
    // without an event yet (busy times can appear between runs).
    if !cancelled && (!unchanged || record.calendar_event_id.is_none()) {
        result.conflicts =
            conflicts_for(api, &record, draft.start_at, draft.end_at, policy.buffer_ms).await?;
    }
    let when_text = when(draft.start_at, Some(&draft.timezone));
    let role = app.role.clone().unwrap_or_default();
    let heading = if role.is_empty() {
        app.company.clone()
    } else {
        format!("{} — {role}", app.company)
    };

    let existing_event = match record.calendar_event_id.clone() {
        Some(id) => Some((id.clone(), api.get_event(&id).await?)),
        None => None,
    };
    let (outcome, change, timeline): (CalendarOutcome, Option<&str>, Option<String>) =
        match existing_event {
            Some((_, Some(_))) if unchanged => (CalendarOutcome::Unchanged, None, None),
            Some((event_id, Some(_))) if cancelled => {
                api.update_event(&event_id, &draft).await?;
                record.calendar_hash = Some(hash.clone());
                (
                    CalendarOutcome::Cancelled,
                    Some("calendar_cancelled"),
                    Some(format!("Interview event marked as cancelled in {calendar}")),
                )
            }
            Some((_, Some(_))) if !result.conflicts.is_empty() => {
                // A reschedule into a busy time: tell, do not move.
                record.calendar_state = CalendarState::Conflict;
                record.conflicts = result.conflicts.clone();
                notifications::add(
                    state,
                    Notice {
                        kind: "conflict",
                        title: "Interview rescheduled — conflict".into(),
                        body: format!(
                            "{heading}\n{when_text}\nConflict detected with: {}\nThe calendar event was not moved.",
                            describe_conflicts(&result.conflicts, Some(&draft.timezone))
                        ),
                        application_id: Some(app.id),
                        interview_id: Some(record.id),
                        dedupe_key: Some(format!("conflict:{}:{}", record.id, draft.start_at)),
                    },
                );
                result.note = Some(
                    "The new time conflicts with existing events; the event was not moved.".into(),
                );
                (CalendarOutcome::Conflict, None, None)
            }
            Some((event_id, Some(_))) => {
                api.update_event(&event_id, &draft).await?;
                record.calendar_hash = Some(hash.clone());
                record.calendar_state = CalendarState::Created;
                record.conflicts.clear();
                notifications::add(
                    state,
                    Notice {
                        kind: "interview",
                        title: "Interview rescheduled".into(),
                        body: format!("{heading}\n{when_text}\nYour {calendar} event was updated."),
                        application_id: Some(app.id),
                        interview_id: Some(record.id),
                        dedupe_key: Some(format!("moved:{}:{}", record.id, draft.start_at)),
                    },
                );
                (
                    CalendarOutcome::Updated,
                    Some("calendar_updated"),
                    Some(format!("Interview event updated in {calendar}")),
                )
            }
            // Deleted by the user: respect that unless the interview changed.
            Some((_, None)) if unchanged || cancelled => {
                record.calendar_state = CalendarState::Removed;
                result.note = Some(format!(
                    "The event was deleted in {calendar}; ReMa did not recreate it."
                ));
                (CalendarOutcome::Removed, None, None)
            }
            Some((_, None)) | None if cancelled => (CalendarOutcome::Unchanged, None, None),
            Some((_, None)) | None => {
                record.calendar_event_id = None;
                if record.calendar_state == CalendarState::Declined && unchanged {
                    (CalendarOutcome::Unchanged, None, None)
                } else if !result.conflicts.is_empty() {
                    record.calendar_state = CalendarState::Conflict;
                    record.calendar_provider = Some(provider);
                    record.conflicts = result.conflicts.clone();
                    record.calendar_hash = Some(hash.clone());
                    notifications::add(
                        state,
                        Notice {
                            kind: "conflict",
                            title: "Interview confirmed — conflict".into(),
                            body: format!(
                                "{heading}\n{when_text}\nConflict detected with: {}",
                                describe_conflicts(&result.conflicts, Some(&draft.timezone))
                            ),
                            application_id: Some(app.id),
                            interview_id: Some(record.id),
                            dedupe_key: Some(format!("conflict:{}:{}", record.id, draft.start_at)),
                        },
                    );
                    result.note = Some("Conflicts with existing events; nothing was added.".into());
                    (CalendarOutcome::Conflict, None, None)
                } else if policy.mode == InterviewMode::Auto
                    && record.confidence.unwrap_or(0.0) >= AUTO_MIN_CONFIDENCE
                {
                    record.calendar_event_id = Some(api.create_event(&draft).await?);
                    record.calendar_provider = Some(provider);
                    record.calendar_state = CalendarState::Created;
                    record.conflicts.clear();
                    record.calendar_hash = Some(hash.clone());
                    notifications::add(
                        state,
                        Notice {
                            kind: "interview",
                            title: "Interview confirmed".into(),
                            body: format!(
                                "{heading}\n{when_text}\nNo calendar conflicts — added to {calendar}."
                            ),
                            application_id: Some(app.id),
                            interview_id: Some(record.id),
                            dedupe_key: Some(format!("added:{}:{}", record.id, draft.start_at)),
                        },
                    );
                    (
                        CalendarOutcome::Created,
                        Some("calendar_created"),
                        Some(format!("Interview added to {calendar}")),
                    )
                } else {
                    // Ask before adding (the default), or not confident enough.
                    let first_proposal =
                        record.calendar_state != CalendarState::Proposed || !unchanged;
                    record.calendar_state = CalendarState::Proposed;
                    record.calendar_provider = Some(provider);
                    record.conflicts.clear();
                    record.calendar_hash = Some(hash.clone());
                    if first_proposal {
                        notifications::add(
                            state,
                            Notice {
                                kind: "interview",
                                title: "Interview confirmed".into(),
                                body: format!(
                                    "{heading}\n{when_text}\nNo calendar conflicts. Add it to {calendar} in Applications."
                                ),
                                application_id: Some(app.id),
                                interview_id: Some(record.id),
                                dedupe_key: Some(format!("proposed:{}:{}", record.id, draft.start_at)),
                            },
                        );
                    }
                    (CalendarOutcome::Proposed, None, None)
                }
            }
        };
    result.outcome = outcome;
    if outcome != CalendarOutcome::Unchanged || record != *interview {
        record.calendar_synced_at = Some(now);
        record.updated_at = now;
        let source = calendar_source(provider);
        save_with_history(
            state,
            &record,
            app,
            change,
            timeline.map(|t| (source, t)),
            now,
        )?;
    }
    Ok(result)
}

/// The user adds a proposed (or conflicting) interview to the calendar.
/// Conflicts are checked again; `allow_conflict` must confirm them.
pub async fn add_to_calendar(
    state: &AppState,
    api: &dyn CalendarProvider,
    interview_id: i64,
    allow_conflict: bool,
    buffer_ms: i64,
    now: i64,
) -> AppResult<InterviewRecord> {
    let (mut record, app) = state.db.call(|c| {
        let record = repo::get_interview(c, interview_id)?;
        let app = repo::get_application(c, record.application_id)?;
        Ok((record, app))
    })?;
    if record.state != InterviewState::Confirmed {
        return Err(AppError::validation(
            "Only confirmed interviews can be added to the calendar.",
        ));
    }
    let draft = event_draft(&record, &app).ok_or_else(|| {
        AppError::validation("The interview time is not known yet; check the email first.")
    })?;
    if record.end_at.is_some_and(|end| end <= now) {
        return Err(AppError::validation("This interview is already over."));
    }
    let provider = api.provider();
    let calendar = calendar_name(provider);
    let conflicts = conflicts_for(api, &record, draft.start_at, draft.end_at, buffer_ms).await?;
    if !conflicts.is_empty() && !allow_conflict {
        record.calendar_state = CalendarState::Conflict;
        record.conflicts = conflicts.clone();
        state.db.call(|c| repo::save_interview(c, &record))?;
        return Err(AppError::validation(format!(
            "This time conflicts with {}. Add it anyway?",
            describe_conflicts(&conflicts, Some(&draft.timezone))
        )));
    }
    // Recover or reuse an existing event instead of creating a second one.
    let existing = match (&record.calendar_event_id, record.calendar_provider) {
        (Some(id), Some(p)) if p == provider => api.get_event(id).await?.map(|e| e.id),
        _ => None,
    };
    let existing = match existing {
        Some(id) => Some(id),
        None => api.find_by_interview(record.id).await?.map(|e| e.id),
    };
    let (event_id, change, text) = match existing {
        Some(id) => {
            api.update_event(&id, &draft).await?;
            (
                id,
                "calendar_updated",
                format!("Interview event updated in {calendar}"),
            )
        }
        None => (
            api.create_event(&draft).await?,
            "calendar_created",
            format!("Interview added to {calendar}"),
        ),
    };
    record.calendar_event_id = Some(event_id);
    record.calendar_provider = Some(provider);
    record.calendar_state = CalendarState::Created;
    record.calendar_hash = Some(draft.content_hash());
    record.calendar_synced_at = Some(now);
    record.conflicts = conflicts;
    record.updated_at = now;
    save_with_history(
        state,
        &record,
        &app,
        Some(change),
        Some((UpdateSource::User, text)),
        now,
    )?;
    Ok(record)
}

/// The user does not want this interview in the calendar (not proposed
/// again unless the interview changes).
pub fn decline(state: &AppState, interview_id: i64, now: i64) -> AppResult<()> {
    state.db.call(|c| {
        let mut record = repo::get_interview(c, interview_id)?;
        let app = repo::get_application(c, record.application_id)?;
        record.calendar_state = CalendarState::Declined;
        record.calendar_hash = event_draft(&record, &app).map(|d| d.content_hash());
        record.updated_at = now;
        repo::save_interview(c, &record)
    })
}

/// Report entry for an interview that was not written to a calendar.
pub fn review_item(app: &ApplicationRecord, interview: &InterviewRecord) -> CalendarItem {
    let mut entry = item(app, interview, CalendarOutcome::NeedsReview);
    entry.note = interview.review_reason.clone();
    entry
}
