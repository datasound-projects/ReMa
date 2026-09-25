//! Job-application intelligence: Gmail → structured state → Calendar.
//!
//! One run of a "job applications" scheduled task:
//!
//! 1. Rust searches Gmail with a fixed query over the date window
//!    (lookback, extended back to the last successful run).
//! 2. Messages already handled in earlier runs are skipped.
//! 3. The model sees only headers + snippets of new candidates and picks the
//!    job-related ones.
//! 4. Only those are read in full; the model extracts facts as JSON that Rust
//!    validates (`extract`), verifies (`interviews`) and stores (`applications`).
//! 5. Confirmed interviews are synchronized to Calendar (`calendar_sync`).
//! 6. The report is built from stored state (`report`).

pub mod applications;
pub mod calendar_sync;
pub mod extract;
pub mod interviews;
pub mod report;

use std::collections::{HashMap, HashSet};

use tokio_util::sync::CancellationToken;

use crate::{
    db::jobs::{self as repo, MailRecord, MailStatus},
    error::{AppError, AppResult},
    integrations::google::{
        calendar::CalendarApi,
        gmail::{job_search_query, GmailApi, MessageMeta, MAX_SEARCH_RESULTS},
    },
    llm::{ChatRequest, Endpoint, Finish},
    models::{
        jobs::{CalendarOutcome, CalendarReport, JobRunReport},
        provider::ModelRef,
    },
    state::AppState,
};
use interviews::InterviewCheck;

const DAY_MS: i64 = 86_400_000;
/// Never look further back than this, however long ReMa was closed.
const MAX_WINDOW_DAYS: i64 = 90;
/// Emails read in full per run; the rest wait for the next run.
pub const MAX_EXTRACTIONS_PER_RUN: usize = 25;

/// Settings of a job-application task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobRunConfig {
    pub lookback_days: u32,
    pub sync_calendar: bool,
    pub detect_conflicts: bool,
}

/// The Google tools a run may use.
pub struct Tools<'a> {
    pub gmail: &'a dyn GmailApi,
    /// Why Calendar cannot be used, if it cannot.
    pub calendar: Result<&'a dyn CalendarApi, String>,
}

/// What the run needs to talk to the task's model.
pub struct RunModel<'a> {
    pub endpoint: &'a Endpoint,
    pub model: &'a ModelRef,
    pub max_output_tokens: Option<u32>,
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
        Finish::Cancelled => Err(AppError::validation("The run was cancelled.")),
        _ => Ok(text),
    }
}

/// What reading one relevant email produced.
struct Handled {
    status: MailStatus,
    extraction: Option<extract::Extraction>,
    application_id: Option<i64>,
    /// Details that were left out (e.g. a link not found in the email).
    notes: Vec<String>,
}

/// Start of the Gmail window: the lookback, extended back to the previous
/// successful run so nothing received in between is missed.
pub fn window_start(now: i64, lookback_days: u32, last_success: Option<i64>) -> i64 {
    let lookback = now - i64::from(lookback_days) * DAY_MS;
    last_success
        .map_or(lookback, |at| at.min(lookback))
        .max(now - MAX_WINDOW_DAYS * DAY_MS)
}

fn short_error(error: &AppError) -> String {
    error.to_string().chars().take(200).collect()
}

