//! The application tracker for the interface: overview, details, the
//! user's changes and interview calendar actions.

use crate::{
    connectors::{self, sync},
    db::jobs as repo,
    error::{AppError, AppResult},
    jobs::{calendar_sync, tracker},
    models::{
        connectors::{ConnectorId, ConnectorKind},
        jobs::{
            ApplicationDetail, ApplicationStatus, ApplicationsOverview, TrackingStatus,
            UpdateSource,
        },
    },
    state::AppState,
    time::now_ms,
};

/// Every application in its current section, and whether mail is being
/// tracked (only by "Job Mail & Interview Sync").
pub fn overview(state: &AppState) -> AppResult<ApplicationsOverview> {
    let now = now_ms();
    let mut overview = state.db.call(|c| tracker::overview(c, now))?;
    let task = crate::services::tasks::job_mail_sync(state)?;
    let mail_connected = state.db.call(|c| {
        Ok(crate::db::connectors::connectors(c)?
            .iter()
            .any(|r| r.enabled && r.id.kind() == ConnectorKind::Mail))
    })?;
    overview.tracking = TrackingStatus {
        task_id: task.as_ref().map(|t| t.id),
        enabled: task.as_ref().is_some_and(|t| t.enabled),
        mail_connected,
        last_run_at: task.as_ref().and_then(|t| t.last_run_at),
        next_run_at: task.as_ref().and_then(|t| t.next_run_at),
    };
    Ok(overview)
}

pub fn detail(state: &AppState, id: i64) -> AppResult<ApplicationDetail> {
    let now = now_ms();
    state.db.call(|c| tracker::detail(c, id, now))
}

pub fn set_status(
    state: &AppState,
    id: i64,
    status: ApplicationStatus,
    note: Option<String>,
) -> AppResult<ApplicationDetail> {
    let now = now_ms();
    state
        .db
        .call(|c| tracker::set_status(c, id, status, note.as_deref(), UpdateSource::User, now))?;
    state.events.applications_changed();
    detail(state, id)
}

/// The calendar for an interview: the one holding its event, else the one
/// of the provider it came from, else any connected calendar.
pub async fn calendar_for(
    state: &AppState,
    interview: &repo::InterviewRecord,
) -> AppResult<Box<dyn connectors::calendar::CalendarProvider>> {
    let ready = connectors::ready(state, ConnectorKind::Calendar).await;
    let pick = interview
        .calendar_provider
        .map(|p| ConnectorId::of(p, ConnectorKind::Calendar))
        .filter(|id| ready.contains(id))
        .or_else(|| {
            Some(ConnectorId::of(interview.provider, ConnectorKind::Calendar))
                .filter(|id| ready.contains(id))
        })
        .or_else(|| ready.first().copied())
        .ok_or_else(|| {
            AppError::configuration(
                "Connect Google Calendar or Outlook Calendar in Settings → Connectors first.",
            )
        })?;
    sync::calendar_client(state, pick).await
}

pub async fn add_interview_to_calendar(
    state: &AppState,
    interview_id: i64,
    allow_conflict: bool,
) -> AppResult<ApplicationDetail> {
    let interview = state.db.call(|c| repo::get_interview(c, interview_id))?;
    let calendar = calendar_for(state, &interview).await?;
    // Conflicts are checked in the other connected calendar too.
    let mut others = Vec::new();
    for id in connectors::ready(state, ConnectorKind::Calendar).await {
        if id.provider() != calendar.provider() {
            others.push(sync::calendar_client(state, id).await?);
        }
    }
    let calendars: Vec<&dyn connectors::calendar::CalendarProvider> =
        std::iter::once(calendar.as_ref())
            .chain(others.iter().map(|c| c.as_ref()))
            .collect();
    let record = calendar_sync::add_to_calendar(
        state,
        calendar.as_ref(),
        &calendars,
        interview_id,
        allow_conflict,
        0,
        now_ms(),
    )
    .await;
    state.events.applications_changed();
    detail(state, record?.application_id)
}

pub fn decline_interview_calendar(
    state: &AppState,
    interview_id: i64,
) -> AppResult<ApplicationDetail> {
    calendar_sync::decline(state, interview_id, now_ms())?;
    let interview = state.db.call(|c| repo::get_interview(c, interview_id))?;
    state.events.applications_changed();
    detail(state, interview.application_id)
}
