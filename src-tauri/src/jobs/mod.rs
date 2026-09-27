//! Job-application intelligence, provider-independent:
//!
//! ```text
//! mail connector (Gmail / Outlook) — incremental sync since the cursor
//!        ↓
//! dedupe            — messages seen before are skipped
//!        ↓
//! prefilter         — deterministic signals; most mail never reaches a model
//!        ↓
//! triage            — headers-only relevance check (candidates only)
//!        ↓
//! classification    — one relevant email at a time, strict JSON, validated
//!        ↓
//! tracker           — matched application, status, audit timeline
//!        ↓
//! interviews        — verified against the email; proposed slots checked
//!        ↓
//! calendar          — conflicts; propose or (auto mode) add; reschedules
//!        ↓
//! notifications and the run report (built from stored state)
//! ```
//!
//! The model never sees a token, the mailbox, or unrelated mail; its
//! requests carry no tools; email text is delimited data; and nothing it
//! writes changes state unless it passes validation.

pub mod applications;
pub mod calendar_sync;
pub mod extract;
pub mod interviews;
pub mod prefilter;
pub mod report;
pub mod tracker;

use std::collections::{HashMap, HashSet};

use tokio_util::sync::CancellationToken;

use crate::{
    connectors::{
        calendar::CalendarProvider,
        mail::{MailMessage, MailProvider},
    },
    db::{
        connectors as connector_repo,
        jobs::{self as repo, MailRecord, MailStatus},
    },
    error::{AppError, AppResult},
    llm::{ChatRequest, Endpoint, Finish},
    models::{
        connectors::{ConnectorId, InterviewMode, ProviderId},
        jobs::{CalendarOutcome, CalendarReport, EmailCategory, JobRunReport, ProposedSlot},
        provider::ModelRef,
    },
    services::notifications::{self, Notice},
    state::AppState,
};
use calendar_sync::CalendarPolicy;
use interviews::InterviewCheck;
use prefilter::{Signals, Verdict};

pub const DAY_MS: i64 = 86_400_000;
/// Never look further back than this, however long ReMa was closed.
pub const MAX_WINDOW_DAYS: i64 = 90;
/// Emails classified per run; the rest wait for the next run.
pub const MAX_EXTRACTIONS_PER_RUN: usize = 25;
/// Below this confidence an email changes nothing (it is listed for review).
pub const MIN_CONFIDENCE: f64 = 0.5;
/// The sync cursor resource for a mailbox's inbox.
pub const MAIL_RESOURCE: &str = "mail:inbox";

/// What the run needs to talk to its model.
pub struct RunModel<'a> {
    pub endpoint: &'a Endpoint,
    pub model: &'a ModelRef,
    pub max_output_tokens: Option<u32>,
}

/// One connected mailbox.
pub struct MailSource<'a> {
    pub connector: ConnectorId,
    pub provider: &'a dyn MailProvider,
    pub account_id: String,
    /// Where a first or recovery sync starts (epoch ms).
    pub since: i64,
}

/// The connectors a run may use.
pub struct Sources<'a> {
    pub mail: Vec<MailSource<'a>>,
    /// Connected calendars (at most one per provider).
    pub calendars: Vec<&'a dyn CalendarProvider>,
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    /// The user's instructions (scheduled tasks); interpretation only.
    pub instructions: String,
    /// Check calendars and handle confirmed interviews.
    pub calendar: bool,
    pub policy: CalendarPolicy,
}

impl RunConfig {
    pub fn load(state: &AppState, instructions: &str, calendar: bool) -> AppResult<Self> {
        let prefs = state.db.call(|c| connector_repo::preferences(c))?;
        Ok(Self {
            instructions: instructions.to_string(),
            calendar,
            policy: CalendarPolicy {
                mode: prefs.interview_mode,
                buffer_ms: i64::from(prefs.prep_buffer_minutes) * 60_000,
            },
        })
    }
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            instructions: String::new(),
            calendar: true,
            policy: CalendarPolicy {
                mode: InterviewMode::Ask,
                buffer_ms: 0,
            },
        }
    }
}