/// Runs the job-application workflow once.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    state: &AppState,
    task_id: i64,
    instructions: &str,
    model: RunModel<'_>,
    config: JobRunConfig,
    tools: Tools<'_>,
    cancel: &CancellationToken,
    now: i64,
) -> AppResult<JobRunReport> {
    let last_success = state.db.call(|c| {
        Ok(c.query_row(
            "SELECT MAX(started_at) FROM task_executions WHERE task_id = ?1 AND status = 'succeeded'",
            [task_id],
            |r| r.get::<_, Option<i64>>(0),
        )?)
    })?;
    let window_start = window_start(now, config.lookback_days, last_success);
    let mut report = JobRunReport {
        window_start,
        window_end: now,
        ..JobRunReport::default()
    };

    // 1–2. Deterministic search, skipping what earlier runs handled.
    let refs = tools
        .gmail
        .search(&job_search_query(window_start), MAX_SEARCH_RESULTS)
        .await?;
    report.emails_checked = refs.len() as u32;
    let ids: Vec<String> = refs.iter().map(|r| r.id.clone()).collect();
    let known_ids = state.db.call(|c| repo::known_messages(c, &ids))?;
    let unseen: Vec<_> = refs.iter().filter(|r| !known_ids.contains(&r.id)).collect();
    report.new_emails = unseen.len() as u32;

    // 3. Relevance from headers and snippets only.
    let mut metas: HashMap<String, MessageMeta> = HashMap::new();
    for r in &unseen {
        if cancel.is_cancelled() {
            return Err(AppError::validation("The run was cancelled."));
        }
        match tools.gmail.metadata(&r.id).await {
            Ok(meta) => {
                metas.insert(meta.id.clone(), meta);
            }
            Err(error) => report
                .issues
                .push(format!("Could not read an email: {}", short_error(&error))),
        }
    }
    let mut candidates: Vec<MessageMeta> = metas.values().cloned().collect();
    candidates.sort_by_key(|m| m.received_at);
    for batch in candidates.chunks(extract::TRIAGE_BATCH) {
        let batch_ids: HashSet<String> = batch.iter().map(|m| m.id.clone()).collect();
        let relevant = match ask(
            state,
            &model,
            extract::triage_request(batch, instructions),
            cancel,
        )
        .await
        .and_then(|text| extract::parse_triage(&text, &batch_ids))
        {
            Ok(relevant) => relevant.into_iter().collect::<HashSet<_>>(),
            Err(error) => {
                if cancel.is_cancelled() {
                    return Err(error);
                }
                // Left unrecorded: these emails are checked again next run.
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
            for meta in batch {
                repo::upsert_mail(
                    c,
                    &MailRecord {
                        message_id: meta.id.clone(),
                        thread_id: meta.thread_id.clone(),
                        received_at: meta.received_at,
                        sender_domain: meta.sender_domain(),
                        status: if relevant.contains(&meta.id) {
                            MailStatus::Pending
                        } else {
                            MailStatus::Irrelevant
                        },
                        attempts: 0,
                        classification: None,
                        extraction: None,
                        application_id: None,
                        error: None,
                        first_seen_at: now,
                        processed_at: None,
                    },
                )?;
            }
            Ok(())
        })?;
    }

    // 4. Read relevant emails and store validated facts.
    let pending = state.db.call(|c| repo::pending_mail(c))?;
    report.deferred_emails = pending.len().saturating_sub(MAX_EXTRACTIONS_PER_RUN) as u32;
    let known = state.db.call(|c| applications::known_applications(c, 20))?;
    let today = jiff::Timestamp::from_millisecond(now)
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()).date().to_string())
        .unwrap_or_default();
    let mut touched = HashSet::new();

    for mail in pending.into_iter().take(MAX_EXTRACTIONS_PER_RUN) {
        if cancel.is_cancelled() {
            return Err(AppError::validation("The run was cancelled."));
        }
        let outcome: AppResult<Handled> = async {
            let content = tools.gmail.message(&mail.message_id).await?;
            let text = ask(
                state,
                &model,
                extract::extraction_request(&content, &known, instructions, &today),
                cancel,
            )
            .await?;
            let Some(extraction) = extract::parse_extraction(&text)? else {
                return Ok(Handled {
                    status: MailStatus::Irrelevant,
                    extraction: None,
                    application_id: None,
                    notes: Vec::new(),
                });
            };
            let email_text = format!("{}\n{}", content.meta.subject, content.body);
            let check = extraction
                .interview
                .as_ref()
                .map(|claim| interviews::validate(claim, &email_text, now));
            let notes = match &check {
                Some(InterviewCheck::Upcoming(v) | InterviewCheck::Past(v)) => v
                    .notes
                    .iter()
                    .map(|note| format!("{}: {note}", extraction.company))
                    .collect(),
                _ => Vec::new(),
            };
            let applied = state.db.call(|c| {
                let tx = c.transaction()?;
                let applied =
                    applications::apply(&tx, &content.meta, &extraction, check.as_ref(), now)?;
                tx.commit()?;
                Ok(applied)
            })?;
            touched.insert(applied.application_id);
            if applied.created {
                report.new_applications += 1;
            }
            if applied.rejected_now {
                report.new_rejections += 1;
            }
            Ok(Handled {
                status: MailStatus::Processed,
                extraction: Some(extraction),
                application_id: Some(applied.application_id),
                notes,
            })
        }
        .await;

        let record = match outcome {
            Ok(Handled {
                status,
                extraction,
                application_id,
                notes,
            }) => {
                report.issues.extend(notes);
                MailRecord {
                    status,
                    classification: extraction.as_ref().map(|e| e.status),
                    extraction: extraction
                        .as_ref()
                        .and_then(|e| serde_json::to_string(e).ok()),
                    application_id,
                    error: None,
                    processed_at: Some(now),
                    attempts: mail.attempts + 1,
                    ..mail.clone()
                }
            }
            Err(error) => {
                if cancel.is_cancelled() {
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
    report.applications_updated = touched.len() as u32;
    state
        .db
        .call(|c| applications::settle_past_interviews(c, now))?;

    // 5. Calendar.
    if config.sync_calendar {
        match tools.calendar {
            Ok(api) => {
                let calendar = sync_calendar(
                    state,
                    api,
                    config.detect_conflicts,
                    window_start,
                    now,
                    &mut report.issues,
                )
                .await?;
                report.calendar = Some(calendar);
            }
            Err(reason) => report
                .issues
                .push(format!("Calendar sync was skipped: {reason}")),
        }
    }

    // 6. The overview, from stored state.
    report.applications = state.db.call(|c| report::overview(c, window_start, now))?;
    report::count(&mut report);
    Ok(report)
}

async fn sync_calendar(
    state: &AppState,
    api: &dyn CalendarApi,
    detect_conflicts: bool,
    window_start: i64,
    now: i64,
    issues: &mut Vec<String>,
) -> AppResult<CalendarReport> {
    let mut calendar = CalendarReport::default();
    let to_sync = state.db.call(|c| repo::interviews_to_sync(c, now))?;
    for interview in to_sync {
        let app = state
            .db
            .call(|c| repo::get_application(c, interview.application_id))?;
        let item = match calendar_sync::sync_interview(
            state,
            api,
            detect_conflicts,
            &interview,
            &app,
            now,
        )
        .await
        {
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
            continue; // already handled in an earlier run
        }
        match item.outcome {
            CalendarOutcome::Created => calendar.created += 1,
            CalendarOutcome::Updated => calendar.updated += 1,
            CalendarOutcome::Unchanged | CalendarOutcome::Removed => calendar.unchanged += 1,
            CalendarOutcome::Cancelled => calendar.cancelled += 1,
            CalendarOutcome::NeedsReview => calendar.needs_review += 1,
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
