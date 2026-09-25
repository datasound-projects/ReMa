//! Skill-gap analysis over the analyzed jobs.
//!
//! Percentages use the jobs that have requirement data as denominator; jobs
//! without it are reported, never treated as "requires nothing".
//!
//! Gap priority (explicit, deterministic):
//!
//! ```text
//! weight(job)      = 1, or 1 → 0.5 from first to last rank ("weight by my ranking")
//! importance       = 1 required, 0.5 preferred
//! gap              = 1 missing, 0.5 partial
//! frequency        = Σ weight·importance·gap over jobs where it is a gap
//!                    ÷ Σ weight over jobs with requirement data
//! salary lift      = max(0, share among jobs paying ≥ the median salary
//!                           − share among all jobs)      (only with ≥ 5 comparable salaries)
//! priority         = frequency × (1 + salary lift)
//! High ≥ 0.40, Medium ≥ 0.20, Low otherwise
//! ```

use std::collections::{HashMap, HashSet};

use super::{dataset::Dataset, normalize};
use crate::models::analytics::{
    CellState, CoverageCounts, GapMatrix, GapPriority, Importance, JobGap, MatchState, MatrixRow,
    PriorityLevel, RequirementCategory, RequirementKind, RequirementState, SalaryPeriod,
    SkillDemand, SkillGapOptions, SkillGapView, StateShare,
};

pub const HIGH: f64 = 0.40;
pub const MEDIUM: f64 = 0.20;
/// Comparable salaries needed before salary affects priorities.
pub const MIN_SALARIES: usize = 5;
const MATRIX_SKILLS: usize = 10;
const MATRIX_ROWS: usize = 12;
const DEMAND_ROWS: usize = 30;

/// One requirement across the analyzed jobs.
#[derive(Debug, Clone)]
pub struct Aggregate {
    pub key: String,
    pub name: String,
    pub kind: RequirementKind,
    pub category: RequirementCategory,
    pub jobs: u32,
    pub required: u32,
    pub preferred: u32,
    /// Jobs per state: matched, partial, missing, unknown.
    pub states: [u32; 4],
    pub evidence: Option<String>,
    pub gap_jobs: u32,
    pub weighted_gap: f64,
    pub levels: HashMap<String, u32>,
    pub examples: Vec<String>,
    pub job_ids: Vec<i64>,
}

impl Aggregate {
    /// The most common state; ties go to the larger gap.
    pub fn state(&self) -> MatchState {
        let order = [
            MatchState::Missing,
            MatchState::Partial,
            MatchState::Unknown,
            MatchState::Matched,
        ];
        let count = |s: MatchState| self.states[state_index(s)];
        order
            .into_iter()
            .max_by(|a, b| {
                count(*a)
                    .cmp(&count(*b))
                    .then_with(|| rank(*b).cmp(&rank(*a)))
            })
            .unwrap_or(MatchState::Unknown)
    }

    pub fn common_level(&self) -> Option<String> {
        self.levels
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(l, _)| l.clone())
    }
}

fn state_index(s: MatchState) -> usize {
    match s {
        MatchState::Matched => 0,
        MatchState::Partial => 1,
        MatchState::Missing => 2,
        MatchState::Unknown => 3,
    }
}

/// Gap order used for ties: missing first.
fn rank(s: MatchState) -> u8 {
    match s {
        MatchState::Missing => 0,
        MatchState::Partial => 1,
        MatchState::Unknown => 2,
        MatchState::Matched => 3,
    }
}

pub struct Aggregation {
    /// Sorted by jobs (desc), then name.
    pub items: Vec<Aggregate>,
    pub with_requirements: u32,
    pub weight_total: f64,
    pub mentions: u32,
}

/// Relevance weight of the job at rank `i` of `n`.
fn weight(i: usize, n: usize, by_rank: bool) -> f64 {
    if by_rank && n > 1 {
        1.0 - 0.5 * i as f64 / (n - 1) as f64
    } else {
        1.0
    }
}

