//! The analytical dataset: stored jobs → scope → filters → ranking → top N.
//!
//! Job facts are loaded from SQLite once per data version and cached; every
//! request then selects, filters, ranks and aggregates in memory, and only
//! the rows a view needs are sent to the frontend.

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use super::{filter, matching::Evidence, normalize, rank, skills};
use crate::{
    db::analytics::{self as repo, StoredJob},
    error::{AppError, AppResult},
    models::analytics::{
        AnalyticsQuery, CoverageCounts, DataScope, FilterField, FilterOp, Importance, JobSearchRun,
        MatchState, RequirementCategory, RequirementKind, ScopeKind,
    },
};

/// A stored requirement in analysis form.
#[derive(Debug, Clone, PartialEq)]
pub struct Requirement {
    pub kind: RequirementKind,
    pub category: RequirementCategory,
    pub name: String,
    /// Lowercase name, the grouping key.
    pub key: String,
    pub importance: Importance,
    pub years: Option<u32>,
    pub level: Option<String>,
    pub original: String,
}

/// Everything the analytics know about one job.
#[derive(Debug, Clone)]
pub struct JobFacts {
    pub job: StoredJob,
    pub role_key: String,
    pub countries: Vec<&'static str>,
    pub cities: Vec<&'static str>,
    pub salary: Option<normalize::Salary>,
    pub requirements: Vec<Requirement>,
    /// Search runs that found it.
    pub runs: Vec<i64>,
}

impl JobFacts {
    pub fn requires(&self, key: &str) -> bool {
        self.requirements.iter().any(|r| r.key == key)
    }

    /// Yearly salary when it is stated in `currency` (see `Salary::annual`).
    pub fn annual_salary(&self, currency: &str) -> Option<f64> {
        let s = self.salary.as_ref()?;
        (s.currency == Some(currency)).then(|| s.annual()).flatten()
    }
}

/// All stored job data, loaded once per data version.
#[derive(Debug, Default)]
pub struct Data {
    pub jobs: Vec<JobFacts>,
    pub runs: Vec<JobSearchRun>,
}

pub fn load(conn: &rusqlite::Connection) -> AppResult<Data> {
    let stored = repo::load_jobs(conn)?;
    let mut requirements: HashMap<i64, Vec<Requirement>> = HashMap::new();
    for r in repo::load_requirements(conn)? {
        requirements.entry(r.job_id).or_default().push(Requirement {
            kind: r.kind,
            category: r.category,
            key: r.name.to_lowercase(),
            name: r.name,
            importance: r.importance,
            years: r.years,
            level: r.level,
            original: r.original,
        });
    }
    let mut runs_of: HashMap<i64, Vec<i64>> = HashMap::new();
    for (run, job) in repo::load_memberships(conn)? {
        runs_of.entry(job).or_default().push(run);
    }
    let jobs = stored
        .into_iter()
        .map(|job| {
            let place = job
                .location
                .as_deref()
                .map(normalize::place)
                .unwrap_or_default();
            let mut countries = place.countries.clone();
            if let Some(c) = job.country.as_deref().and_then(normalize::country_name) {
                if !countries.contains(&c) {
                    countries.insert(0, c);
                }
            }
            let salary =
                (job.salary_min.is_some() || job.salary_max.is_some()).then(|| normalize::Salary {
                    min: job.salary_min,
                    max: job.salary_max,
                    currency: job.salary_currency.as_deref().and_then(currency_code),
                    period: job.salary_period,
                });
            JobFacts {
                role_key: job.normalized_title.to_lowercase(),
                countries,
                cities: place.cities.clone(),
                salary,
                requirements: requirements.remove(&job.id).unwrap_or_default(),
                runs: runs_of.remove(&job.id).unwrap_or_default(),
                job,
            }
        })
        .collect();
    Ok(Data {
        jobs,
        runs: repo::list_runs(conn)?,
    })
}

/// Static currency codes (salaries only store ISO codes).
pub fn currency_code(code: &str) -> Option<&'static str> {
    const CODES: &[&str] = &[
        "EUR", "USD", "GBP", "CHF", "CAD", "AUD", "NZD", "SGD", "HKD", "SEK", "NOK", "DKK", "PLN",
        "CZK", "HUF", "RON", "INR", "JPY", "CNY", "AED", "ILS",
    ];
    CODES.iter().find(|c| c.eq_ignore_ascii_case(code)).copied()
}