async fn ask(
    state: &AppState,
    model: &RunModel<'_>,
    mut request: ChatRequest,
    cancel: &CancellationToken,
) -> AppResult<String> {
    if let Some(limit) = model.max_output_tokens {
        request.max_output_tokens = Some(
            request
                .max_output_tokens
                .map_or(limit, |own| own.min(limit)),
        );
    }
    // Pipeline requests never carry tools or web access: email content
    // cannot make the model act.
    request.tools = None;
    request.web = None;
    let mut text = String::new();
    let mut sink = |delta: &str| text.push_str(delta);
    match state
        .llm
        .stream_chat(
            model.endpoint,
            &model.model.model_id,
            &request,
            cancel.clone(),
            &mut sink,
        )
        .await?
    {
        Finish::Cancelled => Err(cancelled()),
        _ => Ok(text),
    }
}

fn cancelled() -> AppError {
    AppError::validation("The run was cancelled.")
}

fn short_error(error: &AppError) -> String {
    error.to_string().chars().take(200).collect()
}

/// Start of a first or recovery sync: the lookback, extended back to the
/// last success, at most 90 days.
pub fn window_start(now: i64, lookback_days: u32, last_success: Option<i64>) -> i64 {
    let lookback = now - i64::from(lookback_days) * DAY_MS;
    last_success
        .map_or(lookback, |at| at.min(lookback))
        .max(now - MAX_WINDOW_DAYS * DAY_MS)
}

/// Signals for the prefilter, from what the tracker already knows.
pub(crate) fn signals(state: &AppState, provider: ProviderId) -> AppResult<Signals> {
    state.db.call(|c| {
        let apps = repo::all_applications(c)?;
        Ok(Signals {
            known_threads: repo::application_threads(c, provider)?,
            known_domains: apps
                .iter()
                .filter_map(|a| a.sender_domain.clone())
                .filter(|d| !applications::is_shared_domain(d))
                .collect(),
            known_companies: apps.iter().map(|a| a.company_key.clone()).collect(),
            known_roles: apps.iter().filter_map(|a| a.role_key.clone()).collect(),
            known_senders: repo::known_job_senders(c)?,
        })
    })
}

fn new_record(message: &MailMessage, status: MailStatus, score: i64, now: i64) -> MailRecord {
    // Filtered mail keeps no subject or sender (only the domain).
    let keep = status != MailStatus::Filtered;
    MailRecord {
        provider: message.provider(),
        message_id: message.message_id.clone(),
        thread_id: message.thread_id.clone(),
        received_at: message.received_at,
        sender_domain: message.sender_domain(),
        sender: keep.then(|| message.sender.clone()),
        subject: keep.then(|| message.subject.clone()),
        web_link: keep.then(|| message.provider_web_link.clone()).flatten(),
        status,
        prefilter_score: Some(score),
        attempts: 0,
        classification: None,
        category: None,
        confidence: None,
        extraction: None,
        application_id: None,
        error: None,
        first_seen_at: now,
        processed_at: None,
    }
}

/// Step 1: new mail since the cursor, deduplicated and prefiltered. The new
/// cursor is stored in the same transaction as the records.
async fn sync_mailbox(
    state: &AppState,
    source: &MailSource<'_>,
    report: &mut JobRunReport,
    now: i64,
) -> AppResult<()> {
    let provider = source.provider.provider();
    let cursor = state
        .db
        .call(|c| connector_repo::cursor(c, provider, &source.account_id, MAIL_RESOURCE))?;
    let batch = source
        .provider
        .sync_changes(cursor.as_deref(), source.since)
        .await?;
    if batch.resynced {
        report.issues.push(format!(
            "{}: the sync position had expired; ReMa resynchronized recent mail.",
            source.connector.name()
        ));
    }
    report.emails_checked += batch.messages.len() as u32;
    let ids: Vec<String> = batch
        .messages
        .iter()
        .map(|m| m.message_id.clone())
        .collect();
    let known = state.db.call(|c| repo::known_messages(c, provider, &ids))?;
    let mut seen = HashSet::new();
    let fresh: Vec<&MailMessage> = batch
        .messages
        .iter()
        .filter(|m| !known.contains(&m.message_id) && seen.insert(m.message_id.clone()))
        .collect();
    report.new_emails += fresh.len() as u32;
    let signals = signals(state, provider)?;
    state.db.call(|c| {
        let tx = c.transaction()?;
        for message in &fresh {
            let scored = prefilter::score(message, &signals);
            let status = match scored.verdict {
                Verdict::Strong => MailStatus::Pending,
                Verdict::Candidate => MailStatus::Candidate,
                Verdict::Filtered => MailStatus::Filtered,
            };
            repo::upsert_mail(&tx, &new_record(message, status, scored.score, now))?;
        }
        report.relevant_emails += fresh
            .iter()
            .filter(|m| prefilter::score(m, &signals).verdict == Verdict::Strong)
            .count() as u32;
        connector_repo::save_cursor(
            &tx,
            provider,
            &source.account_id,
            MAIL_RESOURCE,
            &batch.cursor,
            now,
        )?;
        tx.commit()?;
        Ok(())
    })?;
    Ok(())
}

