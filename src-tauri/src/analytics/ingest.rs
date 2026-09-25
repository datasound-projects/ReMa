//! Job ingestion: raw search results → validated, normalized,
//! deduplicated job records with their search-run membership.
//!
//! Sources: chat answers and scheduled-task results (automatically, as soon
//! as they finish, and once for all history), answers the user sends to
//! Analytics explicitly, and any future search tool (via [`ingest`]).
//!
//! Deduplication uses deterministic evidence only, strongest first: the job
//! board's own id, the canonical job URL, company and reference number,
//! company and description fingerprint, and company, title and place. A job
//! found again gains a membership in the new run; it is never stored twice.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::{extract, normalize, table};
use crate::{
    db::{
        analytics::{self as repo, JobFields, RequirementFields, RunFields},
        providers as settings,
    },
    error::{AppError, AppResult},
    llm::{ChatRequest, Finish, Turn},
    models::{
        analytics::{DetailsStatus, RunSource},
        chat::MessageRole,
    },
    services::{chat::make_title, providers},
    state::AppState,
    time::now_ms,
};

/// Limits applied to every raw result.
const MAX_JOBS_PER_RUN: usize = 200;
const MAX_QUERY_CHARS: usize = 2_000;

/// One listing as a search reported it (strings as written).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RawJob {
    pub title: String,
    pub company: Option<String>,
    pub location: Option<String>,
    pub work_mode: Option<String>,
    pub employment_type: Option<String>,
    pub seniority: Option<String>,
    pub salary: Option<String>,
    pub posted: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    #[serde(default)]
    pub requirements: Vec<String>,
    pub description: Option<String>,
    pub reference: Option<String>,
}

/// One execution of a job search, from any source.
#[derive(Debug, Clone)]
pub struct RawJobSearch {
    /// Unique per execution, e.g. "chat:42" or "task:17".
    pub origin: String,
    pub title: String,
    pub query: String,
    pub source: RunSource,
    pub task_id: Option<i64>,
    pub conversation_id: Option<i64>,
    pub message_id: Option<i64>,
    pub execution_id: Option<i64>,
    pub created_at: i64,
    pub jobs: Vec<RawJob>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestOutcome {
    pub run_id: i64,
    /// Distinct jobs in the run.
    pub jobs: u32,
    pub new_jobs: u32,
}

/// A listing ready to store, with the keys that identify it.
pub struct Normalized {
    pub fields: JobFields,
    /// Stored and looked up, strongest evidence first.
    pub keys: Vec<String>,
    /// Only looked up (weakest): a country-level match for a listing whose
    /// city another sighting did not state, or the other form of its URL.
    pub probes: Vec<String>,
    /// Only stored: lets later country-only listings find this one.
    pub markers: Vec<String>,
    pub requirements: Vec<extract::NewRequirement>,
}

impl Normalized {
    /// Every key to look up, strongest first.
    pub fn lookup(&self) -> Vec<String> {
        self.keys.iter().chain(&self.probes).cloned().collect()
    }

