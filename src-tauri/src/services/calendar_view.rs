//! The in-app calendar behind the top-right calendar button: events read
//! live from the connected Google and Outlook calendars (the providers stay
//! the source of truth; nothing is stored), with ReMa's interviews marked.
//! A confirmed interview that is in no calendar (a conflict, or no calendar
//! connected) is shown from the application it belongs to, so the schedule
//! the user sees is complete. The same interview is never listed twice.

use std::collections::{HashMap, HashSet};

use crate::{
    connectors::{self, calendar::CalendarEvent, sync},
    db::{
        connectors as connector_repo,
        jobs::{self as repo, ApplicationRecord, InterviewRecord, InterviewState},
    },
    error::{AppError, AppResult},
    jobs::calendar_sync::event_title,
    models::{
        calendar::{CalendarEntry, CalendarSource, CalendarView, InterviewLink},
        connectors::{ConnectorId, ConnectorKind, ProviderId},
        jobs::CalendarState,
    },
    state::AppState,
};

const DAY_MS: i64 = 86_400_000;
/// The longest range one request may read.
pub const MAX_RANGE_DAYS: i64 = 62;

fn link(interview: &InterviewRecord, app: &ApplicationRecord, in_calendar: bool) -> InterviewLink {
    InterviewLink {
        application_id: app.id,
        interview_id: interview.id,
        company: app.company.clone(),
        role: app.role.clone(),
        in_calendar,
        conflict: !in_calendar && interview.calendar_state == CalendarState::Conflict,
        cancelled: interview.state == InterviewState::Cancelled,
    }
}

/// Provider events plus ReMa's interviews, merged into one list by start.
pub fn merge(
    events: Vec<(ProviderId, CalendarEvent)>,
    interviews: &[(InterviewRecord, ApplicationRecord)],
) -> Vec<CalendarEntry> {
    let by_id: HashMap<i64, &(InterviewRecord, ApplicationRecord)> =
        interviews.iter().map(|pair| (pair.0.id, pair)).collect();
    let by_event: HashMap<(ProviderId, &str), i64> = interviews
        .iter()
        .filter_map(|(i, _)| {
            Some((
                (i.calendar_provider?, i.calendar_event_id.as_deref()?),
                i.id,
            ))
        })
        .collect();
    let interview_of = |provider: ProviderId, event: &CalendarEvent| {
        event
            .interview_id
            .filter(|id| by_id.contains_key(id))
            .or_else(|| by_event.get(&(provider, event.id.as_str())).copied())
    };
    // One event per interview: the calendar ReMa wrote it to, if listed.
    let mut chosen: HashMap<i64, ProviderId> = HashMap::new();
    for (provider, event) in &events {
        if let Some(id) = interview_of(*provider, event) {
            let stored = by_id[&id].0.calendar_provider;
            match chosen.get(&id) {
                Some(existing) if Some(*existing) == stored => {}
                _ => {
                    chosen.insert(id, *provider);
                }
            }
        }
    }

    let mut entries = Vec::new();
    let mut shown: HashSet<i64> = HashSet::new();
    for (provider, event) in events {
        let (Some(start_at), Some(end_at)) = (event.start_at, event.end_at) else {
            continue;
        };
        let interview = interview_of(provider, &event)
            .filter(|id| chosen.get(id) == Some(&provider) && shown.insert(*id))
            .map(|id| by_id[&id]);
        if interview.is_none() && interview_of(provider, &event).is_some() {
            continue; // the same interview in another calendar
        }
        entries.push(CalendarEntry {
            key: format!("{}:{}", provider.as_str(), event.id),
            provider: Some(provider),
            title: event.title,
            start_at,
            end_at,
            all_day: event.all_day,
            location: event
                .location
                .or_else(|| interview.and_then(|(i, _)| i.location.clone())),
            meeting_url: event
                .meeting_url
                .or_else(|| interview.and_then(|(i, _)| i.meeting_url.clone())),
            web_link: event.web_link,
            interview: interview.map(|(i, app)| link(i, app, true)),
        });
    }
    // Confirmed interviews that are in no listed calendar.
    for (interview, app) in interviews {
        let (Some(start_at), Some(end_at)) = (interview.start_at, interview.end_at) else {
            continue;
        };
        if interview.state != InterviewState::Confirmed || shown.contains(&interview.id) {
            continue;
        }
        entries.push(CalendarEntry {
            key: format!("interview:{}", interview.id),
            provider: None,
            title: event_title(app),
            start_at,
            end_at,
            all_day: false,
            location: interview.location.clone(),
            meeting_url: interview.meeting_url.clone(),
            web_link: None,
            interview: Some(link(interview, app, false)),
        });
    }
    // All-day events first, then by start.
    entries.sort_by(|a, b| {
        (!a.all_day, a.start_at, &a.title).cmp(&(!b.all_day, b.start_at, &b.title))
    });
    entries
}