/// Step 2: headers-only relevance for candidates.
async fn triage(
    state: &AppState,
    model: &RunModel<'_>,
    provider: ProviderId,
    config: &RunConfig,
    report: &mut JobRunReport,
    cancel: &CancellationToken,
) -> AppResult<()> {
    let candidates = state.db.call(|c| repo::candidate_mail(c, provider))?;
    for batch in candidates.chunks(extract::TRIAGE_BATCH) {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let metas: Vec<MailMessage> = batch
            .iter()
            .map(|r| MailMessage {
                provider: Some(provider),
                message_id: r.message_id.clone(),
                thread_id: r.thread_id.clone(),
                received_at: r.received_at,
                sender: r.sender.clone().unwrap_or_default(),
                subject: r.subject.clone().unwrap_or_default(),
                ..MailMessage::default()
            })
            .collect();
        let ids: HashSet<String> = batch.iter().map(|r| r.message_id.clone()).collect();
        let relevant = match ask(
            state,
            model,
            extract::triage_request(&metas, &config.instructions),
            cancel,
        )
        .await
        .and_then(|text| extract::parse_triage(&text, &ids))
        {
            Ok(relevant) => relevant.into_iter().collect::<HashSet<_>>(),
            Err(error) => {
                if cancel.is_cancelled() {
                    return Err(error);
                }
                // Left as candidates: checked again next run.
                report.issues.push(format!(
                    "{} emails could not be checked and will be retried: {}",
                    batch.len(),
                    short_error(&error)
                ));
                continue;
            }
        };
        report.relevant_emails += relevant.len() as u32;
        state.db.call(|c| {
            let tx = c.transaction()?;
            for record in batch {
                let mut record = record.clone();
                if relevant.contains(&record.message_id) {
                    record.status = MailStatus::Pending;
                } else {
                    record.status = MailStatus::Irrelevant;
                    // Nothing to keep about unrelated mail.
                    record.sender = None;
                    record.subject = None;
                    record.web_link = None;
                }
                repo::upsert_mail(&tx, &record)?;
            }
            tx.commit()?;
            Ok(())
        })?;
    }
    Ok(())
}

/// Validated proposed times, checked against the calendar.
async fn check_slots(
    extraction: &extract::Extraction,
    email_text: &str,
    calendar: Option<&dyn CalendarProvider>,
    buffer_ms: i64,
    now: i64,
) -> Vec<ProposedSlot> {
    let Some(claim) = &extraction.interview else {
        return Vec::new();
    };
    let mut slots = Vec::new();
    for slot in &claim.proposed_slots {
        let Some((start_at, end_at, timezone)) = interviews::validate_slot(slot, email_text, now)
        else {
            continue;
        };
        let (available, conflicts) = match calendar {
            Some(api) => match api
                .list_events(start_at - buffer_ms, end_at + buffer_ms)
                .await
            {
                Ok(events) => {
                    let conflicts: Vec<_> = crate::connectors::calendar::find_conflicts(
                        &events, start_at, end_at, -1, buffer_ms,
                    )
                    .into_iter()
                    .map(|e| crate::models::jobs::ConflictingEvent {
                        title: e.title,
                        start_at: e.start_at.unwrap_or_default(),
                        end_at: e.end_at.unwrap_or_default(),
                    })
                    .collect();
                    (Some(conflicts.is_empty()), conflicts)
                }
                Err(_) => (None, Vec::new()),
            },
            None => (None, Vec::new()),
        };
        slots.push(ProposedSlot {
            start_at,
            end_at,
            timezone,
            available,
            conflicts,
        });
    }
    slots
}