pub fn aggregate(ds: &Dataset, weight_by_rank: bool) -> Aggregation {
    let n = ds.jobs.len();
    let mut by_key: HashMap<String, Aggregate> = HashMap::new();
    let mut with_requirements = 0;
    let mut weight_total = 0.0;
    let mut mentions = 0;
    for (i, job) in ds.jobs.iter().enumerate() {
        if job.facts.requirements.is_empty() {
            continue;
        }
        with_requirements += 1;
        let w = weight(i, n, weight_by_rank);
        weight_total += w;
        for (r, (state, evidence)) in job.facts.requirements.iter().zip(&job.states) {
            mentions += 1;
            let a = by_key.entry(r.key.clone()).or_insert_with(|| Aggregate {
                key: r.key.clone(),
                name: r.name.clone(),
                kind: r.kind,
                category: r.category,
                jobs: 0,
                required: 0,
                preferred: 0,
                states: [0; 4],
                evidence: None,
                gap_jobs: 0,
                weighted_gap: 0.0,
                levels: HashMap::new(),
                examples: Vec::new(),
                job_ids: Vec::new(),
            });
            a.jobs += 1;
            a.job_ids.push(job.id());
            let importance = match r.importance {
                Importance::Required => {
                    a.required += 1;
                    1.0
                }
                Importance::Preferred => {
                    a.preferred += 1;
                    0.5
                }
            };
            a.states[state_index(*state)] += 1;
            if a.evidence.is_none() {
                a.evidence = evidence.clone();
            }
            let gap = match state {
                MatchState::Missing => 1.0,
                MatchState::Partial => 0.5,
                _ => 0.0,
            };
            if gap > 0.0 {
                a.gap_jobs += 1;
                a.weighted_gap += w * importance * gap;
            }
            let level = match (r.kind, &r.level, r.years) {
                (RequirementKind::Experience, _, Some(y)) => Some(format!("{y}+ years")),
                (_, Some(level), _) => Some(level.clone()),
                _ => None,
            };
            if let Some(level) = level {
                *a.levels.entry(level).or_default() += 1;
            }
            let example = normalize::clip(&r.original, 90);
            if a.examples.len() < 2
                && !example.eq_ignore_ascii_case(&r.name)
                && !a.examples.contains(&example)
            {
                a.examples.push(example);
            }
        }
    }
    let mut items: Vec<Aggregate> = by_key.into_values().collect();
    items.sort_by(|a, b| {
        b.jobs
            .cmp(&a.jobs)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Aggregation {
        items,
        with_requirements,
        weight_total,
        mentions,
    }
}

pub fn percent(part: u32, whole: u32) -> f64 {
    if whole == 0 {
        0.0
    } else {
        (f64::from(part) / f64::from(whole) * 1000.0).round() / 10.0
    }
}

/// Skills in the narrow sense: what charts about "skills" show.
pub fn is_skill(kind: RequirementKind) -> bool {
    matches!(
        kind,
        RequirementKind::Skill | RequirementKind::Certification | RequirementKind::Language
    )
}

/// The jobs paying at least the median comparable salary, if enough
/// salaries are known.
struct HighPay {
    jobs: HashSet<i64>,
    threshold: String,
}

fn high_pay(ds: &Dataset) -> Option<HighPay> {
    let currency = ds.currency?;
    let mut paid: Vec<(i64, f64)> = ds
        .jobs
        .iter()
        .filter(|j| !j.facts.requirements.is_empty())
        .filter_map(|j| Some((j.id(), j.facts.annual_salary(currency)?)))
        .collect();
    if paid.len() < MIN_SALARIES {
        return None;
    }
    paid.sort_by(|a, b| a.1.total_cmp(&b.1));
    let median = if paid.len() % 2 == 1 {
        paid[paid.len() / 2].1
    } else {
        (paid[paid.len() / 2 - 1].1 + paid[paid.len() / 2].1) / 2.0
    };
    Some(HighPay {
        jobs: paid
            .iter()
            .filter(|(_, v)| *v >= median)
            .map(|(id, _)| *id)
            .collect(),
        threshold: normalize::format_salary(&normalize::Salary {
            min: Some(median),
            max: Some(median),
            currency: Some(currency),
            period: Some(SalaryPeriod::Year),
        }),
    })
}

pub fn level_of(score: f64) -> PriorityLevel {
    if score >= HIGH {
        PriorityLevel::High
    } else if score >= MEDIUM {
        PriorityLevel::Medium
    } else {
        PriorityLevel::Low
    }
}

/// Every gap with its priority and the facts behind it, highest first.
pub fn priorities(ds: &Dataset, agg: &Aggregation, options: &SkillGapOptions) -> Vec<GapPriority> {
    let high = high_pay(ds);
    let total = agg.with_requirements;
    let mut out: Vec<GapPriority> = agg
        .items
        .iter()
        .filter(|a| a.gap_jobs > 0 && a.kind != RequirementKind::SoftSkill)
        .map(|a| {
            let frequency = if agg.weight_total > 0.0 {
                a.weighted_gap / agg.weight_total
            } else {
                0.0
            };
            let share = f64::from(a.jobs) / f64::from(total.max(1));
            let mut reasons = vec![if a.preferred == 0 {
                format!(
                    "Required by {} of {} jobs ({}%)",
                    a.jobs,
                    total,
                    percent(a.jobs, total).round()
                )
            } else if a.required == 0 {
                format!(
                    "Preferred (not required) by {} of {} jobs ({}%)",
                    a.jobs,
                    total,
                    percent(a.jobs, total).round()
                )
            } else {
                format!(
                    "Asked for by {} of {} jobs ({}%): required by {}, preferred by {}",
                    a.jobs,
                    total,
                    percent(a.jobs, total).round(),
                    a.required,
                    a.preferred
                )
            }];
            let state = a.state();
            reasons.push(match state {
                MatchState::Missing => "Missing from your Profile".into(),
                MatchState::Partial => match &a.evidence {
                    Some(e) => format!("Only partly covered by your Profile ({e})"),
                    None => "Only partly covered by your Profile".into(),
                },
                _ if a.gap_jobs < a.jobs => format!("A gap in {} of these jobs", a.gap_jobs),
                _ => "A gap in these jobs".into(),
            });
            let mut lift = 0.0;
            if let Some(h) = &high {
                let in_high = a.job_ids.iter().filter(|id| h.jobs.contains(id)).count() as u32;
                let h_total = h.jobs.len() as u32;
                let share_high = f64::from(in_high) / f64::from(h_total.max(1));
                lift = (share_high - share).max(0.0);
                reasons.push(format!(
                    "{in_high} of the {h_total} jobs paying {} or more ask for it ({}%)",
                    h.threshold,
                    percent(in_high, h_total).round()
                ));
            }
            if options.weight_by_rank {
                reasons.push("Higher-ranked jobs count more (weighted by your ranking)".into());
            }
            let score = (frequency * (1.0 + lift) * 1000.0).round() / 1000.0;
            GapPriority {
                name: a.name.clone(),
                kind: a.kind,
                category: a.category,
                state: if matches!(state, MatchState::Missing | MatchState::Partial) {
                    state
                } else if a.states[2] > 0 {
                    MatchState::Missing
                } else {
                    MatchState::Partial
                },
                score,
                level: level_of(score),
                jobs: a.jobs,
                percent: percent(a.jobs, total),
                reasons,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.jobs.cmp(&a.jobs))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

fn cell(state: Option<MatchState>, has_data: bool) -> CellState {
    match state {
        Some(MatchState::Matched) => CellState::Matched,
        Some(MatchState::Partial) => CellState::Partial,
        Some(MatchState::Missing) => CellState::Missing,
        Some(MatchState::Unknown) => CellState::Unknown,
        None if has_data => CellState::NotRequired,
        None => CellState::NoData,
    }
}

fn names(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_string(),
        many => many.join(", "),
    }
}

pub fn view(
    ds: &Dataset,
    evidence_available: bool,
    options: &SkillGapOptions,
    notes: Vec<String>,
) -> SkillGapView {
    let agg = aggregate(ds, options.weight_by_rank);
    let total = agg.with_requirements;
    let jobs = ds.jobs.len() as u32;

    let demand_items: Vec<&Aggregate> = agg.items.iter().filter(|a| is_skill(a.kind)).collect();
    let demand: Vec<SkillDemand> = demand_items
        .iter()
        .take(DEMAND_ROWS)
        .map(|a| SkillDemand {
            name: a.name.clone(),
            kind: a.kind,
            category: a.category,
            jobs: a.jobs,
            percent: percent(a.jobs, total),
            state: evidence_available.then(|| a.state()),
            evidence: a.evidence.clone(),
        })
        .collect();

    let mut coverage = Vec::new();
    if evidence_available {
        for state in [
            MatchState::Matched,
            MatchState::Partial,
            MatchState::Missing,
            MatchState::Unknown,
        ] {
            let i = state_index(state);
            let mentions: u32 = agg.items.iter().map(|a| a.states[i]).sum();
            coverage.push(StateShare {
                state,
                requirements: agg.items.iter().filter(|a| a.state() == state).count() as u32,
                mentions,
                percent: percent(mentions, agg.mentions),
            });
        }
    }

    let skills: Vec<&Aggregate> = demand_items.iter().take(MATRIX_SKILLS).copied().collect();
    let matrix = GapMatrix {
        skills: skills.iter().map(|a| a.name.clone()).collect(),
        gap_jobs: skills.iter().map(|a| a.gap_jobs).collect(),
        rows: ds
            .jobs
            .iter()
            .take(MATRIX_ROWS)
            .map(|job| MatrixRow {
                job_id: job.id(),
                title: job.facts.job.title.clone(),
                company: job.facts.job.company.clone(),
                coverage: job.coverage.map(f64::round),
                cells: skills
                    .iter()
                    .map(|a| {
                        let state = job.state_of(&a.key);
                        let has_data = !job.facts.requirements.is_empty();
                        if !evidence_available && state.is_some() {
                            CellState::Unknown
                        } else {
                            cell(state, has_data)
                        }
                    })
                    .collect(),
            })
            .collect(),
        more_rows: ds.jobs.len().saturating_sub(MATRIX_ROWS) as u32,
    };

    let priorities = if evidence_available {
        priorities(ds, &agg, options)
    } else {
        Vec::new()
    };

    let mut counts = CoverageCounts::default();
    for j in &ds.jobs {
        counts.requirements += j.counts.requirements;
        counts.matched += j.counts.matched;
        counts.partial += j.counts.partial;
        counts.missing += j.counts.missing;
        counts.unknown += j.counts.unknown;
    }
    let coverages: Vec<f64> = ds.jobs.iter().filter_map(|j| j.coverage).collect();
    let average_coverage = (!coverages.is_empty())
        .then(|| (coverages.iter().sum::<f64>() / coverages.len() as f64 * 10.0).round() / 10.0);

    let job = (ds.jobs.len() == 1).then(|| {
        let j = &ds.jobs[0];
        let mut requirements: Vec<RequirementState> = j
            .facts
            .requirements
            .iter()
            .zip(&j.states)
            .map(|(r, (state, evidence))| RequirementState {
                name: r.name.clone(),
                kind: r.kind,
                category: r.category,
                importance: r.importance,
                state: *state,
                evidence: evidence.clone(),
                original: r.original.clone(),
            })
            .collect();
        requirements.sort_by(|a, b| {
            rank(a.state)
                .cmp(&rank(b.state))
                .then_with(|| {
                    (a.importance == Importance::Preferred)
                        .cmp(&(b.importance == Importance::Preferred))
                })
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        JobGap {
            job_id: j.id(),
            title: j.facts.job.title.clone(),
            company: j.facts.job.company.clone(),
            coverage: j.coverage.map(f64::round),
            counts: j.counts.clone(),
            requirements,
        }
    });

    // Deterministic summary: every number comes from the aggregation above.
    let mut summary = Vec::new();
    summary.push(if total < jobs {
        format!("Selected jobs: {jobs} · requirements known for {total}")
    } else {
        format!("Selected jobs: {jobs}")
    });
    if evidence_available {
        if let Some(avg) = average_coverage {
            summary.push(format!("Average skill coverage: {}%", avg.round()));
        }
        let strong: Vec<&str> = demand_items
            .iter()
            .filter(|a| a.state() == MatchState::Matched)
            .take(3)
            .map(|a| a.name.as_str())
            .collect();
        if !strong.is_empty() {
            summary.push(format!("Strongest coverage: {}", names(&strong)));
        }
        let top: Vec<&str> = priorities.iter().take(3).map(|p| p.name.as_str()).collect();
        if !top.is_empty() {
            summary.push(format!("Highest-impact gaps: {}", names(&top)));
        }
        if let Some(p) = priorities.first() {
            summary.push(format!(
                "{} appears in {}% of selected jobs and is {} the current Profile.",
                p.name,
                p.percent.round(),
                if p.state == MatchState::Missing {
                    "missing from"
                } else {
                    "only partly covered by"
                }
            ));
        }
    } else {
        summary.push("Profile required for personal skill-gap comparison.".into());
    }

    SkillGapView {
        profile_available: evidence_available,
        jobs,
        jobs_with_requirements: total,
        average_coverage,
        counts,
        demand,
        coverage,
        matrix,
        priorities: priorities.into_iter().take(20).collect(),
        job,
        summary,
        notes,
    }
}
