//! The deterministic job analytics engine.
//!
//! ```text
//! job searches (chat, scheduled tasks, "Analyze", future tools)
//!   → ingest   validation, normalization, deduplication, run membership
//!   → enrich   background: job page (JSON-LD), description, requirements
//!   → dataset  scope → filters → ranking → top N          (all in Rust)
//!   → views    jobs table · skill gap · unique requirements · learning
//! ```
//!
//! Every count, percentage, ranking and priority is computed here from
//! stored records. The model is only used to extract requirements from
//! descriptions and to research learning resources; both answers are
//! validated before they are stored.

pub mod dataset;
pub mod enrich;
pub mod extract;
pub mod filter;
#[cfg(test)]
pub mod fixtures;
pub mod gap;
pub mod ingest;
pub mod learning;
pub mod matching;
pub mod normalize;
pub mod overview;
pub mod page;
pub mod rank;
pub mod skills;
pub mod table;
pub mod unique;

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::{
    db::analytics as repo,
    error::{AppError, AppResult},
    llm::{ChatRequest, Finish, Turn},
    models::{
        analytics::{
            AnalyticsOverview, AnalyticsPreferences, AnalyticsQuery, DashboardTab, JobColumn,
            JobSearchRun, LearningCriteria, LearningView, PipelineStatus, RequirementTableQuery,
            RequirementsView, ResearchStatus, SkillGapOptions, SkillGapView, SortCriterion,
            SortDirection, SortKey,
        },
        chat::MessageRole,
        provider::ModelRef,
    },
    services::{profile, providers},
    state::AppState,
    time::now_ms,
};
use dataset::Data;
use matching::Evidence;

/// Background options, read from the saved preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub read_pages: bool,
    pub use_model: bool,
}

/// Job data with the data version it was loaded at.
type Cached = Option<(u64, Arc<Data>)>;

/// Shared analytics state: the cached job data, the details worker's
/// wake-up signal and its settings.
#[derive(Clone, Default)]
pub struct AnalyticsContext {
    version: Arc<AtomicU64>,
    cache: Arc<Mutex<Cached>>,
    pub(crate) wake: Arc<Notify>,
    pub(crate) shutdown: CancellationToken,
    options: Arc<Mutex<Option<Options>>>,
    last_event: Arc<Mutex<Option<Instant>>>,
    client: Arc<OnceLock<reqwest::Client>>,
}

impl AnalyticsContext {
    /// Job data changed: the next query reloads it.
    pub fn changed(&self) {
        self.version.fetch_add(1, Ordering::SeqCst);
    }

    /// New jobs wait for their details.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    pub fn stop(&self) {
        self.shutdown.cancel();
    }

    fn data(&self, state: &AppState) -> AppResult<Arc<Data>> {
        let version = self.version.load(Ordering::SeqCst);
        if let Some((v, data)) = self.cache.lock().unwrap().as_ref() {
            if *v == version {
                return Ok(data.clone());
            }
        }
        let data = Arc::new(state.db.call(|c| dataset::load(c))?);
        *self.cache.lock().unwrap() = Some((version, data.clone()));
        Ok(data)
    }

    pub fn options(&self, state: &AppState) -> AppResult<Options> {
        if let Some(o) = *self.options.lock().unwrap() {
            return Ok(o);
        }
        let prefs = preferences(state)?;
        let o = Options {
            read_pages: prefs.read_pages,
            use_model: prefs.use_model,
        };
        *self.options.lock().unwrap() = Some(o);
        Ok(o)
    }

    /// Tells the dashboard about background progress, at most every 1.5 s
    /// (and always once the queue is empty).
    pub fn notify_changed(&self, state: &AppState) {
        let idle = state
            .db
            .call(|c| repo::count_pending(c))
            .map_or(true, |n| n == 0);
        let mut last = self.last_event.lock().unwrap();
        if idle || last.is_none_or(|t| t.elapsed() >= Duration::from_millis(1_500)) {
            *last = Some(Instant::now());
            state.events.analytics_changed();
        }
    }

    fn client(&self, state: &AppState) -> &reqwest::Client {
        self.client
            .get_or_init(|| page::client(&state.info.version))
    }

    /// Local job pages are only read in development builds, when explicitly
    /// allowed (for end-to-end tests against a local test site).
    fn allow_private() -> bool {
        cfg!(debug_assertions) && std::env::var_os("REMA_DEV_ALLOW_LOCAL_PAGES").is_some()
    }

    pub async fn fetch_page(&self, state: &AppState, url: &str) -> AppResult<String> {
        page::fetch(self.client(state), url, Self::allow_private()).await
    }

    /// Requirements the default model finds in a description, validated.
    /// `None` when no model is set up.
    pub async fn model_requirements(
        &self,
        state: &AppState,
        description: &str,
    ) -> AppResult<Option<(Vec<extract::NewRequirement>, usize)>> {
        let text: String = description.chars().take(extract::MAX_MODEL_INPUT).collect();
        let Some((answer, _)) = ask_default_model(
            state,
            extract::RULES,
            format!("Job description:\n\n{text}"),
            4_000,
        )
        .await?
        else {
            return Ok(None);
        };
        extract::parse_model(&answer, &text).map(Some)
    }
}