/// The calendar between `start` and `end` (epoch ms).
pub async fn view(state: &AppState, start: i64, end: i64) -> AppResult<CalendarView> {
    if end <= start || end - start > MAX_RANGE_DAYS * DAY_MS {
        return Err(AppError::validation(format!(
            "Choose a range of at most {MAX_RANGE_DAYS} days."
        )));
    }
    let mut calendars = Vec::new();
    let mut events = Vec::new();
    let mut problems = Vec::new();
    for id in ConnectorId::ALL
        .into_iter()
        .filter(|id| id.kind() == ConnectorKind::Calendar)
    {
        let (record, account) = state.db.call(|c| {
            Ok((
                connector_repo::connector(c, id)?,
                connector_repo::account(c, id.provider())?,
            ))
        })?;
        if !record.enabled {
            continue;
        }
        let ready = connectors::require(state, id).await;
        calendars.push(CalendarSource {
            connector: id,
            provider: id.provider(),
            name: id.name().into(),
            account_email: account.and_then(|a| a.email),
            ready: ready.is_ok(),
        });
        if let Err(error) = ready {
            problems.push(error.to_string());
            continue;
        }
        let listed = match sync::calendar_client(state, id).await {
            Ok(client) => client.list_events(start, end).await,
            Err(error) => Err(error),
        };
        match listed {
            Ok(list) => events.extend(list.into_iter().map(|e| (id.provider(), e))),
            Err(error) => problems.push(format!(
                "{} could not be read: {}",
                id.name(),
                error.to_string().chars().take(160).collect::<String>()
            )),
        }
    }
    let interviews = state.db.call(|c| {
        repo::interviews_overlapping(c, start, end)?
            .into_iter()
            .map(|i| {
                let app = repo::get_application(c, i.application_id)?;
                Ok((i, app))
            })
            .collect::<AppResult<Vec<_>>>()
    })?;
    Ok(CalendarView {
        calendars,
        entries: merge(events, &interviews),
        problems,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::jobs::ApplicationStatus;

    const HOUR: i64 = 3_600_000;

    fn event(id: &str, title: &str, start: i64) -> CalendarEvent {
        CalendarEvent {
            id: id.into(),
            title: title.into(),
            start_at: Some(start),
            end_at: Some(start + HOUR),
            all_day: false,
            transparent: false,
            interview_id: None,
            location: None,
            meeting_url: None,
            web_link: Some(format!("https://calendar.example/{id}")),
        }
    }

    fn interview(
        id: i64,
        start: i64,
        provider: Option<ProviderId>,
        event: Option<&str>,
    ) -> InterviewRecord {
        let mut record = InterviewRecord::blank(id, ProviderId::Google, "t", "m", 0);
        record.id = id;
        record.state = InterviewState::Confirmed;
        record.start_at = Some(start);
        record.end_at = Some(start + HOUR);
        record.meeting_url = Some("https://meet.google.com/abc-defg-hij".into());
        record.calendar_provider = provider;
        record.calendar_event_id = event.map(str::to_string);
        record.calendar_state = if event.is_some() {
            CalendarState::Created
        } else {
            CalendarState::Conflict
        };
        record
    }

    fn app(id: i64, company: &str, role: &str) -> ApplicationRecord {
        let mut app = ApplicationRecord::new(company, ApplicationStatus::UpcomingInterview, 0);
        app.id = id;
        app.role = Some(role.into());
        app
    }

    #[test]
    fn shows_both_calendars_and_marks_interviews_once() {
        let t = 100 * HOUR;
        let mut rema = event(
            "g-2",
            "Interview — Anthropic — Forward Deployed Engineer",
            t + 2 * HOUR,
        );
        rema.interview_id = Some(1);
        // The same interview also landed in Outlook (e.g. an older event).
        let mut copy = event(
            "o-9",
            "Interview — Anthropic — Forward Deployed Engineer",
            t + 2 * HOUR,
        );
        copy.interview_id = Some(1);
        let entries = merge(
            vec![
                (ProviderId::Google, event("g-1", "Standup", t)),
                (ProviderId::Google, rema),
                (ProviderId::Microsoft, event("o-1", "Dentist", t + HOUR)),
                (ProviderId::Microsoft, copy),
            ],
            &[
                (
                    interview(1, t + 2 * HOUR, Some(ProviderId::Google), Some("g-2")),
                    app(10, "Anthropic", "Forward Deployed Engineer"),
                ),
                // Confirmed, but it conflicted and was not added.
                (
                    interview(2, t + 5 * HOUR, None, None),
                    app(11, "Globex", "Data Engineer"),
                ),
            ],
        );
        let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Standup",
                "Dentist",
                "Interview — Anthropic — Forward Deployed Engineer",
                "Interview — Globex — Data Engineer"
            ]
        );
        assert_eq!(entries[1].provider, Some(ProviderId::Microsoft));
        let anthropic = entries[2].interview.as_ref().unwrap();
        assert_eq!(
            (anthropic.company.as_str(), anthropic.in_calendar),
            ("Anthropic", true)
        );
        assert_eq!(
            entries[2].provider,
            Some(ProviderId::Google),
            "the calendar ReMa wrote to"
        );
        assert_eq!(
            entries[2].meeting_url.as_deref(),
            Some("https://meet.google.com/abc-defg-hij"),
            "the interview's meeting link"
        );
        let globex = &entries[3];
        assert_eq!(globex.provider, None);
        let link = globex.interview.as_ref().unwrap();
        assert!(!link.in_calendar && link.conflict);
    }
}