/// A job with its Profile comparison.
pub struct Job<'a> {
    pub facts: &'a JobFacts,
    /// Per requirement, in `facts.requirements` order.
    pub states: Vec<(MatchState, Option<String>)>,
    pub counts: CoverageCounts,
    /// (matched + ½ partial) / (matched + partial + missing); Unknown excluded.
    pub coverage: Option<f64>,
}

impl<'a> Job<'a> {
    pub fn evaluate(facts: &'a JobFacts, evidence: &Evidence) -> Self {
        let states: Vec<(MatchState, Option<String>)> = facts
            .requirements
            .iter()
            .map(|r| evidence.state(r))
            .collect();
        let mut counts = CoverageCounts {
            requirements: states.len() as u32,
            ..CoverageCounts::default()
        };
        for (state, _) in &states {
            match state {
                MatchState::Matched => counts.matched += 1,
                MatchState::Partial => counts.partial += 1,
                MatchState::Missing => counts.missing += 1,
                MatchState::Unknown => counts.unknown += 1,
            }
        }
        let known = counts.matched + counts.partial + counts.missing;
        let coverage = (evidence.available && known > 0).then(|| {
            (f64::from(counts.matched) + 0.5 * f64::from(counts.partial)) / f64::from(known) * 100.0
        });
        Job {
            facts,
            states,
            counts,
            coverage,
        }
    }

    pub fn id(&self) -> i64 {
        self.facts.job.id
    }

    pub fn state_of(&self, key: &str) -> Option<MatchState> {
        self.facts
            .requirements
            .iter()
            .position(|r| r.key == key)
            .map(|i| self.states[i].0)
    }

    /// Missing requirements, when the Profile could be compared.
    pub fn missing(&self) -> Option<u32> {
        self.coverage.map(|_| self.counts.missing)
    }
}

/// The selected, filtered, ranked jobs of one query.
pub struct Dataset<'a> {
    pub data: &'a Data,
    /// Jobs in scope, before filters (for filter suggestions).
    pub scoped: Vec<&'a JobFacts>,
    pub in_scope: usize,
    /// After filters, before the top-N limit.
    pub matching: usize,
    /// The analyzed jobs, in ranking order.
    pub jobs: Vec<Job<'a>>,
    /// Currency salary comparisons use.
    pub currency: Option<&'static str>,
    pub label: String,
    pub notes: Vec<String>,
}

impl Dataset<'_> {
    pub fn with_requirements(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| !j.facts.requirements.is_empty())
            .count()
    }
}