/// Asks the default model once; `None` when no default model is set.
pub async fn ask_default_model(
    state: &AppState,
    system: &str,
    content: String,
    max_output_tokens: u32,
) -> AppResult<Option<(String, ModelRef)>> {
    let Some(model) = providers::catalog(state)?.default_model else {
        return Ok(None);
    };
    let endpoint = providers::resolve_endpoint(state, &model.provider_id).await?;
    let limit = providers::max_output_tokens(state, &model)?
        .map_or(max_output_tokens, |l| l.min(max_output_tokens));
    let request = ChatRequest {
        system: Some(system.into()),
        turns: vec![Turn {
            role: MessageRole::User,
            content,
        }],
        max_output_tokens: Some(limit),
    };
    let mut answer = String::new();
    let mut sink = |delta: &str| answer.push_str(delta);
    let cancel = state.analytics.shutdown.child_token();
    match state
        .llm
        .stream_chat(&endpoint, &model.model_id, &request, cancel, &mut sink)
        .await?
    {
        Finish::Cancelled => Err(AppError::validation("ReMa is closing.")),
        _ => Ok(Some((answer, model))),
    }
}

// ── Preferences ───────────────────────────────────────────────────────

pub fn default_preferences() -> AnalyticsPreferences {
    AnalyticsPreferences {
        query: AnalyticsQuery {
            ranking: vec![
                SortCriterion {
                    key: SortKey::DatePosted,
                    direction: SortDirection::Desc,
                    value: None,
                },
                SortCriterion {
                    key: SortKey::DateDiscovered,
                    direction: SortDirection::Desc,
                    value: None,
                },
            ],
            ..AnalyticsQuery::default()
        },
        columns: vec![
            JobColumn::Rank,
            JobColumn::Company,
            JobColumn::Role,
            JobColumn::Location,
            JobColumn::WorkMode,
            JobColumn::Salary,
            JobColumn::Match,
            JobColumn::Posted,
        ],
        tab: DashboardTab::Jobs,
        gap: SkillGapOptions::default(),
        learning: LearningCriteria::default(),
        read_pages: true,
        use_model: true,
    }
}

pub fn preferences(state: &AppState) -> AppResult<AnalyticsPreferences> {
    Ok(state
        .db
        .call(|c| repo::get_preferences(c))?
        .unwrap_or_else(default_preferences))
}

