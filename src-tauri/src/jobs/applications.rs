//! Turning validated email facts into application state.
//!
//! Matching an email to an existing application is deterministic: the
//! Gmail thread, then a reference number, then the normalized company and
//! role, then the company's own email domain. The model's suggested match
//! is accepted only if the company agrees. Only the newest email decides an
//! application's status.

use rusqlite::Connection;

use super::{
    extract::{ClaimState, Extraction},
    interviews::{InterviewCheck, VerifiedInterview},
};
use crate::{
    db::jobs::{self as repo, ApplicationRecord, InterviewRecord, InterviewState},
    error::AppResult,
    integrations::google::gmail::MessageMeta,
    models::jobs::ApplicationStatus,
};

const LEGAL_SUFFIXES: &[&str] = &[
    "inc",
    "incorporated",
    "ltd",
    "limited",
    "llc",
    "llp",
    "plc",
    "gmbh",
    "ag",
    "kg",
    "se",
    "sa",
    "sas",
    "bv",
    "nv",
    "srl",
    "sarl",
    "oy",
    "ab",
    "as",
    "aps",
    "corp",
    "corporation",
    "co",
    "company",
    "group",
    "holding",
    "holdings",
    "mbh",
    "ug",
    "og",
    "e",
    "u",
];

/// "ACME GmbH & Co. KG" → "acme".
pub fn company_key(name: &str) -> String {
    let words: Vec<String> = name
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !LEGAL_SUFFIXES.contains(w))
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        name.trim().to_lowercase()
    } else {
        words.join(" ")
    }
}