/// Notifications for what one email changed.
fn notify_change(
    state: &AppState,
    message: &MailMessage,
    extraction: &extract::Extraction,
    applied: &applications::Applied,
    slots: &[ProposedSlot],
) {
    let app = match state
        .db
        .call(|c| repo::get_application(c, applied.application_id))
    {
        Ok(app) => app,
        Err(_) => return,
    };
    let heading = match &app.role {
        Some(role) => format!("{} — {role}", app.company),
        None => app.company.clone(),
    };
    let key = Some(format!(
        "mail:{}:{}",
        message.provider().as_str(),
        message.message_id
    ));
    let notice = |kind: &'static str, title: &str, body: String| Notice {
        kind,
        title: title.to_string(),
        body,
        application_id: Some(app.id),
        interview_id: applied.interview_id,
        dedupe_key: key.clone(),
    };
    match extraction.category {
        EmailCategory::InterviewRequest => {
            let mut body = heading;
            for slot in slots.iter().take(3) {
                let status = match slot.available {
                    Some(true) => "free".to_string(),
                    Some(false) => format!(
                        "conflict: {}",
                        slot.conflicts
                            .iter()
                            .map(|c| c.title.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    None => "calendar not connected".to_string(),
                };
                body.push_str(&format!(
                    "\n{} ({status})",
                    calendar_sync::when(slot.start_at, Some(&slot.timezone))
                ));
            }
            notifications::add(
                state,
                notice("interview", "Interview invitation detected", body),
            );
        }
        // Confirmed interviews are announced by the calendar step (with
        // their conflict status).
        EmailCategory::InterviewConfirmed | EmailCategory::InterviewRescheduled => {}
        EmailCategory::AssessmentRequest | EmailCategory::ActionRequired => {
            let next = extraction
                .next_action
                .clone()
                .unwrap_or_else(|| "Check the latest email.".into());
            notifications::add(
                state,
                notice("action", "Action required", format!("{heading}\n{next}")),
            );
        }
        _ if applied.status_changed || applied.created => {
            notifications::add(
                state,
                notice(
                    "application",
                    "Application update detected",
                    format!("{heading}\nStatus: {}", applied.status.label()),
                ),
            );
        }
        _ => {}
    }
}

/// Step 3: classify pending mail and update the tracker.
#[allow(clippy::too_many_arguments)]
async fn classify(
    state: &AppState,
    model: &RunModel<'_>,
    source: &MailSource<'_>,
    calendars: &[&dyn CalendarProvider],
    config: &RunConfig,
    budget: &mut usize,
    report: &mut JobRunReport,
    touched: &mut HashSet<i64>,
    cancel: &CancellationToken,
    now: i64,
) -> AppResult<()> {
    let provider = source.provider.provider();
    let pending = state.db.call(|c| repo::pending_mail(c, provider))?;
    report.deferred_emails += pending.len().saturating_sub(*budget) as u32;
    let known = state.db.call(|c| applications::known_applications(c, 20))?;
    let today = jiff::Timestamp::from_millisecond(now)
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()).date().to_string())
        .unwrap_or_default();
    let calendar = calendars
        .iter()
        .find(|c| c.provider() == provider)
        .or(calendars.first())
        .copied();

    for mail in pending.into_iter().take(*budget) {
        *budget -= 1;
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let outcome: AppResult<MailRecord> = async {
            let message = source.provider.get_message(&mail.message_id).await?;
            let text = ask(
                state,
                model,
                extract::classification_request(&message, &known, &config.instructions, &today),
                cancel,
            )
            .await?;
            let Some(extraction) = extract::parse_extraction(&text)? else {
                return Ok(MailRecord {
                    status: MailStatus::Irrelevant,
                    category: Some(EmailCategory::NotJobRelated),
                    sender: None,
                    subject: None,
                    web_link: None,
                    processed_at: Some(now),
                    attempts: mail.attempts + 1,
                    ..mail.clone()
                });
            };
            let stored = serde_json::to_string(&extraction).ok();
            let base = MailRecord {
                sender: Some(message.sender.clone()),
                subject: Some(message.subject.clone()),
                web_link: message.provider_web_link.clone(),
                category: Some(extraction.category),
                confidence: Some(extraction.confidence),
                classification: extraction.status,
                extraction: stored,
                processed_at: Some(now),
                attempts: mail.attempts + 1,
                error: None,
                ..mail.clone()
            };
            if extraction.confidence < MIN_CONFIDENCE {
                // Ambiguous: listed for review, no state changes.
                let application_id = state
                    .db
                    .call(|c| applications::find_application(c, &message, &extraction))?
                    .map(|a| a.id);
                return Ok(MailRecord {
                    status: MailStatus::Ambiguous,
                    application_id,
                    ..base
                });
            }
            let email_text = message.text();
            let check = extraction
                .interview
                .as_ref()
                .map(|claim| interviews::validate(claim, &email_text, now));
            if let Some(InterviewCheck::Upcoming(v) | InterviewCheck::Past(v)) = &check {
                report.issues.extend(
                    v.notes
                        .iter()
                        .map(|n| format!("{}: {n}", extraction.company)),
                );
            }
            let slots = if extraction.category == EmailCategory::InterviewRequest {
                check_slots(
                    &extraction,
                    &email_text,
                    calendar.filter(|_| config.calendar),
                    config.policy.buffer_ms,
                    now,
                )
                .await
            } else {
                Vec::new()
            };
            let applied = state.db.call(|c| {
                let tx = c.transaction()?;
                let applied = applications::apply(&tx, &message, &extraction, check.as_ref(), now)?;
                if let (Some(applied), false) = (&applied, slots.is_empty()) {
                    if let Some(id) = applied.interview_id {
                        let mut interview = repo::get_interview(&tx, id)?;
                        interview.proposed_slots = slots.clone();
                        repo::save_interview(&tx, &interview)?;
                    }
                }
                tx.commit()?;
                Ok(applied)
            })?;
            let Some(applied) = applied else {
                return Ok(MailRecord {
                    status: MailStatus::Processed,
                    ..base
                });
            };
            touched.insert(applied.application_id);
            if applied.created {
                report.new_applications += 1;
            }
            if applied.rejected_now {
                report.new_rejections += 1;
            }
            notify_change(state, &message, &extraction, &applied, &slots);
            Ok(MailRecord {
                status: MailStatus::Processed,
                application_id: Some(applied.application_id),
                ..base
            })
        }
        .await;

        let record = match outcome {
            Ok(record) => record,
            Err(error) => {
                if cancel.is_cancelled() {
                    return Err(error);
                }
                if matches!(error, AppError::Authentication(_)) {
                    return Err(error);
                }
                let domain = mail
                    .sender_domain
                    .clone()
                    .unwrap_or_else(|| "an unknown sender".into());
                report.issues.push(format!(
                    "An email from {domain} could not be processed: {}",
                    short_error(&error)
                ));
                MailRecord {
                    status: MailStatus::Failed,
                    attempts: mail.attempts + 1,
                    error: Some(short_error(&error)),
                    processed_at: Some(now),
                    ..mail.clone()
                }
            }
        };
        state.db.call(|c| repo::upsert_mail(c, &record))?;
    }
    Ok(())
}

