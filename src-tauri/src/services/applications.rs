//! The application tracker for the interface: overview, details, the
//! user's changes and interview calendar actions.

use crate::{
    connectors::{self, sync},
    db::{connectors as connector_repo, jobs as repo},
    error::{AppError, AppResult},
    jobs::{calendar_sync, tracker},
    models::{
        connectors::{ConnectorId, ConnectorKind},
        jobs::{ApplicationDetail, ApplicationStatus, ApplicationsOverview, UpdateSource},
    },
    state::AppState,
    time::now_ms,
};

pub fn overview(state: &AppState) -> AppResult<ApplicationsOverview> {
    let now = now_ms();
    state.db.call(|c| tracker::overview(c, now))
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
    let buffer_ms = i64::from(
        state
            .db
            .call(|c| connector_repo::preferences(c))?
            .prep_buffer_minutes,
    ) * 60_000;
    let record = calendar_sync::add_to_calendar(
        state,
        calendar.as_ref(),
        interview_id,
        allow_conflict,
        buffer_ms,
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