/// "Senior AI Engineer (m/w/d)" → "senior ai engineer".
pub fn role_key(role: &str) -> String {
    role.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1 && !matches!(*w, "mwd" | "fmd" | "fmx" | "wmd" | "all" | "genders"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Normalized reference numbers ("REQ-12 345" → "req12345").
pub fn reference_key(reference: &str) -> Option<String> {
    let key: String = reference
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_lowercase();
    (key.len() >= 3).then_some(key)
}

/// Mail providers and applicant-tracking systems send for many companies,
/// so their domains say nothing about which application an email is for.
pub fn is_shared_domain(domain: &str) -> bool {
    const SHARED: &[&str] = &[
        "gmail.com",
        "googlemail.com",
        "outlook.com",
        "hotmail.com",
        "live.com",
        "yahoo.com",
        "icloud.com",
        "me.com",
        "gmx.at",
        "gmx.de",
        "gmx.net",
        "web.de",
        "proton.me",
        "protonmail.com",
        "aol.com",
        "greenhouse.io",
        "greenhouse-mail.io",
        "lever.co",
        "workday.com",
        "myworkday.com",
        "smartrecruiters.com",
        "recruitee.com",
        "personio.de",
        "personio.com",
        "successfactors.com",
        "successfactors.eu",
        "icims.com",
        "workable.com",
        "workablemail.com",
        "bamboohr.com",
        "teamtailor.com",
        "teamtailor-mail.com",
        "join.com",
        "ashbyhq.com",
        "linkedin.com",
        "indeed.com",
        "xing.com",
        "stepstone.de",
        "stepstone.at",
        "karriere.at",
        "softgarden.io",
        "softgarden.de",
        "jobvite.com",
        "taleo.net",
    ];
    SHARED
        .iter()
        .any(|shared| domain == *shared || domain.ends_with(&format!(".{shared}")))
}

/// Recent applications, offered to the model to help it match.
pub fn known_applications(
    conn: &Connection,
    limit: usize,
) -> AppResult<Vec<super::extract::KnownApplication>> {
    let apps = repo::overview_applications(conn, 0)?;
    Ok(apps
        .into_iter()
        .take(limit)
        .map(|a| super::extract::KnownApplication {
            id: a.id,
            company: a.company,
            role: a.role,
            reference: a.reference,
        })
        .collect())
}

fn roles_compatible(a: &ApplicationRecord, role_key: Option<&str>) -> bool {
    match (a.role_key.as_deref(), role_key) {
        (Some(existing), Some(new)) => existing == new,
        _ => true, // one side unknown: same company is enough
    }
}

/// Finds the application an email belongs to.
fn find_application(
    conn: &Connection,
    meta: &MessageMeta,
    extraction: &Extraction,
) -> AppResult<Option<ApplicationRecord>> {
    // 1. Same Gmail thread.
    if let Some(id) = repo::application_for_thread(conn, &meta.thread_id)? {
        return Ok(Some(repo::get_application(conn, id)?));
    }
    let company = company_key(&extraction.company);
    let role = extraction.role.as_deref().map(role_key);

    // 2. The model's suggestion, if the company agrees.
    if let Some(id) = extraction.existing_application_id {
        if let Ok(app) = repo::get_application(conn, id) {
            if app.company_key == company {
                return Ok(Some(app));
            }
        }
    }
    // 3. Same reference number.
    if let Some(reference) = extraction.reference.as_deref().and_then(reference_key) {
        if let Some(app) = repo::applications_by_reference(conn, &reference)?
            .into_iter()
            .next()
        {
            return Ok(Some(app));
        }
    }
    // 4. Same company and compatible role.
    let same_company = repo::applications_by_company(conn, &company)?;
    if let Some(app) = same_company
        .iter()
        .find(|a| role.is_some() && a.role_key == role)
        .or_else(|| {
            same_company
                .iter()
                .find(|a| roles_compatible(a, role.as_deref()))
        })
    {
        return Ok(Some(app.clone()));
    }
    // 5. The company's own email domain and compatible role.
    if let Some(domain) = meta.sender_domain().filter(|d| !is_shared_domain(d)) {
        if let Some(app) = repo::applications_by_domain(conn, &domain)?
            .into_iter()
            .find(|a| roles_compatible(a, role.as_deref()))
        {
            return Ok(Some(app));
        }
    }
    Ok(None)
}

/// What applying an email changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    pub application_id: i64,
    pub created: bool,
    /// The application became rejected with this email.
    pub rejected_now: bool,
}

/// Status, action and next step after deterministic interview rules.
fn effective_status(
    extraction: &Extraction,
    check: Option<&InterviewCheck>,
) -> (ApplicationStatus, bool, Option<String>) {
    let mut status = extraction.status;
    let mut requires_action = extraction.requires_action;
    let mut next_action = extraction.next_action.clone();
    let claim_state = extraction.interview.as_ref().map(|i| i.state);
    match (claim_state, check) {
        (Some(ClaimState::Cancelled), _) => {
            if status == ApplicationStatus::UpcomingInterview {
                status = ApplicationStatus::InProcess;
            }
        }
        (Some(ClaimState::Proposed), _) => {
            status = ApplicationStatus::NeedsAction;
            requires_action = true;
            next_action = next_action.or_else(|| Some("Reply to confirm an interview time".into()));
        }
        (_, Some(InterviewCheck::Upcoming(_))) => {
            status = ApplicationStatus::UpcomingInterview;
            requires_action = false;
            next_action = None;
        }
        (_, Some(InterviewCheck::NeedsReview(reason))) => {
            status = ApplicationStatus::NeedsAction;
            requires_action = true;
            next_action = Some(format!("Check the interview details: {reason}"));
        }
        (_, Some(InterviewCheck::Past(_))) => {
            if status == ApplicationStatus::UpcomingInterview {
                status = ApplicationStatus::InProcess;
            }
        }
        _ => {}
    }
    if status == ApplicationStatus::NeedsAction {
        requires_action = true;
        next_action = next_action.or_else(|| Some("Check the latest email".into()));
    }
    (status, requires_action, next_action)
}

/// Stores one validated email: application, history, thread link, interview.
pub fn apply(
    conn: &Connection,
    meta: &MessageMeta,
    extraction: &Extraction,
    check: Option<&InterviewCheck>,
    now: i64,
) -> AppResult<Applied> {
    let (status, requires_action, next_action) = effective_status(extraction, check);
    let domain = meta.sender_domain().filter(|d| !is_shared_domain(d));
    let reference = extraction.reference.as_deref().and_then(reference_key);

    let (application_id, created, rejected_now) = match find_application(conn, meta, extraction)? {
        Some(mut app) => {
            let was_rejected = app.status == ApplicationStatus::Rejected;
            // Fill in details the application did not have yet.
            if app.role.is_none() && extraction.role.is_some() {
                app.role = extraction.role.clone();
                app.role_key = extraction.role.as_deref().map(role_key);
            }
            app.reference = app.reference.or(reference);
            app.sender_domain = app.sender_domain.or(domain);
            // Only the newest email decides the status.
            let newest = meta.received_at >= app.last_update_at;
            if newest {
                app.status = status;
                app.requires_action = requires_action;
                app.next_action = next_action;
                app.last_update_at = meta.received_at;
            }
            app.updated_at = now;
            repo::save_application(conn, &app)?;
            let rejected_now = newest && !was_rejected && status == ApplicationStatus::Rejected;
            (app.id, false, rejected_now)
        }
        None => {
            let id = repo::insert_application(
                conn,
                &ApplicationRecord {
                    id: 0,
                    company: extraction.company.clone(),
                    company_key: company_key(&extraction.company),
                    role: extraction.role.clone(),
                    role_key: extraction.role.as_deref().map(role_key),
                    reference,
                    sender_domain: domain,
                    status,
                    requires_action,
                    next_action,
                    last_update_at: meta.received_at,
                    created_at: now,
                    updated_at: now,
                },
            )?;
            (id, true, status == ApplicationStatus::Rejected)
        }
    };

    repo::link_thread(conn, &meta.thread_id, application_id)?;
    repo::record_update(
        conn,
        application_id,
        &meta.id,
        status,
        extraction.summary.as_deref(),
        meta.received_at,
        now,
    )?;
    if let Some(claim) = &extraction.interview {
        apply_interview(conn, application_id, meta, claim.state, check, now)?;
    }
    Ok(Applied {
        application_id,
        created,
        rejected_now,
    })
}

fn active(i: &InterviewRecord) -> bool {
    i.state != InterviewState::Cancelled
}

/// The interview an update refers to: same thread first, then the latest.
fn target_interview(interviews: &[InterviewRecord], thread_id: &str) -> Option<InterviewRecord> {
    interviews
        .iter()
        .rev()
        .filter(|i| active(i))
        .find(|i| i.source_thread_id == thread_id)
        .or_else(|| interviews.iter().rev().find(|i| active(i)))
        .cloned()
}

fn set_verified(interview: &mut InterviewRecord, v: &VerifiedInterview) {
    interview.start_at = Some(v.start_at);
    interview.end_at = Some(v.end_at);
    interview.timezone = Some(v.timezone.clone());
    interview.interview_type = v.interview_type.clone();
    interview.location = v.location.clone();
    interview.meeting_url = v.meeting_url.clone();
    interview.participants = v.participants.clone();
    interview.review_reason = None;
}

/// Creates or updates the interview record for an email. Re-processing the
/// same email never creates a second interview.
fn apply_interview(
    conn: &Connection,
    application_id: i64,
    meta: &MessageMeta,
    claim_state: ClaimState,
    check: Option<&InterviewCheck>,
    now: i64,
) -> AppResult<()> {
    let existing = repo::interviews_for_application(conn, application_id)?;
    let from_this_email = existing
        .iter()
        .find(|i| i.source_message_id == meta.id)
        .cloned();
    let blank = || InterviewRecord {
        id: 0,
        application_id,
        source_thread_id: meta.thread_id.clone(),
        source_message_id: meta.id.clone(),
        state: InterviewState::Proposed,
        interview_type: None,
        start_at: None,
        end_at: None,
        timezone: None,
        location: None,
        meeting_url: None,
        participants: Vec::new(),
        review_reason: None,
        calendar_event_id: None,
        calendar_hash: None,
        calendar_synced_at: None,
        created_at: now,
        updated_at: now,
    };
    let history = |id: i64, change: &str, details: Option<&str>| {
        repo::add_interview_history(conn, id, change, details, Some(&meta.id), now)
    };

    match (claim_state, check) {
        (ClaimState::Cancelled, _) => {
            if let Some(mut target) = target_interview(&existing, &meta.thread_id) {
                target.state = InterviewState::Cancelled;
                target.updated_at = now;
                repo::save_interview(conn, &target)?;
                history(target.id, "cancelled", None)?;
            }
        }
        (_, Some(InterviewCheck::Upcoming(v) | InterviewCheck::Past(v))) => {
            // Same email seen again: nothing new.
            if from_this_email
                .as_ref()
                .is_some_and(|i| i.start_at == Some(v.start_at))
            {
                return Ok(());
            }
            let same_time = existing
                .iter()
                .find(|i| {
                    active(i)
                        && i.state == InterviewState::Confirmed
                        && i.start_at == Some(v.start_at)
                })
                .cloned();
            if let Some(mut same) = same_time {
                // A repeated confirmation of a known interview: refresh details only.
                set_verified(&mut same, v);
                same.updated_at = now;
                return repo::save_interview(conn, &same);
            }
            // A reschedule, or a confirmation of an interview we knew as
            // proposed / needing review, updates that interview.
            let reuse = match claim_state {
                ClaimState::Rescheduled => target_interview(&existing, &meta.thread_id),
                _ => existing
                    .iter()
                    .rev()
                    .find(|i| {
                        i.source_thread_id == meta.thread_id
                            && matches!(
                                i.state,
                                InterviewState::Proposed | InterviewState::NeedsReview
                            )
                    })
                    .cloned(),
            };
            match reuse {
                Some(mut interview) => {
                    let before = interview.clone();
                    let previous = before.start_at;
                    set_verified(&mut interview, v);
                    // Logistics a follow-up email does not repeat stay as
                    // verified from the earlier email.
                    interview.interview_type = interview.interview_type.or(before.interview_type);
                    interview.location = interview.location.or(before.location);
                    interview.meeting_url = interview.meeting_url.or(before.meeting_url);
                    if interview.participants.is_empty() {
                        interview.participants = before.participants;
                    }
                    interview.state = InterviewState::Confirmed;
                    interview.source_message_id = meta.id.clone();
                    interview.source_thread_id = meta.thread_id.clone();
                    interview.updated_at = now;
                    repo::save_interview(conn, &interview)?;
                    let change = if previous.is_some() && previous != Some(v.start_at) {
                        "rescheduled"
                    } else {
                        "confirmed"
                    };
                    let details = previous.map(|p| format!("previous start {p}"));
                    history(interview.id, change, details.as_deref())?;
                }
                None => {
                    let mut interview = blank();
                    set_verified(&mut interview, v);
                    interview.state = InterviewState::Confirmed;
                    let id = repo::insert_interview(conn, &interview)?;
                    history(id, "confirmed", None)?;
                }
            }
        }
        (_, Some(InterviewCheck::NeedsReview(reason))) => {
            let state = if claim_state == ClaimState::Proposed {
                InterviewState::Proposed
            } else {
                InterviewState::NeedsReview
            };
            match from_this_email {
                Some(mut interview) => {
                    interview.state = state;
                    interview.review_reason = Some(reason.clone());
                    interview.updated_at = now;
                    repo::save_interview(conn, &interview)?;
                }
                None => {
                    let mut interview = blank();
                    interview.state = state;
                    interview.review_reason = Some(reason.clone());
                    let id = repo::insert_interview(conn, &interview)?;
                    history(id, state.as_str(), Some(reason))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Applications marked "upcoming interview" whose interviews are all over
/// go back to "in process".
pub fn settle_past_interviews(conn: &Connection, now: i64) -> AppResult<()> {
    for mut app in repo::applications_with_status(conn, ApplicationStatus::UpcomingInterview)? {
        let upcoming = repo::interviews_for_application(conn, app.id)?
            .iter()
            .any(|i| i.state == InterviewState::Confirmed && i.end_at.is_some_and(|end| end > now));
        if !upcoming {
            app.status = ApplicationStatus::InProcess;
            app.next_action = None;
            app.requires_action = false;
            app.updated_at = now;
            repo::save_application(conn, &app)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::Database, jobs::extract::InterviewClaim};

    fn meta(id: &str, thread: &str, from: &str, received_at: i64) -> MessageMeta {
        MessageMeta {
            id: id.into(),
            thread_id: thread.into(),
            received_at,
            from: from.into(),
            subject: String::new(),
            snippet: String::new(),
        }
    }

    fn extraction(company: &str, role: Option<&str>, status: ApplicationStatus) -> Extraction {
        Extraction {
            company: company.into(),
            role: role.map(str::to_string),
            reference: None,
            status,
            requires_action: false,
            next_action: None,
            summary: None,
            existing_application_id: None,
            interview: None,
        }
    }

    fn verified(start_at: i64) -> VerifiedInterview {
        VerifiedInterview {
            start_at,
            end_at: start_at + 3_600_000,
            timezone: "UTC".into(),
            interview_type: None,
            location: None,
            meeting_url: None,
            participants: vec![],
            notes: vec![],
        }
    }

    fn claim(state: ClaimState) -> InterviewClaim {
        InterviewClaim {
            state,
            date: None,
            start_time: None,
            end_time: None,
            duration_minutes: None,
            timezone: None,
            datetime_quote: None,
            timezone_quote: None,
            interview_type: None,
            location: None,
            meeting_url: None,
            participants: vec![],
            interviewer: None,
            unclear: None,
        }
    }

    #[test]
    fn normalizes_names() {
        assert_eq!(company_key("ACME GmbH & Co. KG"), "acme");
        assert_eq!(company_key("Acme"), "acme");
        assert_eq!(role_key("Senior AI Engineer (m/w/d)"), "senior ai engineer");
        assert_eq!(reference_key("REQ-12 345").as_deref(), Some("req12345"));
        assert!(is_shared_domain("boards.greenhouse.io"));
        assert!(!is_shared_domain("acme.io"));
    }

    #[test]
    fn groups_emails_into_one_application() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            use ApplicationStatus::*;
            let first = apply(
                c,
                &meta("m1", "t1", "jobs@acme.io", 10),
                &extraction("Acme GmbH", Some("AI Engineer"), Confirmed),
                None,
                1,
            )?;
            assert!(first.created);
            // Same thread.
            let same_thread = apply(
                c,
                &meta("m2", "t1", "x@other.com", 20),
                &extraction("ACME", None, InProcess),
                None,
                2,
            )?;
            // Other thread, same company + role.
            let same_role = apply(
                c,
                &meta("m3", "t2", "hr@acme.io", 30),
                &extraction("Acme", Some("AI Engineer (m/w/d)"), InProcess),
                None,
                3,
            )?;
            // Other thread, same company domain, no role.
            let same_domain = apply(
                c,
                &meta("m4", "t3", "talent@acme.io", 40),
                &extraction("Acme Labs", None, InProcess),
                None,
                4,
            )?;
            for applied in [same_thread, same_role, same_domain] {
                assert_eq!(applied.application_id, first.application_id);
                assert!(!applied.created);
            }
            // Different role at the same company: a separate application.
            let other_role = apply(
                c,
                &meta("m5", "t4", "jobs@acme.io", 50),
                &extraction("Acme", Some("Data Scientist"), Confirmed),
                None,
                5,
            )?;
            assert!(other_role.created);
            // Shared ATS domain never links different companies.
            let ats_a = apply(
                c,
                &meta("m6", "t5", "no-reply@greenhouse.io", 60),
                &extraction("Beta", Some("ML Engineer"), Confirmed),
                None,
                6,
            )?;
            let ats_b = apply(
                c,
                &meta("m7", "t6", "no-reply@greenhouse.io", 70),
                &extraction("Gamma", Some("ML Engineer"), Confirmed),
                None,
                7,
            )?;
            assert_ne!(ats_a.application_id, ats_b.application_id);
            assert_eq!(repo::count_rows(c, "job_applications")?, 4);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn only_the_newest_email_sets_the_status() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            use ApplicationStatus::*;
            let rejected = apply(
                c,
                &meta("m2", "t1", "a@acme.io", 200),
                &extraction("Acme", Some("AI"), Rejected),
                None,
                1,
            )?;
            assert!(rejected.rejected_now);
            // An older email processed later does not undo the rejection.
            apply(
                c,
                &meta("m1", "t1", "a@acme.io", 100),
                &extraction("Acme", Some("AI"), InProcess),
                None,
                2,
            )?;
            let app = repo::get_application(c, rejected.application_id)?;
            assert_eq!(app.status, Rejected);
            assert_eq!(app.last_update_at, 200);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn reschedules_update_the_same_interview_and_cancellations_keep_history() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            use ApplicationStatus::*;
            let mut e = extraction("Acme", Some("AI Engineer"), UpcomingInterview);
            e.interview = Some(claim(ClaimState::Confirmed));
            let applied = apply(
                c,
                &meta("m1", "t1", "a@acme.io", 10),
                &e,
                Some(&InterviewCheck::Upcoming(verified(1_000_000))),
                1,
            )?;
            let app_id = applied.application_id;
            assert_eq!(repo::get_application(c, app_id)?.status, UpcomingInterview);

            // Processing the same email again changes nothing.
            apply(
                c,
                &meta("m1", "t1", "a@acme.io", 10),
                &e,
                Some(&InterviewCheck::Upcoming(verified(1_000_000))),
                2,
            )?;
            assert_eq!(repo::interviews_for_application(c, app_id)?.len(), 1);

            let mut moved = e.clone();
            moved.interview = Some(claim(ClaimState::Rescheduled));
            apply(
                c,
                &meta("m2", "t1", "a@acme.io", 20),
                &moved,
                Some(&InterviewCheck::Upcoming(verified(2_000_000))),
                3,
            )?;
            let interviews = repo::interviews_for_application(c, app_id)?;
            assert_eq!(interviews.len(), 1, "a reschedule never duplicates");
            assert_eq!(interviews[0].start_at, Some(2_000_000));
            assert_eq!(interviews[0].source_message_id, "m2");

            let mut cancelled = extraction("Acme", Some("AI Engineer"), InProcess);
            cancelled.interview = Some(claim(ClaimState::Cancelled));
            apply(c, &meta("m3", "t1", "a@acme.io", 30), &cancelled, None, 4)?;
            let interview = &repo::interviews_for_application(c, app_id)?[0];
            assert_eq!(interview.state, InterviewState::Cancelled);
            let history: Vec<String> = repo::interview_history(c, interview.id)?
                .into_iter()
                .map(|(c, _)| c)
                .collect();
            assert_eq!(history, ["confirmed", "rescheduled", "cancelled"]);
            assert_eq!(repo::get_application(c, app_id)?.status, InProcess);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn unclear_interviews_need_the_users_action() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let mut e = extraction("Acme", Some("AI"), ApplicationStatus::UpcomingInterview);
            e.interview = Some(claim(ClaimState::Confirmed));
            let check = InterviewCheck::NeedsReview(
                "the email does not state the interview time zone".into(),
            );
            let applied = apply(c, &meta("m1", "t1", "a@acme.io", 10), &e, Some(&check), 1)?;
            let app = repo::get_application(c, applied.application_id)?;
            assert_eq!(app.status, ApplicationStatus::NeedsAction);
            assert!(app.next_action.unwrap().contains("time zone"));
            let interview = &repo::interviews_for_application(c, app.id)?[0];
            assert_eq!(interview.state, InterviewState::NeedsReview);
            assert!(interview.calendar_event_id.is_none());
            Ok(())
        })
        .unwrap();
    }
}