/// Runs the pipeline once over the given sources. Without a model, mail is
/// still synchronized and prefiltered; classification waits for a model.
pub async fn run(
    state: &AppState,
    model: Option<RunModel<'_>>,
    sources: Sources<'_>,
    config: RunConfig,
    cancel: &CancellationToken,
    now: i64,
) -> AppResult<JobRunReport> {
    let window_start = sources
        .mail
        .iter()
        .map(|s| s.since)
        .min()
        .unwrap_or(now - 14 * DAY_MS);
    let mut report = JobRunReport {
        window_start,
        window_end: now,
        ..JobRunReport::default()
    };

    // 1. Sync every mailbox (errors stay with their mailbox).
    let mut synced = Vec::new();
    for source in &sources.mail {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        match sync_mailbox(state, source, &mut report, now).await {
            Ok(()) => synced.push(source),
            Err(error @ AppError::Authentication(_)) if sources.mail.len() == 1 => {
                return Err(error)
            }
            Err(error) => report.issues.push(format!(
                "{} could not be synchronized: {}",
                source.connector.name(),
                short_error(&error)
            )),
        }
    }

    // 2–3. Relevance and classification need a model.
    let mut touched = HashSet::new();
    match &model {
        Some(model) => {
            let mut budget = MAX_EXTRACTIONS_PER_RUN;
            for source in &synced {
                let provider = source.provider.provider();
                triage(state, model, provider, &config, &mut report, cancel).await?;
                classify(
                    state,
                    model,
                    source,
                    &sources.calendars,
                    &config,
                    &mut budget,
                    &mut report,
                    &mut touched,
                    cancel,
                    now,
                )
                .await?;
            }
        }
        None => report.issues.push(
            "No model is set up: new job emails wait until you choose a default model in \
             Settings."
                .into(),
        ),
    }
    report.applications_updated = touched.len() as u32;
    state
        .db
        .call(|c| applications::settle_past_interviews(c, now))?;

    // 4. Calendars.
    if config.calendar && !sources.calendars.is_empty() {
        let calendar = sync_calendars(
            state,
            &sources.calendars,
            config.policy,
            window_start,
            now,
            &mut report.issues,
        )
        .await?;
        report.calendar = Some(calendar);
    }

    // 5. The overview, from stored state.
    report.applications = state.db.call(|c| report::overview(c, window_start, now))?;
    report::count(&mut report);
    state.events.applications_changed();
    Ok(report)
}

