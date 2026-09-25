//! The application overview and the run summary, built from stored state.

use rusqlite::Connection;

use crate::{
    db::jobs::{self as repo, InterviewState},
    error::AppResult,
    models::jobs::{ApplicationRow, ApplicationStatus, JobRunReport},
};

/// Applications updated in the window, plus any still needing attention.
pub fn overview(conn: &Connection, window_start: i64, now: i64) -> AppResult<Vec<ApplicationRow>> {
    let apps = repo::overview_applications(conn, window_start)?;
    let mut rows = Vec::with_capacity(apps.len());
    for app in apps {
        let next_interview = repo::interviews_for_application(conn, app.id)?
            .into_iter()
            .filter(|i| {
                i.state == InterviewState::Confirmed && i.end_at.is_some_and(|end| end > now)
            })
            .min_by_key(|i| i.start_at);
        let upcoming = app.status == ApplicationStatus::UpcomingInterview;
        rows.push(ApplicationRow {
            id: app.id,
            company: app.company,
            role: app.role,
            status: app.status,
            last_update_at: app.last_update_at,
            next_action: if upcoming { None } else { app.next_action },
            interview_at: next_interview
                .as_ref()
                .filter(|_| upcoming)
                .and_then(|i| i.start_at),
            interview_timezone: next_interview.filter(|_| upcoming).and_then(|i| i.timezone),
        });
    }
    Ok(rows)
}

/// Fills the headline counts from the overview rows.
pub fn count(report: &mut JobRunReport) {
    report.upcoming_interviews = report
        .applications
        .iter()
        .filter(|a| a.status == ApplicationStatus::UpcomingInterview)
        .count() as u32;
    report.needs_action = report
        .applications
        .iter()
        .filter(|a| a.status == ApplicationStatus::NeedsAction)
        .count() as u32;
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A compact plain-text summary for history lists and notifications.
pub fn summary(report: &JobRunReport) -> String {
    let mut lines = vec![
        "Job Application Update".to_string(),
        plural(
            report.applications_updated,
            "application updated",
            "applications updated",
        ),
        plural(
            report.relevant_emails,
            "new relevant email",
            "new relevant emails",
        ),
        plural(
            report.upcoming_interviews,
            "upcoming interview",
            "upcoming interviews",
        ),
        plural(
            report.needs_action,
            "application needs action",
            "applications need action",
        ),
        plural(report.new_rejections, "new rejection", "new rejections"),
    ];
    if let Some(calendar) = &report.calendar {
        lines.push(plural(
            calendar.conflicts,
            "calendar conflict",
            "calendar conflicts",
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::jobs::CalendarReport;

    #[test]
    fn summarizes_counts() {
        let report = JobRunReport {
            applications_updated: 5,
            relevant_emails: 2,
            upcoming_interviews: 1,
            needs_action: 1,
            new_rejections: 0,
            calendar: Some(CalendarReport {
                conflicts: 1,
                ..CalendarReport::default()
            }),
            ..JobRunReport::default()
        };
        assert_eq!(
            summary(&report),
            "Job Application Update\n5 applications updated\n2 new relevant emails\n1 upcoming interview\n\
             1 application needs action\n0 new rejections\n1 calendar conflict"
        );
    }
}