/// The most common currency of comparable salaries (ties: alphabetical).
fn dominant_currency<'a>(jobs: impl Iterator<Item = &'a JobFacts>) -> Option<&'static str> {
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    for job in jobs {
        if let Some(s) = &job.salary {
            if let (Some(c), Some(_)) = (s.currency, s.annual()) {
                *counts.entry(c).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(c, _)| c)
}

fn scope_label(scope: &DataScope, data: &Data) -> String {
    match scope.kind {
        ScopeKind::All => "All job searches".into(),
        ScopeKind::Searches => {
            let titles: Vec<String> = data
                .runs
                .iter()
                .filter(|r| scope.run_ids.contains(&r.id))
                .map(|r| {
                    format!(
                        "{} ({})",
                        r.title,
                        normalize::date_of(r.created_at).strftime("%-d %b")
                    )
                })
                .collect();
            match titles.as_slice() {
                [] => "No search selected".into(),
                [one] => one.clone(),
                many if many.len() <= 3 => many.join(", "),
                many => format!("{} searches", many.len()),
            }
        }
        ScopeKind::Jobs => match scope.job_ids.len() {
            1 => {
                let id = scope.job_ids[0];
                data.jobs
                    .iter()
                    .find(|j| j.job.id == id)
                    .map_or("1 selected job".into(), |j| match &j.job.company {
                        Some(c) => format!("{} at {c}", j.job.title),
                        None => j.job.title.clone(),
                    })
            }
            n => format!("{n} selected jobs"),
        },
    }
}

pub const MAX_FILTERS: usize = 20;
pub const MAX_CRITERIA: usize = 8;

pub fn validate(query: &AnalyticsQuery) -> AppResult<()> {
    if query.filters.len() > MAX_FILTERS {
        return Err(AppError::validation(format!(
            "Use at most {MAX_FILTERS} filters."
        )));
    }
    if query.ranking.len() > MAX_CRITERIA {
        return Err(AppError::validation(format!(
            "Use at most {MAX_CRITERIA} ranking criteria."
        )));
    }
    if query.scope.run_ids.len() > 1_000 || query.scope.job_ids.len() > 5_000 {
        return Err(AppError::validation("Too many items are selected."));
    }
    if query.limit == Some(0) {
        return Err(AppError::validation("Keep at least one job."));
    }
    query.filters.iter().try_for_each(filter::validate)?;
    query.ranking.iter().try_for_each(rank::validate)
}

/// Runs the query deterministically.
pub fn select<'a>(
    data: &'a Data,
    query: &AnalyticsQuery,
    evidence: &Evidence,
    today: i64,
) -> AppResult<Dataset<'a>> {
    validate(query)?;
    let mut notes = Vec::new();
    let scope = &query.scope;
    let runs: HashSet<i64> = scope.run_ids.iter().copied().collect();
    let ids: HashSet<i64> = scope.job_ids.iter().copied().collect();
    let scoped: Vec<&JobFacts> = data
        .jobs
        .iter()
        .filter(|j| match scope.kind {
            ScopeKind::All => true,
            ScopeKind::Searches => j.runs.iter().any(|r| runs.contains(r)),
            ScopeKind::Jobs => ids.contains(&j.job.id),
        })
        .collect();
    match scope.kind {
        ScopeKind::Jobs => {
            let gone = ids.len() - scoped.len();
            if gone > 0 {
                notes.push(format!("{gone} selected job(s) are no longer stored."));
            }
        }
        ScopeKind::Searches => {
            let gone = runs
                .iter()
                .filter(|id| !data.runs.iter().any(|r| r.id == **id))
                .count();
            if gone > 0 {
                notes.push(format!("{gone} selected search(es) are no longer stored."));
            }
        }
        ScopeKind::All => {}
    }

    // Salary conditions name their currency; otherwise the most common one.
    let currency = query
        .filters
        .iter()
        .find(|f| {
            f.field == FilterField::Salary && matches!(f.op, FilterOp::AtLeast | FilterOp::AtMost)
        })
        .and_then(|f| f.currency.as_deref().and_then(currency_code))
        .or_else(|| dominant_currency(scoped.iter().copied()));
    let ctx = filter::Context {
        today,
        evidence,
        currency,
    };
    let in_scope = scoped.len();
    let mut jobs: Vec<Job> = scoped
        .iter()
        .map(|facts| Job::evaluate(facts, evidence))
        .filter(|job| query.filters.iter().all(|f| filter::matches(f, job, &ctx)))
        .collect();
    let matching = jobs.len();
    jobs.sort_by(|a, b| {
        rank::compare(a, b, &query.ranking, currency)
            .then_with(|| {
                b.facts
                    .job
                    .date_discovered
                    .cmp(&a.facts.job.date_discovered)
            })
            .then_with(|| b.id().cmp(&a.id()))
    });
    if let Some(limit) = query.limit {
        jobs.truncate(limit as usize);
    }
    if !evidence.available
        && query.filters.iter().any(|f| {
            matches!(
                f.field,
                FilterField::Match | FilterField::MissingSkill | FilterField::SkillGap
            )
        })
    {
        notes.push("Profile conditions match no job until your Profile has details.".into());
    }

    let mut parts = vec![scope_label(scope, data)];
    parts.extend(
        query
            .filters
            .iter()
            .map(|f| filter::describe(f, data, currency)),
    );
    if let Some(limit) = query.limit {
        let by = query
            .ranking
            .first()
            .map(rank::describe)
            .unwrap_or_else(|| "newest".into());
        parts.push(format!("top {limit} by {by}"));
    }
    Ok(Dataset {
        data,
        scoped,
        in_scope,
        matching,
        jobs,
        currency,
        label: parts.join(" · "),
        notes,
    })
}

/// Canonical requirement key for a user-typed skill ("k8s" → "kubernetes").
pub fn skill_key(value: &str) -> String {
    match skills::lookup_item(value).as_slice() {
        [def] => def.name.to_lowercase(),
        _ => skills::language(&value.trim().to_lowercase())
            .map(str::to_lowercase)
            .unwrap_or_else(|| value.trim().to_lowercase()),
    }
}

/// Orders optional values with unknowns last, whatever the direction.
pub fn known_first<T: PartialOrd>(a: Option<T>, b: Option<T>, descending: bool) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => {
            let o = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
            if descending {
                o.reverse()
            } else {
                o
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Shared, cached job data.
pub type Shared = Arc<Data>;