/// The calendar step for every interview that needs it.
pub async fn sync_calendars(
    state: &AppState,
    calendars: &[&dyn CalendarProvider],
    policy: CalendarPolicy,
    window_start: i64,
    now: i64,
    issues: &mut Vec<String>,
) -> AppResult<CalendarReport> {
    let mut calendar = CalendarReport::default();
    let to_sync = state.db.call(|c| repo::interviews_to_sync(c, now))?;
    let by_provider: HashMap<ProviderId, &dyn CalendarProvider> =
        calendars.iter().map(|c| (c.provider(), *c)).collect();
    for interview in to_sync {
        // The calendar that holds its event, else the mail provider's, else any.
        let api = interview
            .calendar_provider
            .and_then(|p| by_provider.get(&p))
            .or_else(|| by_provider.get(&interview.provider))
            .or(calendars.first())
            .copied();
        let Some(api) = api else { continue };
        let app = state
            .db
            .call(|c| repo::get_application(c, interview.application_id))?;
        let item =
            match calendar_sync::sync_interview(state, api, policy, &interview, &app, now).await {
                Ok(item) => item,
                Err(error @ AppError::Authentication(_)) => return Err(error),
                Err(error) => {
                    issues.push(format!(
                        "Calendar: {} — {}",
                        app.company,
                        short_error(&error)
                    ));
                    continue;
                }
            };
        let settled_cancellation = interview.state == repo::InterviewState::Cancelled
            && matches!(
                item.outcome,
                CalendarOutcome::Unchanged | CalendarOutcome::Removed
            );
        if settled_cancellation {
            continue;
        }
        match item.outcome {
            CalendarOutcome::Created => calendar.created += 1,
            CalendarOutcome::Updated => calendar.updated += 1,
            CalendarOutcome::Unchanged | CalendarOutcome::Removed => calendar.unchanged += 1,
            CalendarOutcome::Cancelled => calendar.cancelled += 1,
            CalendarOutcome::NeedsReview
            | CalendarOutcome::Proposed
            | CalendarOutcome::Conflict => calendar.needs_review += 1,
        }
        if !item.conflicts.is_empty() {
            calendar.conflicts += 1;
        }
        calendar.items.push(item);
    }
    let reviews = state
        .db
        .call(|c| repo::interviews_needing_review(c, window_start))?;
    for interview in reviews {
        let app = state
            .db
            .call(|c| repo::get_application(c, interview.application_id))?;
        calendar.needs_review += 1;
        calendar
            .items
            .push(calendar_sync::review_item(&app, &interview));
    }
    Ok(calendar)
}

#[cfg(test)]
mod tests;
