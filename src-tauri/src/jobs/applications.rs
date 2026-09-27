//! Turning validated email facts into application state.
//!
//! Matching an email to an existing application is deterministic: the mail
//! thread, then a reference number, then the normalized company and role,
//! then the company's own email domain. The model's suggested match is
//! accepted only if the company agrees. Only the newest email decides an
//! application's status. Every change leaves a timeline entry with its
//! source and the classifier's confidence.

use rusqlite::Connection;

use super::{
    extract::{ClaimState, Extraction},
    interviews::{InterviewCheck, VerifiedInterview},
};
use crate::{
    connectors::mail::MailMessage,
    db::jobs::{self as repo, ApplicationRecord, InterviewRecord, InterviewState, TimelineRecord},
    error::AppResult,
    models::{
        connectors::ProviderId,
        jobs::{ApplicationStatus, UpdateSource},
    },
};

/// The timeline source for mail from a provider.
pub fn mail_source(provider: ProviderId) -> UpdateSource {
    match provider {
        ProviderId::Google => UpdateSource::Gmail,
        ProviderId::Microsoft => UpdateSource::Outlook,
    }
}

/// "Application changed to Interview", or what the email was about.
pub fn change_text(
    previous: Option<ApplicationStatus>,
    status: ApplicationStatus,
    category_label: &str,
) -> String {
    match previous {
        Some(previous) if previous == status => category_label.to_string(),
        _ => format!("Application changed to {}", status.label()),
    }
}

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
pub fn find_application(
    conn: &Connection,
    meta: &MailMessage,
    extraction: &Extraction,
) -> AppResult<Option<ApplicationRecord>> {
    // 1. Same mail thread / conversation.
    if let Some(id) = repo::application_for_thread(conn, meta.provider(), &meta.thread_id)? {
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
    pub previous_status: Option<ApplicationStatus>,
    pub status: ApplicationStatus,
    /// This email decided the status (it is the newest).
    pub status_changed: bool,
    /// The interview this email created or updated.
    pub interview_id: Option<i64>,
}

/// Status, action and next step after deterministic interview rules.
fn effective_status(
    status: ApplicationStatus,
    extraction: &Extraction,
    check: Option<&InterviewCheck>,
) -> (ApplicationStatus, bool, Option<String>) {
    let mut status = status;
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

/// Stores one validated email: application, timeline, thread link and
/// interview. An email that implies no status (other job-related mail) only
/// adds to an application it clearly belongs to; `Ok(None)` otherwise.
pub fn apply(
    conn: &Connection,
    meta: &MailMessage,
    extraction: &Extraction,
    check: Option<&InterviewCheck>,
    now: i64,
) -> AppResult<Option<Applied>> {
    let provider = meta.provider();
    let domain = meta.sender_domain().filter(|d| !is_shared_domain(d));
    let reference = extraction.reference.as_deref().and_then(reference_key);
    let existing = find_application(conn, meta, extraction)?;

    let Some(implied) = extraction.status else {
        // Timeline only, never a new application.
        let Some(app) = existing else {
            return Ok(None);
        };
        repo::link_thread(conn, provider, &meta.thread_id, app.id)?;
        repo::add_timeline(
            conn,
            &TimelineRecord {
                application_id: app.id,
                source: mail_source(provider),
                provider: Some(provider),
                message_id: Some(&meta.message_id),
                category: Some(extraction.category),
                confidence: Some(extraction.confidence),
                status: app.status,
                previous_status: Some(app.status),
                change: extraction.category.label(),
                summary: extraction.summary.as_deref(),
                occurred_at: meta.received_at,
                created_at: now,
            },
        )?;
        return Ok(Some(Applied {
            application_id: app.id,
            created: false,
            rejected_now: false,
            previous_status: Some(app.status),
            status: app.status,
            status_changed: false,
            interview_id: None,
        }));
    };
    let (status, requires_action, next_action) = effective_status(implied, extraction, check);

    let (application_id, created, previous, current, newest) = match existing {
        Some(mut app) => {
            let previous = app.status;
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
            (app.id, false, Some(previous), app.status, newest)
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
            (id, true, None, status, true)
        }
    };

    repo::link_thread(conn, provider, &meta.thread_id, application_id)?;
    let change = if created {
        format!("Application added: {}", status.label())
    } else if newest {
        change_text(previous, status, extraction.category.label())
    } else {
        format!(
            "{} (older email; status unchanged)",
            extraction.category.label()
        )
    };
    repo::add_timeline(
        conn,
        &TimelineRecord {
            application_id,
            source: mail_source(provider),
            provider: Some(provider),
            message_id: Some(&meta.message_id),
            category: Some(extraction.category),
            confidence: Some(extraction.confidence),
            status: current,
            previous_status: previous,
            change: &change,
            summary: extraction.summary.as_deref(),
            occurred_at: meta.received_at,
            created_at: now,
        },
    )?;
    let interview_id = match &extraction.interview {
        Some(claim) => apply_interview(
            conn,
            application_id,
            meta,
            claim.state,
            check,
            extraction.confidence,
            now,
        )?,
        None => None,
    };
    Ok(Some(Applied {
        application_id,
        created,
        rejected_now: newest
            && previous != Some(ApplicationStatus::Rejected)
            && current == ApplicationStatus::Rejected,
        previous_status: previous,
        status: current,
        status_changed: newest && previous != Some(current),
        interview_id,
    }))
}

/// Identifies an interview: company, role, start and conversation.
pub fn fingerprint(app: &ApplicationRecord, start_at: i64, thread_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let text = format!(
        "{}|{}|{start_at}|{thread_id}",
        app.company_key,
        app.role_key.as_deref().unwrap_or("")
    );
    Sha256::digest(text.as_bytes())
        .iter()
        .take(12)
        .map(|b| format!("{b:02x}"))
        .collect()
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
/// same email never creates a second interview. Returns the interview.
fn apply_interview(
    conn: &Connection,
    application_id: i64,
    meta: &MailMessage,
    claim_state: ClaimState,
    check: Option<&InterviewCheck>,
    confidence: f64,
    now: i64,
) -> AppResult<Option<i64>> {
    let provider = meta.provider();
    let existing = repo::interviews_for_application(conn, application_id)?;
    let from_this_email = existing
        .iter()
        .find(|i| i.source_message_id == meta.message_id && i.provider == provider)
        .cloned();
    let blank = || {
        let mut record = InterviewRecord::blank(
            application_id,
            provider,
            &meta.thread_id,
            &meta.message_id,
            now,
        );
        record.confidence = Some(confidence);
        record
    };
    let history = |id: i64, change: &str, details: Option<&str>| {
        repo::add_interview_history(conn, id, change, details, Some(&meta.message_id), now)
    };
    let app = repo::get_application(conn, application_id)?;

    match (claim_state, check) {
        (ClaimState::Cancelled, _) => {
            if let Some(mut target) = target_interview(&existing, &meta.thread_id) {
                target.state = InterviewState::Cancelled;
                target.updated_at = now;
                repo::save_interview(conn, &target)?;
                history(target.id, "cancelled", None)?;
                return Ok(Some(target.id));
            }
            Ok(None)
        }
        (_, Some(InterviewCheck::Upcoming(v) | InterviewCheck::Past(v))) => {
            // Same email seen again: nothing new.
            if let Some(same) = from_this_email
                .as_ref()
                .filter(|i| i.start_at == Some(v.start_at))
            {
                return Ok(Some(same.id));
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
                repo::save_interview(conn, &same)?;
                return Ok(Some(same.id));
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
                    interview.provider = provider;
                    interview.source_message_id = meta.message_id.clone();
                    interview.source_thread_id = meta.thread_id.clone();
                    interview.confidence = Some(confidence);
                    interview.fingerprint = Some(fingerprint(&app, v.start_at, &meta.thread_id));
                    interview.updated_at = now;
                    repo::save_interview(conn, &interview)?;
                    let change = if previous.is_some() && previous != Some(v.start_at) {
                        "rescheduled"
                    } else {
                        "confirmed"
                    };
                    let details = previous.map(|p| format!("previous start {p}"));
                    history(interview.id, change, details.as_deref())?;
                    Ok(Some(interview.id))
                }
                None => {
                    let mut interview = blank();
                    set_verified(&mut interview, v);
                    interview.state = InterviewState::Confirmed;
                    interview.fingerprint = Some(fingerprint(&app, v.start_at, &meta.thread_id));
                    let id = repo::insert_interview(conn, &interview)?;
                    history(id, "confirmed", None)?;
                    Ok(Some(id))
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
                    Ok(Some(interview.id))
                }
                None => {
                    let mut interview = blank();
                    interview.state = state;
                    interview.review_reason = Some(reason.clone());
                    let id = repo::insert_interview(conn, &interview)?;
                    history(id, state.as_str(), Some(reason))?;
                    Ok(Some(id))
                }
            }
        }
        _ => Ok(None),
    }
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
    use crate::{db::Database, jobs::extract::InterviewClaim, models::jobs::EmailCategory};

    fn meta(id: &str, thread: &str, from: &str, received_at: i64) -> MailMessage {
        MailMessage {
            provider: Some(ProviderId::Google),
            message_id: id.into(),
            thread_id: thread.into(),
            conversation_id: thread.into(),
            received_at,
            sender: from.into(),
            ..MailMessage::default()
        }
    }

    fn category_for(status: ApplicationStatus) -> EmailCategory {
        match status {
            ApplicationStatus::Confirmed => EmailCategory::ApplicationReceived,
            ApplicationStatus::InProcess => EmailCategory::ApplicationUpdate,
            ApplicationStatus::NeedsAction => EmailCategory::ActionRequired,
            ApplicationStatus::UpcomingInterview => EmailCategory::InterviewConfirmed,
            ApplicationStatus::Rejected => EmailCategory::Rejection,
            ApplicationStatus::Offer => EmailCategory::Offer,
        }
    }

    fn extraction(company: &str, role: Option<&str>, status: ApplicationStatus) -> Extraction {
        Extraction {
            category: category_for(status),
            confidence: 0.9,
            company: company.into(),
            role: role.map(str::to_string),
            reference: None,
            stage: None,
            status: Some(status),
            requires_action: false,
            next_action: None,
            summary: None,
            existing_application_id: None,
            contacts: vec![],
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
            proposed_slots: vec![],
            unclear: None,
        }
    }

    fn applied(
        c: &Connection,
        meta: &MailMessage,
        e: &Extraction,
        check: Option<&InterviewCheck>,
        now: i64,
    ) -> Applied {
        apply(c, meta, e, check, now).unwrap().unwrap()
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
            let first = applied(
                c,
                &meta("m1", "t1", "jobs@acme.io", 10),
                &extraction("Acme GmbH", Some("AI Engineer"), Confirmed),
                None,
                1,
            );
            assert!(first.created);
            let same_thread = applied(
                c,
                &meta("m2", "t1", "x@other.com", 20),
                &extraction("ACME", None, InProcess),
                None,
                2,
            );
            let same_role = applied(
                c,
                &meta("m3", "t2", "hr@acme.io", 30),
                &extraction("Acme", Some("AI Engineer (m/w/d)"), InProcess),
                None,
                3,
            );
            let same_domain = applied(
                c,
                &meta("m4", "t3", "talent@acme.io", 40),
                &extraction("Acme Labs", None, InProcess),
                None,
                4,
            );
            for a in [same_thread, same_role, same_domain] {
                assert_eq!(a.application_id, first.application_id);
                assert!(!a.created);
            }
            let other_role = applied(
                c,
                &meta("m5", "t4", "jobs@acme.io", 50),
                &extraction("Acme", Some("Data Scientist"), Confirmed),
                None,
                5,
            );
            assert!(other_role.created);
            // Shared ATS domain never links different companies.
            let ats_a = applied(
                c,
                &meta("m6", "t5", "no-reply@greenhouse.io", 60),
                &extraction("Beta", Some("ML Engineer"), Confirmed),
                None,
                6,
            );
            let ats_b = applied(
                c,
                &meta("m7", "t6", "no-reply@greenhouse.io", 70),
                &extraction("Gamma", Some("ML Engineer"), Confirmed),
                None,
                7,
            );
            assert_ne!(ats_a.application_id, ats_b.application_id);
            assert_eq!(repo::count_rows(c, "job_applications")?, 4);
            // The same thread id at another provider is another thread.
            let mut outlook = meta("m8", "t1", "x@elsewhere.com", 80);
            outlook.provider = Some(ProviderId::Microsoft);
            let separate = applied(c, &outlook, &extraction("Delta", None, Confirmed), None, 8);
            assert_ne!(separate.application_id, first.application_id);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn every_change_leaves_an_audit_entry_with_source_and_confidence() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            use ApplicationStatus::*;
            let first = applied(
                c,
                &meta("m1", "t1", "jobs@acme.io", 10),
                &extraction("Acme", Some("AI Engineer"), Confirmed),
                None,
                1,
            );
            let mut reject = extraction("Acme", Some("AI Engineer"), Rejected);
            reject.confidence = 0.97;
            let second = applied(c, &meta("m2", "t1", "jobs@acme.io", 20), &reject, None, 2);
            assert!(second.status_changed && second.rejected_now);
            let timeline = repo::timeline(c, first.application_id)?;
            assert_eq!(timeline.len(), 2);
            assert_eq!(timeline[0].change, "Application changed to Rejected");
            assert_eq!(timeline[0].source, UpdateSource::Gmail);
            assert_eq!(timeline[0].confidence, Some(0.97));
            assert_eq!(timeline[0].previous_status, Some(Confirmed));
            assert_eq!(
                timeline[1].change,
                "Application added: Application received"
            );
            // The same email again adds nothing.
            applied(c, &meta("m2", "t1", "jobs@acme.io", 20), &reject, None, 3);
            assert_eq!(repo::timeline(c, first.application_id)?.len(), 2);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn job_mail_without_a_status_only_joins_a_known_application() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let mut other = extraction("Acme", None, ApplicationStatus::InProcess);
            other.category = EmailCategory::OtherJobRelated;
            other.status = None;
            assert!(apply(c, &meta("m1", "t1", "x@acme.io", 10), &other, None, 1)?.is_none());
            let app = applied(
                c,
                &meta("m2", "t2", "x@acme.io", 20),
                &extraction("Acme", None, ApplicationStatus::Confirmed),
                None,
                2,
            );
            let joined = applied(c, &meta("m3", "t2", "x@acme.io", 30), &other, None, 3);
            assert_eq!(joined.application_id, app.application_id);
            assert!(!joined.status_changed);
            assert_eq!(
                repo::get_application(c, app.application_id)?.status,
                ApplicationStatus::Confirmed
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn only_the_newest_email_sets_the_status() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            use ApplicationStatus::*;
            let rejected = applied(
                c,
                &meta("m2", "t1", "a@acme.io", 200),
                &extraction("Acme", Some("AI"), Rejected),
                None,
                1,
            );
            assert!(rejected.rejected_now);
            applied(
                c,
                &meta("m1", "t1", "a@acme.io", 100),
                &extraction("Acme", Some("AI"), InProcess),
                None,
                2,
            );
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
            let first = applied(
                c,
                &meta("m1", "t1", "a@acme.io", 10),
                &e,
                Some(&InterviewCheck::Upcoming(verified(1_000_000))),
                1,
            );
            let app_id = first.application_id;
            assert_eq!(repo::get_application(c, app_id)?.status, UpcomingInterview);
            let interview = &repo::interviews_for_application(c, app_id)?[0];
            assert!(interview.fingerprint.is_some());
            assert_eq!(interview.confidence, Some(0.9));

            applied(
                c,
                &meta("m1", "t1", "a@acme.io", 10),
                &e,
                Some(&InterviewCheck::Upcoming(verified(1_000_000))),
                2,
            );
            assert_eq!(repo::interviews_for_application(c, app_id)?.len(), 1);

            let mut moved = e.clone();
            moved.category = EmailCategory::InterviewRescheduled;
            moved.interview = Some(claim(ClaimState::Rescheduled));
            applied(
                c,
                &meta("m2", "t1", "a@acme.io", 20),
                &moved,
                Some(&InterviewCheck::Upcoming(verified(2_000_000))),
                3,
            );
            let interviews = repo::interviews_for_application(c, app_id)?;
            assert_eq!(interviews.len(), 1, "a reschedule never duplicates");
            assert_eq!(interviews[0].start_at, Some(2_000_000));
            assert_eq!(interviews[0].source_message_id, "m2");

            let mut cancelled = extraction("Acme", Some("AI Engineer"), InProcess);
            cancelled.category = EmailCategory::InterviewCancelled;
            cancelled.interview = Some(claim(ClaimState::Cancelled));
            applied(c, &meta("m3", "t1", "a@acme.io", 30), &cancelled, None, 4);
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
            let a = applied(c, &meta("m1", "t1", "a@acme.io", 10), &e, Some(&check), 1);
            let app = repo::get_application(c, a.application_id)?;
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