pub fn save_preferences(state: &AppState, prefs: AnalyticsPreferences) -> AppResult<()> {
    dataset::validate(&prefs.query)?;
    if prefs.columns.len() > 20 || prefs.learning.focus.len() > 50 {
        return Err(AppError::validation("Too many columns or focus items."));
    }
    let before = state.analytics.options(state)?;
    state
        .db
        .call(|c| repo::save_preferences(c, &prefs, now_ms()))?;
    let after = Options {
        read_pages: prefs.read_pages,
        use_model: prefs.use_model,
    };
    *state.analytics.options.lock().unwrap() = Some(after);
    if after != before && (after.read_pages || after.use_model) {
        // Jobs skipped while reading was off get another chance.
        state.db.call(|c| repo::requeue_unread(c))?;
        state.analytics.wake();
    }
    Ok(())
}

// ── Queries ───────────────────────────────────────────────────────────

fn evidence(state: &AppState) -> AppResult<Evidence> {
    let view = profile::get(state)?;
    Ok(Evidence::from_profile(
        &view.profile,
        &view.documents,
        matching::today(),
    ))
}

fn today() -> i64 {
    normalize::start_of_day(now_ms())
}

pub fn status(state: &AppState) -> AppResult<PipelineStatus> {
    let (jobs, runs) = state.db.call(|c| repo::counts(c))?;
    let pending = state.db.call(|c| repo::count_pending(c))?;
    let options = state.analytics.options(state)?;
    Ok(PipelineStatus {
        jobs,
        runs,
        pending_details: pending,
        read_pages: options.read_pages,
        use_model: options.use_model,
        model: providers::catalog(state)?.default_model.map(|m| m.model_id),
    })
}

pub fn runs(state: &AppState) -> AppResult<Vec<JobSearchRun>> {
    Ok(state.analytics.data(state)?.runs.clone())
}

pub fn delete_run(state: &AppState, id: i64) -> AppResult<()> {
    state.db.call(|c| {
        let tx = c.transaction()?;
        repo::delete_run(&tx, id)?;
        tx.commit()?;
        Ok(())
    })?;
    state.analytics.changed();
    state.events.analytics_changed();
    Ok(())
}

pub fn overview(state: &AppState, query: &AnalyticsQuery) -> AppResult<AnalyticsOverview> {
    let data = state.analytics.data(state)?;
    let evidence = evidence(state)?;
    let ds = dataset::select(&data, query, &evidence, today())?;
    let summary = overview::summary(&ds, query.scope.kind, &query.scope.run_ids);
    Ok(overview::overview(&ds, summary, status(state)?))
}

fn pending_note(state: &AppState) -> AppResult<Option<String>> {
    let pending = state.db.call(|c| repo::count_pending(c))?;
    Ok((pending > 0).then(|| {
        format!("Details of {pending} job(s) are still being read; results update automatically.")
    }))
}

pub fn skill_gap(
    state: &AppState,
    query: &AnalyticsQuery,
    options: &SkillGapOptions,
) -> AppResult<SkillGapView> {
    let data = state.analytics.data(state)?;
    let evidence = evidence(state)?;
    let ds = dataset::select(&data, query, &evidence, today())?;
    let mut notes = ds.notes.clone();
    let with = ds.with_requirements();
    if with < ds.jobs.len() {
        notes.push(format!(
            "Requirements are known for {with} of {} jobs; the others are left out of the percentages.",
            ds.jobs.len()
        ));
    }
    notes.extend(pending_note(state)?);
    Ok(gap::view(&ds, evidence.available, options, notes))
}

pub fn requirements(
    state: &AppState,
    query: &AnalyticsQuery,
    table: &RequirementTableQuery,
) -> AppResult<RequirementsView> {
    if table.search.len() > 200 {
        return Err(AppError::validation("The search is too long."));
    }
    let data = state.analytics.data(state)?;
    let evidence = evidence(state)?;
    let ds = dataset::select(&data, query, &evidence, today())?;
    Ok(unique::view(&ds, evidence.available, table))
}

