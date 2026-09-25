//! The ranked job table, dataset coverage and filter suggestions.

use std::collections::{HashMap, HashSet};

use super::{dataset::Dataset, normalize};
use crate::models::analytics::{
    AnalyticsOverview, DatasetSummary, FacetValue, Facets, PipelineStatus, RankedJob,
    RequirementKind, SalaryCoverage, ScopeKind,
};

/// Rows sent to the table at most; the rest are counted.
pub const MAX_ROWS: usize = 300;

pub fn summary(ds: &Dataset, scope_kind: ScopeKind, run_ids: &[i64]) -> DatasetSummary {
    let counted_run = |run: &i64| scope_kind != ScopeKind::Searches || run_ids.contains(run);
    let mut runs = HashSet::new();
    let mut sightings = 0usize;
    for job in &ds.jobs {
        for run in job.facts.runs.iter().filter(|r| counted_run(r)) {
            runs.insert(*run);
            sightings += 1;
        }
    }
    let analyzed = ds.jobs.len();
    let known = ds.jobs.iter().filter(|j| j.facts.salary.is_some()).count();
    let comparable = ds.currency.map_or(0, |c| {
        ds.jobs
            .iter()
            .filter(|j| j.facts.annual_salary(c).is_some())
            .count()
    });
    let mut notes = ds.notes.clone();
    if analyzed > 0 {
        notes.push(format!("Salary available for {known} of {analyzed} jobs."));
        if let Some(currency) = ds.currency {
            if comparable < known {
                notes.push(format!(
                    "Salary comparisons use {currency} per year (monthly × 12); {} other salaries (other currencies or hourly/daily rates) are not compared.",
                    known - comparable
                ));
            }
        }
        let posted = ds
            .jobs
            .iter()
            .filter(|j| j.facts.job.date_posted.is_some())
            .count();
        if posted < analyzed {
            notes.push(format!(
                "Posting date known for {posted} of {analyzed} jobs; the discovery date is shown separately and never used as a posting date."
            ));
        }
    }
    DatasetSummary {
        label: ds.label.clone(),
        in_scope: ds.in_scope as u32,
        matching: ds.matching as u32,
        analyzed: analyzed as u32,
        runs: runs.len() as u32,
        duplicates_merged: sightings.saturating_sub(analyzed) as u32,
        posted_known: ds
            .jobs
            .iter()
            .filter(|j| j.facts.job.date_posted.is_some())
            .count() as u32,
        requirements_known: ds.with_requirements() as u32,
        salary: SalaryCoverage {
            known: known as u32,
            comparable: comparable as u32,
            currency: ds.currency.map(str::to_string),
        },
        notes,
    }
}

fn facet(values: impl Iterator<Item = (String, String)>, limit: usize) -> Vec<FacetValue> {
    // Group by key; show the first spelling seen.
    let mut groups: HashMap<String, (String, u32)> = HashMap::new();
    for (key, shown) in values {
        groups.entry(key).or_insert((shown, 0)).1 += 1;
    }
    let mut out: Vec<FacetValue> = groups
        .into_values()
        .map(|(value, count)| FacetValue { value, count })
        .collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.value.to_lowercase().cmp(&b.value.to_lowercase()))
    });
    out.truncate(limit);
    out
}

/// Values in the scoped jobs (before filters), for filter suggestions.
pub fn facets(ds: &Dataset) -> Facets {
    let jobs = &ds.scoped;
    let requirement = |kinds: &[RequirementKind]| {
        facet(
            jobs.iter().flat_map(|j| {
                j.requirements
                    .iter()
                    .filter(|r| kinds.contains(&r.kind))
                    .map(|r| (r.key.clone(), r.name.clone()))
            }),
            200,
        )
    };
    Facets {
        companies: facet(
            jobs.iter().filter_map(|j| {
                let c = j.job.company.clone()?;
                Some((
                    j.job
                        .company_key
                        .clone()
                        .unwrap_or_else(|| c.to_lowercase()),
                    c,
                ))
            }),
            100,
        ),
        roles: facet(
            jobs.iter()
                .map(|j| (j.role_key.clone(), j.job.normalized_title.clone())),
            100,
        ),
        countries: facet(
            jobs.iter()
                .flat_map(|j| j.countries.iter().map(|c| (c.to_string(), c.to_string()))),
            100,
        ),
        cities: facet(
            jobs.iter()
                .flat_map(|j| j.cities.iter().map(|c| (c.to_string(), c.to_string()))),
            100,
        ),
        sources: facet(
            jobs.iter().filter_map(|j| {
                let s = j.job.source.clone()?;
                Some((s.to_lowercase(), s))
            }),
            50,
        ),
        skills: requirement(&[
            RequirementKind::Skill,
            RequirementKind::Other,
            RequirementKind::SoftSkill,
        ]),
        languages: requirement(&[RequirementKind::Language]),
        certifications: requirement(&[RequirementKind::Certification]),
        currencies: facet(
            jobs.iter().filter_map(|j| {
                let c = j.salary.as_ref()?.currency?;
                Some((c.to_string(), c.to_string()))
            }),
            20,
        ),
    }
}

pub fn overview(
    ds: &Dataset,
    summary: DatasetSummary,
    status: PipelineStatus,
) -> AnalyticsOverview {
    let jobs = ds
        .jobs
        .iter()
        .take(MAX_ROWS)
        .enumerate()
        .map(|(i, job)| {
            let f = job.facts;
            let j = &f.job;
            RankedJob {
                rank: i as u32 + 1,
                id: j.id,
                title: j.title.clone(),
                company: j.company.clone(),
                location: j.location.clone(),
                work_mode: j.work_mode,
                employment_type: j.employment_type,
                seniority: j.seniority,
                salary: f.salary.as_ref().map(normalize::format_salary),
                salary_comparable: ds.currency.is_some_and(|c| f.annual_salary(c).is_some()),
                match_percent: job.coverage.map(|c| c.round()),
                missing: job.missing(),
                date_posted: j.date_posted,
                date_discovered: j.date_discovered,
                source: j.source.clone(),
                url: j.source_url.clone(),
                appearances: f.runs.len() as u32,
                details: j.details,
            }
        })
        .collect();
    AnalyticsOverview {
        summary,
        more: ds.jobs.len().saturating_sub(MAX_ROWS) as u32,
        jobs,
        facets: facets(ds),
        status,
    }
}
