//! Background job-details pipeline.
//!
//! For each newly ingested job, one at a time:
//! 1. read its page (if allowed): JSON-LD facts, else the page text;
//! 2. extract requirements deterministically from the description;
//! 3. optionally let the default model extract requirements, validated
//!    against the description (see `extract::parse_model`);
//! 4. store only values the job did not state yet, and merge the job into
//!    an existing record when the page proves they are the same posting.

use std::time::Duration;

use super::{extract, ingest, normalize, page};
use crate::{
    db::analytics::{self as repo, DetailsUpdate, JobFields, PendingJob},
    error::AppResult,
    models::analytics::DetailsStatus,
    state::AppState,
    time::now_ms,
};

/// Pause between two jobs, so sites are not asked in quick succession.
const PAUSE: Duration = Duration::from_millis(800);
const IDLE: Duration = Duration::from_secs(60);

/// Starts the worker (after making earlier searches available).
pub fn start(state: AppState) {
    tauri::async_runtime::spawn(async move {
        let history = state.clone();
        match tauri::async_runtime::spawn_blocking(move || ingest::ingest_history(&history)).await {
            Ok(Err(error)) => eprintln!("analytics: reading earlier searches failed: {error}"),
            Err(error) => eprintln!("analytics: reading earlier searches failed: {error}"),
            Ok(Ok(_)) => {}
        }
        let shutdown = state.analytics.shutdown.clone();
        loop {
            if shutdown.is_cancelled() {
                break;
            }
            match process_next(&state).await {
                Ok(true) => {
                    tokio::select! {
                        _ = shutdown.cancelled() => break,
                        _ = tokio::time::sleep(PAUSE) => {}
                    }
                }
                Ok(false) => {
                    tokio::select! {
                        _ = shutdown.cancelled() => break,
                        _ = state.analytics.wake.notified() => {}
                        _ = tokio::time::sleep(IDLE) => {}
                    }
                }
                Err(error) => {
                    eprintln!("analytics: job details: {error}");
                    tokio::time::sleep(IDLE).await;
                }
            }
        }
    });
}

/// Processes one pending job. Returns false when none is waiting.
pub async fn process_next(state: &AppState) -> AppResult<bool> {
    let Some(job) = state.db.call(|c| repo::next_pending(c))? else {
        return Ok(false);
    };
    let options = state.analytics.options(state)?;
    let outcome = read(state, &job, options.read_pages, options.use_model).await;
    let now = now_ms();
    state.db.call(|conn| {
        let tx = conn.transaction()?;
        repo::save_details(&tx, job.id, &outcome.update, now)?;
        repo::add_requirements(
            &tx,
            job.id,
            &ingest::requirement_fields(&outcome.requirements),
        )?;
        // A reference number or description the page shows may prove this
        // job is one ReMa already knows.
        for key in &outcome.keys {
            match repo::key_owner(&tx, key)? {
                Some(other) if other != job.id => {
                    let (keep, duplicate) = (other.min(job.id), other.max(job.id));
                    repo::merge_jobs(&tx, keep, duplicate, now)?;
                    repo::add_keys(&tx, keep, &outcome.keys)?;
                    break;
                }
                Some(_) => {}
                None => repo::add_keys(&tx, job.id, std::slice::from_ref(key))?,
            }
        }
        tx.commit()?;
        Ok(())
    })?;
    state.analytics.changed();
    state.analytics.notify_changed(state);
    Ok(true)
}

struct Outcome {
    update: DetailsUpdate,
    requirements: Vec<extract::NewRequirement>,
    keys: Vec<String>,
}