pub fn learning(
    state: &AppState,
    query: &AnalyticsQuery,
    criteria: &LearningCriteria,
    options: &SkillGapOptions,
) -> AppResult<LearningView> {
    let data = state.analytics.data(state)?;
    let evidence = evidence(state)?;
    let ds = dataset::select(&data, query, &evidence, today())?;
    let gaps = if evidence.available {
        learning::gaps(&ds, criteria, options)
    } else {
        Vec::new()
    };
    let key = learning::dataset_key(query, criteria, options);
    let research = state
        .db
        .call(|c| repo::latest_research(c, &key))?
        .map(|stored| learning::to_view(stored, &key, &gaps));
    let mut notes = ds.notes.clone();
    if !evidence.available {
        notes.push(
            "Profile required for personal skill-gap comparison and learning recommendations."
                .into(),
        );
    } else if gaps.is_empty() && !ds.jobs.is_empty() {
        notes.push("No gaps match the learning criteria for these jobs.".into());
    }
    notes.extend(pending_note(state)?);
    Ok(LearningView {
        profile_available: evidence.available,
        jobs: ds.jobs.len() as u32,
        jobs_with_requirements: ds.with_requirements() as u32,
        gaps,
        research,
        model: providers::catalog(state)?.default_model.map(|m| m.model_id),
        notes,
    })
}

/// Starts learning-resource research for the current selection in the
/// background and returns the view with the research marked as running.
pub fn research(
    state: &AppState,
    query: &AnalyticsQuery,
    criteria: &LearningCriteria,
    options: &SkillGapOptions,
) -> AppResult<LearningView> {
    let model = providers::catalog(state)?.default_model.ok_or_else(|| {
        AppError::configuration(
            "Choose a default model in Settings to research learning resources.",
        )
    })?;
    if state.db.call(|c| repo::research_running(c))? {
        return Err(AppError::validation(
            "Research is already running; wait for it to finish.",
        ));
    }
    let data = state.analytics.data(state)?;
    let evidence = evidence(state)?;
    if !evidence.available {
        return Err(AppError::validation(
            "Add your skills to your Profile first, so ReMa can find your gaps.",
        ));
    }
    let ds = dataset::select(&data, query, &evidence, today())?;
    let gaps = learning::gaps(&ds, criteria, options);
    if gaps.is_empty() {
        return Err(AppError::validation(
            "There are no gaps to research for this selection.",
        ));
    }
    let key = learning::dataset_key(query, criteria, options);
    let prompt = learning::prompt(&ds, &gaps);
    let snapshot = serde_json::to_string(&gaps).map_err(|e| AppError::internal(e.to_string()))?;
    let dataset_json =
        serde_json::to_string(&(query, criteria)).map_err(|e| AppError::internal(e.to_string()))?;
    let id = state.db.call(|c| {
        repo::insert_research(
            c,
            &repo::NewResearch {
                dataset_key: &key,
                dataset_label: &ds.label,
                dataset: &dataset_json,
                job_count: ds.jobs.len() as u32,
                gap_snapshot: &snapshot,
                provider_id: &model.provider_id,
                model_id: &model.model_id,
                generated_at: now_ms(),
            },
        )
    })?;
    let background = state.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = async {
            let (answer, _) = ask_default_model(&background, learning::RULES, prompt, 8_000)
                .await?
                .ok_or_else(|| AppError::configuration("No default model is set."))?;
            let mut recommendations = learning::parse(&answer, &gaps)?;
            if recommendations.is_empty() {
                return Err(AppError::provider("the model returned no usable resources"));
            }
            let client = background.analytics.client(&background).clone();
            learning::check_links(
                &client,
                &mut recommendations,
                AnalyticsContext::allow_private(),
            )
            .await;
            serde_json::to_string(&recommendations).map_err(|e| AppError::internal(e.to_string()))
        }
        .await;
        let saved = background.db.call(|c| match &outcome {
            Ok(json) => repo::finish_research(c, id, ResearchStatus::Done, Some(json), None),
            Err(error) => learning::store_failure(c, id, error),
        });
        if let Err(error) = saved {
            eprintln!("analytics: could not store research {id}: {error}");
        }
        background.events.analytics_changed();
    });
    state.events.analytics_changed();
    learning(state, query, criteria, options)
}

#[cfg(test)]
mod tests;