    /// Every key to store.
    pub fn stored(&self) -> Vec<String> {
        self.keys.iter().chain(&self.markers).cloned().collect()
    }
}

fn words_key(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
        .filter(|w| !w.is_empty())
        .map(|w| match w {
            "sr" => "senior",
            "jr" => "junior",
            "ml" => "machine learning",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fingerprint of a description, for deduplication.
pub fn description_key(company_key: &str, description: &str) -> Option<String> {
    let simplified = words_key(description);
    if company_key.is_empty() || simplified.len() < 200 {
        return None;
    }
    let digest = Sha256::digest(simplified.as_bytes());
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    Some(format!("desc:{company_key}|{hex}"))
}

/// Validates and normalizes one listing. `None` if it names no job.
pub fn normalize_job(raw: &RawJob, seen_at: i64) -> Option<Normalized> {
    let title = normalize::clean_title(raw.title.trim());
    if normalize::is_missing(&title) || title.chars().count() < 2 {
        return None;
    }
    let stated =
        |v: &Option<String>, max: usize| v.as_deref().and_then(|t| normalize::stated(t, max));
    let company = stated(&raw.company, 120);
    let company_key = company
        .as_deref()
        .map(normalize::company_key)
        .filter(|k| !k.is_empty());
    let location = stated(&raw.location, 200);
    let place = location
        .as_deref()
        .map(normalize::place)
        .unwrap_or_default();
    let work_mode = stated(&raw.work_mode, 60)
        .and_then(|w| normalize::work_mode(&w))
        .or_else(|| location.as_deref().and_then(normalize::work_mode))
        .or_else(|| normalize::work_mode(&raw.title));
    let employment_type = stated(&raw.employment_type, 60)
        .and_then(|t| normalize::employment_type(&t))
        .or_else(|| normalize::employment_type(&title));
    let seniority = stated(&raw.seniority, 60)
        .and_then(|s| normalize::seniority(&s))
        .or_else(|| normalize::seniority(&title));
    let salary_text = stated(&raw.salary, 120);
    let salary = salary_text.as_deref().and_then(normalize::salary);
    let url = raw.url.as_deref().and_then(normalize::web_url);
    let source =
        stated(&raw.source, 60).or_else(|| url.as_deref().and_then(normalize::source_name));
    let reference = stated(&raw.reference, 60);
    let description = raw
        .description
        .as_deref()
        .map(|d| normalize::clip_lines(d, 4_000))
        .filter(|d| !normalize::is_missing(d));

    let mut keys = Vec::new();
    let mut probes = Vec::new();
    let mut markers = Vec::new();
    let title_key = words_key(&title);
    if let Some(url) = &url {
        if let Some((board, id)) = normalize::platform_id(url) {
            keys.push(format!("id:{board}:{id}"));
        }
        if let Some(canonical) = normalize::canonical_url(url) {
            // A general careers page is shared by many jobs: only the page
            // together with the title identifies one. Both forms are known.
            let with_title = format!("url:{canonical}#{title_key}");
            if normalize::is_job_specific(url) {
                keys.push(format!("url:{canonical}"));
                probes.push(with_title);
            } else {
                keys.push(with_title);
            }
        }
    }
    if let Some(company_key) = &company_key {
        if let Some(reference) = &reference {
            keys.push(format!("ref:{company_key}|{}", words_key(reference)));
        }
        if let Some(key) = description
            .as_deref()
            .and_then(|d| description_key(company_key, d))
        {
            keys.push(key);
        }
        // Company + title + place. The same title in two cities of one
        // country stays two jobs; a listing that only names the country
        // matches a listing in one of its cities.
        let ctl = |place: &str| format!("ctl:{company_key}|{title_key}|{place}");
        let country_level =
            |country: &str| format!("ctl-country:{company_key}|{title_key}|{country}");
        match (place.city(), place.country()) {
            (Some(city), country) => {
                keys.push(ctl(&city.to_lowercase()));
                if let Some(country) = country {
                    markers.push(country_level(&country.to_lowercase()));
                    probes.push(ctl(&country.to_lowercase()));
                }
            }
            (None, Some(country)) => {
                keys.push(ctl(&country.to_lowercase()));
                probes.push(country_level(&country.to_lowercase()));
            }
            (None, None) => keys.push(ctl(&location
                .as_deref()
                .map(words_key)
                .unwrap_or_else(|| "?".into()))),
        }
    }

    let mut requirements = extract::from_items(&raw.requirements);
    if let Some(description) = &description {
        requirements.extend(extract::from_description(description).requirements);
    }
    let details = if url.as_deref().is_some_and(normalize::is_job_specific) || description.is_some()
    {
        DetailsStatus::Pending
    } else {
        DetailsStatus::Skipped
    };
    Some(Normalized {
        fields: JobFields {
            normalized_title: normalize::role(&title),
            title,
            company,
            company_key,
            city: place.city().map(str::to_string),
            country: place.country().map(str::to_string),
            location,
            work_mode,
            employment_type,
            seniority,
            salary_min: salary.as_ref().and_then(|s| s.min),
            salary_max: salary.as_ref().and_then(|s| s.max),
            salary_currency: salary.as_ref().and_then(|s| s.currency.map(str::to_string)),
            salary_period: salary.as_ref().and_then(|s| s.period),
            salary_text: salary.as_ref().and(salary_text),
            date_posted: raw
                .posted
                .as_deref()
                .and_then(|p| normalize::posted(p, seen_at)),
            seen_at,
            source,
            source_url: url,
            external_ref: reference,
            description,
            details: Some(details),
        },
        keys,
        probes,
        markers,
        requirements: extract::dedupe(requirements),
    })
}

pub fn requirement_fields(reqs: &[extract::NewRequirement]) -> Vec<RequirementFields<'_>> {
    reqs.iter()
        .map(|r| RequirementFields {
            kind: r.kind,
            category: r.category,
            name: &r.name,
            original: &r.original,
            importance: r.importance,
            years: r.years,
            level: r.level.as_deref(),
            source: r.source,
        })
        .collect()
}

/// Stores a search run. Returns `None` (and removes an earlier run with the
/// same origin) when it lists no valid job.
pub fn ingest(state: &AppState, search: RawJobSearch) -> AppResult<Option<IngestOutcome>> {
    let now = now_ms();
    let title = normalize::clip(&search.title, 120);
    let query = normalize::clip(&search.query, MAX_QUERY_CHARS);
    let outcome = state.db.call(|conn| {
        let tx = conn.transaction()?;
        let run_id = repo::upsert_run(
            &tx,
            &RunFields {
                origin: &search.origin,
                title: if title.is_empty() {
                    "Job search"
                } else {
                    &title
                },
                query: &query,
                source: search.source,
                task_id: search.task_id,
                conversation_id: search.conversation_id,
                message_id: search.message_id,
                execution_id: search.execution_id,
                created_at: search.created_at,
            },
        )?;
        let mut new_jobs = 0;
        for (position, raw) in search.jobs.iter().take(MAX_JOBS_PER_RUN).enumerate() {
            let Some(mut job) = normalize_job(raw, search.created_at) else {
                continue;
            };
            if job.keys.is_empty() {
                // Neither company nor link: distinct within this run only.
                job.keys.push(format!("row:{}|{position}", search.origin));
            }
            let job_id = match repo::find_by_keys(&tx, &job.lookup())? {
                Some(id) => {
                    repo::fill_job(&tx, id, &job.fields, now)?;
                    id
                }
                None => {
                    new_jobs += 1;
                    repo::insert_job(&tx, &job.fields, now)?
                }
            };
            repo::add_keys(&tx, job_id, &job.stored())?;
            repo::add_requirements(&tx, job_id, &requirement_fields(&job.requirements))?;
            let raw_json =
                serde_json::to_string(raw).map_err(|e| AppError::internal(e.to_string()))?;
            repo::add_member(&tx, run_id, job_id, position, &raw_json)?;
        }
        let jobs = repo::finish_run(&tx, run_id)?;
        if jobs == 0 {
            tx.execute("DELETE FROM job_search_runs WHERE id = ?1", [run_id])?;
        }
        repo::delete_orphans(&tx)?;
        tx.commit()?;
        Ok((jobs > 0).then_some(IngestOutcome {
            run_id,
            jobs,
            new_jobs,
        }))
    })?;
    state.analytics.changed();
    state.analytics.wake();
    state.events.analytics_changed();
    Ok(outcome)
}

fn chat_search(answer: &repo::AnswerRow, jobs: Vec<RawJob>, source: RunSource) -> RawJobSearch {
    let prompt = answer.prompt.clone().unwrap_or_default();
    RawJobSearch {
        origin: format!("chat:{}", answer.message_id),
        title: if prompt.trim().is_empty() {
            "Chat answer".into()
        } else {
            make_title(&prompt)
        },
        query: prompt,
        source,
        task_id: None,
        conversation_id: Some(answer.conversation_id),
        message_id: Some(answer.message_id),
        execution_id: None,
        created_at: answer.created_at,
        jobs,
    }
}

fn task_search(row: &repo::TaskResultRow, jobs: Vec<RawJob>, source: RunSource) -> RawJobSearch {
    RawJobSearch {
        origin: format!("task:{}", row.execution_id),
        title: row.task_name.clone(),
        query: row.prompt.clone(),
        source,
        task_id: Some(row.task_id),
        conversation_id: None,
        message_id: None,
        execution_id: Some(row.execution_id),
        created_at: row.started_at,
        jobs,
    }
}

/// After a chat answer finishes: stores the jobs its tables list. An answer
/// that was regenerated without jobs drops its earlier results.
pub fn from_chat_answer(state: &AppState, message_id: i64) -> AppResult<Option<IngestOutcome>> {
    let answer = state.db.call(|c| repo::answer(c, message_id))?;
    let jobs = table::jobs(&answer.content);
    if jobs.is_empty() {
        let removed = state.db.call(|c| {
            let Some(id) = repo::run_by_origin(c, &format!("chat:{message_id}"))? else {
                return Ok(false);
            };
            repo::delete_run(c, id)?;
            Ok(true)
        })?;
        if removed {
            state.analytics.changed();
            state.events.analytics_changed();
        }
        return Ok(None);
    }
    ingest(state, chat_search(&answer, jobs, RunSource::Chat))
}

/// After a scheduled prompt task succeeds: stores the jobs its result lists.
pub fn from_task_result(state: &AppState, execution_id: i64) -> AppResult<Option<IngestOutcome>> {
    let row = state.db.call(|c| repo::task_result(c, execution_id))?;
    let jobs = table::jobs(&row.result);
    if jobs.is_empty() {
        return Ok(None);
    }
    ingest(state, task_search(&row, jobs, RunSource::Task))
}

/// Logs ingestion failures: they must never break chat or the scheduler.
pub fn after_chat_answer(state: &AppState, message_id: i64) {
    if let Err(error) = from_chat_answer(state, message_id) {
        eprintln!("analytics: could not read jobs from answer {message_id}: {error}");
    }
}

pub fn after_task_result(state: &AppState, execution_id: i64) {
    if let Err(error) = from_task_result(state, execution_id) {
        eprintln!("analytics: could not read jobs from run {execution_id}: {error}");
    }
}

// ── History ───────────────────────────────────────────────────────────

/// Bump when parsing improves so history is read again (runs are keyed by
/// origin, so re-reading never duplicates anything).
const HISTORY_VERSION: &str = "2";
const HISTORY_KEY: &str = "analytics.history_version";

/// Makes every earlier job search available to Analytics, once.
pub fn ingest_history(state: &AppState) -> AppResult<u32> {
    let done = state.db.call(|c| settings::get_setting(c, HISTORY_KEY))?;
    if done.as_deref() == Some(HISTORY_VERSION) {
        return Ok(0);
    }
    let mut runs = 0;
    let mut after = 0;
    loop {
        let batch = state.db.call(|c| repo::answers_with_tables(c, after, 50))?;
        let Some(last) = batch.last() else { break };
        after = last.message_id;
        for answer in batch {
            let jobs = table::jobs(&answer.content);
            if !jobs.is_empty()
                && ingest(state, chat_search(&answer, jobs, RunSource::Chat))?.is_some()
            {
                runs += 1;
            }
        }
    }
    let mut after = 0;
    loop {
        let batch = state
            .db
            .call(|c| repo::task_results_with_tables(c, after, 50))?;
        let Some(last) = batch.last() else { break };
        after = last.execution_id;
        for row in batch {
            let jobs = table::jobs(&row.result);
            if !jobs.is_empty()
                && ingest(state, task_search(&row, jobs, RunSource::Task))?.is_some()
            {
                runs += 1;
            }
        }
    }
    state
        .db
        .call(|c| settings::set_setting(c, HISTORY_KEY, HISTORY_VERSION))?;
    Ok(runs)
}

// ── Explicit "Analyze" on an answer ───────────────────────────────────

const LIST_RULES: &str = r#"You copy job listings out of a text for ReMa, a career app, as JSON.

Rules:
- Only jobs the text lists. Copy every value exactly as written in the text; never add, estimate or complete anything.
- Use null for anything the text does not state.
- "skills": requirements the text lists for that job, each copied verbatim.

Return JSON only: {"jobs": [{"title": "...", "company": ..., "location": ..., "work_mode": ..., "salary": ..., "posted": ..., "url": ..., "skills": ["..."]}]}"#;

#[derive(Deserialize)]
struct ListAnswer {
    #[serde(default)]
    jobs: Vec<ListedJob>,
}

#[derive(Deserialize)]
struct ListedJob {
    #[serde(default)]
    title: String,
    company: Option<String>,
    location: Option<String>,
    work_mode: Option<String>,
    salary: Option<String>,
    posted: Option<String>,
    url: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
}

fn simplified(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Keeps only values that appear in the text verbatim.
pub fn parse_listed(answer: &str, text: &str) -> AppResult<Vec<RawJob>> {
    let parsed: ListAnswer = serde_json::from_str(crate::jobs::extract::json_object(answer)?)
        .map_err(|_| AppError::provider("the job list was not in the expected format"))?;
    let haystack = simplified(text);
    let found = |value: Option<String>| {
        value.filter(|v| {
            let v = simplified(v);
            !v.is_empty() && haystack.contains(&v)
        })
    };
    Ok(parsed
        .jobs
        .into_iter()
        .take(MAX_JOBS_PER_RUN)
        .filter_map(|job| {
            let title = found(Some(job.title))?;
            Some(RawJob {
                title,
                company: found(job.company),
                location: found(job.location),
                work_mode: found(job.work_mode),
                salary: found(job.salary),
                posted: found(job.posted),
                url: job.url.filter(|u| text.contains(u.as_str())),
                requirements: job
                    .skills
                    .into_iter()
                    .filter_map(|s| found(Some(s)))
                    .take(40)
                    .collect(),
                ..RawJob::default()
            })
        })
        .collect())
}

async fn listed_with_model(state: &AppState, text: &str) -> AppResult<Vec<RawJob>> {
    let Some(model) = providers::catalog(state)?.default_model else {
        return Err(AppError::configuration(
            "This answer has no job table. Connect a model in Settings so ReMa can read job lists written as text.",
        ));
    };
    let endpoint = providers::resolve_endpoint(state, &model.provider_id).await?;
    let request = ChatRequest {
        system: Some(LIST_RULES.into()),
        turns: vec![Turn {
            role: MessageRole::User,
            content: format!("Text:\n\n{}", text.chars().take(20_000).collect::<String>()),
        }],
        max_output_tokens: Some(8_000),
    };
    let mut answer = String::new();
    let mut sink = |delta: &str| answer.push_str(delta);
    let finish = state
        .llm
        .stream_chat(
            &endpoint,
            &model.model_id,
            &request,
            CancellationToken::new(),
            &mut sink,
        )
        .await?;
    if finish == Finish::Cancelled {
        return Err(AppError::validation("Reading the answer was cancelled."));
    }
    parse_listed(&answer, text)
}

/// The user asked to analyze an answer: its table, or (with a model) a job
/// list written as text. Returns the run id.
pub async fn analyze_answer(state: &AppState, message_id: i64) -> AppResult<i64> {
    let answer = state.db.call(|c| repo::answer(c, message_id))?;
    let mut jobs = table::jobs(&answer.content);
    let mut source = RunSource::Chat;
    if jobs.is_empty() {
        jobs = listed_with_model(state, &answer.content).await?;
        source = RunSource::Manual;
    }
    ingest(state, chat_search(&answer, jobs, source))?
        .map(|o| o.run_id)
        .ok_or_else(|| AppError::validation("ReMa found no job listings in this answer."))
}

/// The user asked to analyze a scheduled task's result.
pub async fn analyze_task_result(state: &AppState, execution_id: i64) -> AppResult<i64> {
    let row = state.db.call(|c| repo::task_result(c, execution_id))?;
    let mut jobs = table::jobs(&row.result);
    let mut source = RunSource::Task;
    if jobs.is_empty() {
        jobs = listed_with_model(state, &row.result).await?;
        source = RunSource::Manual;
    }
    ingest(state, task_search(&row, jobs, source))?
        .map(|o| o.run_id)
        .ok_or_else(|| AppError::validation("ReMa found no job listings in this result."))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing};

    fn job(title: &str, company: &str, url: Option<&str>) -> RawJob {
        RawJob {
            title: title.into(),
            company: Some(company.into()),
            location: Some("Vienna, Austria".into()),
            url: url.map(str::to_string),
            requirements: vec!["Python".into(), "K8s".into()],
            ..RawJob::default()
        }
    }

    fn search(origin: &str, created_at: i64, jobs: Vec<RawJob>) -> RawJobSearch {
        RawJobSearch {
            origin: origin.into(),
            title: format!("Search {origin}"),
            query: "AI jobs in Austria".into(),
            source: RunSource::Tool,
            task_id: None,
            conversation_id: None,
            message_id: None,
            execution_id: None,
            created_at,
            jobs,
        }
    }

    fn count(state: &AppState, sql: &str) -> i64 {
        state
            .db
            .call(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    #[test]
    fn deduplicates_jobs_across_runs_and_keeps_membership() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let a = "https://boards.greenhouse.io/companya/jobs/5512345";
        // Search A and B report the same job, B through a tracking URL and
        // a slightly different title; C finds it via company + title + place.
        let first = ingest(
            &state,
            search(
                "t:1",
                1_000,
                vec![job("AI Engineer (m/w/d)", "Company A GmbH", Some(a))],
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!((first.jobs, first.new_jobs), (1, 1));
        let second = ingest(
            &state,
            search(
                "t:2",
                2_000,
                vec![
                    job(
                        "AI Engineer",
                        "Company A",
                        Some(&format!("{a}?gh_src=x&utm_source=y")),
                    ),
                    job("Data Engineer", "Globex", None),
                ],
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!((second.jobs, second.new_jobs), (2, 1));
        let third = ingest(
            &state,
            search(
                "t:3",
                3_000,
                vec![job("AI Engineer", "company a gmbh", None)],
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(third.new_jobs, 0);

        assert_eq!(count(&state, "SELECT COUNT(*) FROM jobs"), 2);
        assert_eq!(count(&state, "SELECT COUNT(*) FROM job_search_runs"), 3);
        assert_eq!(
            count(
                &state,
                "SELECT COUNT(*) FROM job_search_run_jobs WHERE job_id = 1"
            ),
            3,
            "the job keeps its membership in all three runs"
        );
        // Discovery is the first sighting; requirements are normalized once.
        assert_eq!(
            count(&state, "SELECT date_discovered FROM jobs WHERE id = 1"),
            1_000
        );
        assert_eq!(
            count(&state, "SELECT last_seen FROM jobs WHERE id = 1"),
            3_000
        );
        assert_eq!(
            count(
                &state,
                "SELECT COUNT(*) FROM job_requirements WHERE job_id = 1 AND name = 'Kubernetes'"
            ),
            1
        );
    }

    #[test]
    fn a_regenerated_answer_replaces_its_own_results() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        ingest(
            &state,
            search(
                "chat:5",
                1,
                vec![job("AI Engineer", "A", None), job("ML Engineer", "B", None)],
            ),
        )
        .unwrap();
        ingest(
            &state,
            search("chat:5", 1, vec![job("Data Scientist", "C", None)]),
        )
        .unwrap();
        assert_eq!(count(&state, "SELECT COUNT(*) FROM job_search_runs"), 1);
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM jobs"),
            1,
            "orphaned jobs are removed"
        );
        // Nothing valid: the run disappears.
        assert_eq!(
            ingest(
                &state,
                search(
                    "chat:5",
                    1,
                    vec![RawJob {
                        title: "—".into(),
                        ..RawJob::default()
                    }]
                )
            )
            .unwrap(),
            None
        );
        assert_eq!(count(&state, "SELECT COUNT(*) FROM job_search_runs"), 0);
    }

    #[test]
    fn missing_values_stay_missing() {
        let n = normalize_job(
            &RawJob {
                title: "ML Engineer".into(),
                company: Some("Globex".into()),
                location: Some("Remote (EU)".into()),
                salary: Some("Not stated".into()),
                posted: Some("n/a".into()),
                seniority: Some("—".into()),
                ..RawJob::default()
            },
            0,
        )
        .unwrap();
        let f = n.fields;
        assert_eq!(
            (f.salary_min, f.salary_max, f.salary_currency, f.salary_text),
            (None, None, None, None)
        );
        assert_eq!(
            (f.date_posted, f.seniority, f.country, f.city),
            (None, None, None, None)
        );
        assert_eq!(
            f.work_mode,
            Some(crate::models::analytics::WorkMode::Remote)
        );
        assert_eq!(
            f.details,
            Some(DetailsStatus::Skipped),
            "nothing to read without a link"
        );
    }

    #[test]
    fn matches_country_only_listings_but_keeps_cities_apart() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let at = |location: &str| RawJob {
            location: Some(location.into()),
            requirements: vec![],
            ..job("Software Engineer", "BigCorp", None)
        };
        ingest(
            &state,
            search("t:1", 1, vec![at("Vienna, Austria"), at("Graz, Austria")]),
        )
        .unwrap();
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM jobs"),
            2,
            "two offices, two jobs"
        );
        ingest(&state, search("t:2", 2, vec![at("Austria (Remote)")])).unwrap();
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM jobs"),
            2,
            "the country-only listing matches"
        );
        // The other order: a country-only listing first, then a city.
        ingest(
            &state,
            search(
                "t:3",
                3,
                vec![RawJob {
                    location: Some("Germany".into()),
                    ..job("Data Engineer", "Initech", None)
                }],
            ),
        )
        .unwrap();
        ingest(
            &state,
            search(
                "t:4",
                4,
                vec![RawJob {
                    location: Some("Berlin, Germany".into()),
                    ..job("Data Engineer", "Initech", None)
                }],
            ),
        )
        .unwrap();
        assert_eq!(count(&state, "SELECT COUNT(*) FROM jobs"), 3);
    }

    #[test]
    fn older_url_keys_still_match() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        // Stored under a title-qualified URL key (e.g. by an older version).
        ingest(
            &state,
            search(
                "t:1",
                1,
                vec![job(
                    "AI Engineer",
                    "A",
                    Some("https://a.com/jobs/ai-engineer"),
                )],
            ),
        )
        .unwrap();
        state
            .db
            .call(|c| {
                c.execute(
                    "UPDATE job_keys SET key = key || '#ai engineer' WHERE key LIKE 'url:%'",
                    [],
                )?;
                c.execute("DELETE FROM job_keys WHERE key LIKE 'ctl%'", [])?;
                Ok(())
            })
            .unwrap();
        ingest(
            &state,
            search(
                "t:2",
                2,
                vec![job(
                    "AI Engineer",
                    "A",
                    Some("https://a.com/jobs/ai-engineer?utm_source=x"),
                )],
            ),
        )
        .unwrap();
        assert_eq!(count(&state, "SELECT COUNT(*) FROM jobs"), 1);
    }

    #[test]
    fn listings_without_company_or_link_are_kept() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let bare = RawJob {
            title: "Data Engineer".into(),
            ..RawJob::default()
        };
        let outcome = ingest(&state, search("t:1", 1, vec![bare.clone(), bare]))
            .unwrap()
            .unwrap();
        assert_eq!(outcome.jobs, 2, "nothing proves they are the same job");
    }

    #[test]
    fn general_career_pages_do_not_merge_different_jobs() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let careers = "https://initech.com/careers";
        ingest(
            &state,
            search(
                "t:1",
                1,
                vec![
                    RawJob {
                        location: None,
                        ..job("AI Engineer", "Initech", Some(careers))
                    },
                    RawJob {
                        location: None,
                        ..job("Data Scientist", "Initech", Some(careers))
                    },
                ],
            ),
        )
        .unwrap();
        assert_eq!(count(&state, "SELECT COUNT(*) FROM jobs"), 2);
    }

    #[test]
    fn listed_jobs_keep_only_values_found_in_the_text() {
        let text = "1. **AI Engineer** at Company A, Vienna — €100k — https://a.com/jobs/ai-12345\n2. Data Scientist at Globex";
        let answer = r#"{"jobs": [
            {"title": "AI Engineer", "company": "Company A", "location": "Vienna", "salary": "€100k", "url": "https://a.com/jobs/ai-12345", "skills": ["Python"]},
            {"title": "Data Scientist", "company": "Globex", "location": "Berlin", "salary": "€90k", "url": null},
            {"title": "Invented Role", "company": "Nowhere"}
        ]}"#;
        let jobs = parse_listed(answer, text).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].salary.as_deref(), Some("€100k"));
        assert!(
            jobs[0].requirements.is_empty(),
            "\"Python\" is not in the text"
        );
        assert_eq!(
            (jobs[1].location.as_deref(), jobs[1].salary.as_deref()),
            (None, None)
        );
    }
}