async fn read(state: &AppState, job: &PendingJob, read_pages: bool, use_model: bool) -> Outcome {
    let mut notes: Vec<String> = Vec::new();
    let mut fields = JobFields::default();
    let mut page_description = None;
    let mut items: Vec<String> = Vec::new();
    let mut benefits = Vec::new();
    let mut keys = Vec::new();
    let mut page_read = false;

    let url = job
        .source_url
        .as_deref()
        .filter(|u| normalize::is_job_specific(u));
    match (url, read_pages) {
        (Some(url), true) => match state.analytics.fetch_page(state, url).await {
            Ok(html) => {
                page_read = true;
                let facts = page::parse(&html, &job.title);
                let place = facts
                    .location
                    .as_deref()
                    .map(normalize::place)
                    .unwrap_or_default();
                fields = JobFields {
                    company: facts.company.clone(),
                    company_key: facts.company.as_deref().map(normalize::company_key),
                    city: place.city().map(str::to_string),
                    country: place.country().map(str::to_string),
                    location: facts.location.clone(),
                    work_mode: facts.work_mode,
                    employment_type: facts.employment_type,
                    salary_min: facts.salary.as_ref().and_then(|s| s.min),
                    salary_max: facts.salary.as_ref().and_then(|s| s.max),
                    salary_currency: facts
                        .salary
                        .as_ref()
                        .and_then(|s| s.currency.map(str::to_string)),
                    salary_period: facts.salary.as_ref().and_then(|s| s.period),
                    salary_text: facts.salary_text.clone(),
                    date_posted: facts
                        .date_posted
                        .as_deref()
                        .and_then(|d| normalize::posted(d, now_ms())),
                    external_ref: facts.reference.clone(),
                    ..JobFields::default()
                };
                if let Some(company_key) =
                    job.company_key.as_deref().or(fields.company_key.as_deref())
                {
                    if let Some(reference) = &facts.reference {
                        keys.push(format!(
                            "ref:{company_key}|{}",
                            reference
                                .to_lowercase()
                                .split(|c: char| !c.is_alphanumeric())
                                .filter(|w| !w.is_empty())
                                .collect::<Vec<_>>()
                                .join(" ")
                        ));
                    }
                    if let Some(key) = facts
                        .description
                        .as_deref()
                        .and_then(|d| ingest::description_key(company_key, d))
                    {
                        keys.push(key);
                    }
                }
                items = facts.skills;
                benefits = facts.benefits;
                page_description = match (facts.description, facts.requirement_text) {
                    (Some(d), Some(r)) => Some(format!("{d}\n{r}")),
                    (d, r) => d.or(r),
                };
                if page_description.is_none() {
                    notes.push("The page has no readable job description.".into());
                }
            }
            Err(error) => notes.push(format!("The job page could not be read: {error}.")),
        },
        (Some(_), false) => notes.push("Reading job pages is turned off.".into()),
        (None, _) => {}
    }

    let description = page_description.clone().or_else(|| job.description.clone());
    let mut requirements = extract::from_items(&items);
    for r in &mut requirements {
        r.source = crate::models::analytics::RequirementSource::Description;
    }
    if let Some(text) = &description {
        let facts = extract::from_description(text);
        requirements.extend(facts.requirements);
        if benefits.is_empty() {
            benefits = facts.benefits;
        }
        if use_model && text.len() >= 200 {
            match state.analytics.model_requirements(state, text).await {
                Ok(Some((found, _dropped))) => requirements.extend(found),
                Ok(None) => notes.push("No model is set up to read descriptions.".into()),
                Err(error) => notes.push(format!(
                    "The model could not read the description: {error}."
                )),
            }
        }
    }
    // A page that could not be read is final: ReMa does not retry sites
    // that refuse automated reading.
    let status = if description.is_some() || page_read || url.is_none() {
        DetailsStatus::Done
    } else if !read_pages {
        DetailsStatus::Skipped
    } else {
        DetailsStatus::Failed
    };
    Outcome {
        update: DetailsUpdate {
            description: page_description,
            fields,
            benefits,
            status: Some(status),
            note: (!notes.is_empty()).then(|| notes.join(" ")),
        },
        requirements: extract::dedupe(requirements),
        keys,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        analytics::ingest::{ingest, RawJob, RawJobSearch},
        llm::fake::FakeLanguageModel,
        models::analytics::RunSource,
        state::testing,
    };

    fn search(jobs: Vec<RawJob>) -> RawJobSearch {
        RawJobSearch {
            origin: "t:1".into(),
            title: "Search".into(),
            query: "q".into(),
            source: RunSource::Tool,
            task_id: None,
            conversation_id: None,
            message_id: None,
            execution_id: None,
            created_at: 1,
            jobs,
        }
    }

    #[tokio::test]
    async fn reads_descriptions_without_pages_or_model() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        ingest(
            &state,
            search(vec![RawJob {
                title: "Data Engineer".into(),
                company: Some("Globex".into()),
                description: Some(
                    "You bring:\n- 3+ years of experience with Apache Spark\n- dbt is a plus"
                        .into(),
                ),
                ..RawJob::default()
            }]),
        )
        .unwrap();
        assert!(process_next(&state).await.unwrap());
        assert!(!process_next(&state).await.unwrap(), "nothing left");
        let (status, names): (String, String) = state
            .db
            .call(|c| {
                Ok((
                    c.query_row("SELECT details_status FROM jobs", [], |r| r.get(0))?,
                    c.query_row(
                        "SELECT group_concat(name || ':' || importance, ',') FROM
                         (SELECT name, importance FROM job_requirements ORDER BY name)",
                        [],
                        |r| r.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!(status, "done");
        assert_eq!(
            names,
            "3+ years experience:required,Apache Spark:required,dbt:preferred"
        );
    }
}
