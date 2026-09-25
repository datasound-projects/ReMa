//! Dynamic multi-criteria ranking.
//!
//! The user's criteria are ordered sort priorities: the first criterion
//! decides, later ones break ties. Unknown values always rank after known
//! ones, whichever the direction. There is no hidden "best job" formula.

use std::cmp::Ordering;

use super::{
    dataset::{known_first, skill_key, Job},
    normalize,
};
use crate::{
    error::{AppError, AppResult},
    models::analytics::{Seniority, SortCriterion, SortDirection, SortKey, WorkMode},
};

pub fn validate(c: &SortCriterion) -> AppResult<()> {
    let needs_value = matches!(
        c.key,
        SortKey::PreferCountry
            | SortKey::PreferCity
            | SortKey::PreferRole
            | SortKey::PreferCompany
            | SortKey::PreferSkill
    );
    if needs_value && c.value.as_deref().is_none_or(|v| v.trim().is_empty()) {
        return Err(AppError::validation("Choose what should rank first."));
    }
    if c.value.as_deref().is_some_and(|v| v.len() > 200) {
        return Err(AppError::validation("The ranking value is too long."));
    }
    Ok(())
}

fn work_mode_rank(mode: Option<WorkMode>) -> Option<u8> {
    mode.map(|m| match m {
        WorkMode::Remote => 3,
        WorkMode::Hybrid => 2,
        WorkMode::Onsite => 1,
    })
}

fn seniority_rank(s: Option<Seniority>) -> Option<u8> {
    s.map(|s| s as u8)
}

/// Whether a "prefer …" criterion holds for a job.
fn preferred(c: &SortCriterion, job: &Job) -> bool {
    let value = c.value.as_deref().unwrap_or("").trim().to_lowercase();
    let j = &job.facts.job;
    match c.key {
        SortKey::PreferCountry => {
            let wanted = normalize::country_name(&value).map_or(value.clone(), str::to_lowercase);
            job.facts
                .countries
                .iter()
                .any(|c| c.to_lowercase() == wanted)
        }
        SortKey::PreferCity => {
            let wanted = normalize::city_name(&value).map_or(value.clone(), str::to_lowercase);
            job.facts.cities.iter().any(|c| c.to_lowercase() == wanted)
        }
        SortKey::PreferRole => {
            j.title.to_lowercase().contains(&value)
                || j.normalized_title.to_lowercase().contains(&value)
        }
        SortKey::PreferCompany => j.company.as_deref().is_some_and(|c| {
            normalize::company_key(c) == normalize::company_key(&value)
                || c.to_lowercase().contains(&value)
        }),
        SortKey::PreferSkill => job.facts.requires(&skill_key(&value)),
        _ => false,
    }
}

fn by(c: &SortCriterion, a: &Job, b: &Job, currency: Option<&str>) -> Ordering {
    let desc = c.direction == SortDirection::Desc;
    let (x, y) = (&a.facts.job, &b.facts.job);
    match c.key {
        SortKey::WorkMode => known_first(
            work_mode_rank(x.work_mode),
            work_mode_rank(y.work_mode),
            desc,
        ),
        SortKey::Salary => {
            let value = |job: &Job| currency.and_then(|cur| job.facts.annual_salary(cur));
            known_first(value(a), value(b), desc)
        }
        SortKey::Match => known_first(a.coverage, b.coverage, desc),
        SortKey::SkillGap => known_first(a.missing(), b.missing(), desc),
        SortKey::DatePosted => known_first(x.date_posted, y.date_posted, desc),
        SortKey::DateDiscovered => {
            known_first(Some(x.date_discovered), Some(y.date_discovered), desc)
        }
        SortKey::Company => known_first(
            x.company.as_deref().map(str::to_lowercase),
            y.company.as_deref().map(str::to_lowercase),
            desc,
        ),
        SortKey::Title => known_first(
            Some(x.title.to_lowercase()),
            Some(y.title.to_lowercase()),
            desc,
        ),
        SortKey::Seniority => known_first(
            seniority_rank(x.seniority),
            seniority_rank(y.seniority),
            desc,
        ),
        SortKey::PreferCountry
        | SortKey::PreferCity
        | SortKey::PreferRole
        | SortKey::PreferCompany
        | SortKey::PreferSkill => {
            // Descending: matching jobs first.
            let o = preferred(c, a).cmp(&preferred(c, b));
            if desc {
                o.reverse()
            } else {
                o
            }
        }
    }
}

/// Compares two jobs by the criteria in priority order.
pub fn compare(a: &Job, b: &Job, criteria: &[SortCriterion], currency: Option<&str>) -> Ordering {
    criteria
        .iter()
        .map(|c| by(c, a, b, currency))
        .find(|o| *o != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

/// Short description, e.g. "highest salary".
pub fn describe(c: &SortCriterion) -> String {
    let desc = c.direction == SortDirection::Desc;
    let value = c.value.as_deref().unwrap_or("").trim();
    match c.key {
        SortKey::WorkMode => if desc {
            "remote first"
        } else {
            "on-site first"
        }
        .into(),
        SortKey::Salary => if desc {
            "highest salary"
        } else {
            "lowest salary"
        }
        .into(),
        SortKey::Match => if desc {
            "best Profile match"
        } else {
            "lowest Profile match"
        }
        .into(),
        SortKey::SkillGap => if desc {
            "most missing skills"
        } else {
            "fewest missing skills"
        }
        .into(),
        SortKey::DatePosted => if desc {
            "newest posting"
        } else {
            "oldest posting"
        }
        .into(),
        SortKey::DateDiscovered => if desc { "newest found" } else { "oldest found" }.into(),
        SortKey::Company => "company".into(),
        SortKey::Title => "title".into(),
        SortKey::Seniority => if desc { "most senior" } else { "most junior" }.into(),
        SortKey::PreferCountry | SortKey::PreferCity => format!("{value} first"),
        SortKey::PreferRole => format!("“{value}” roles first"),
        SortKey::PreferCompany => format!("{value} first"),
        SortKey::PreferSkill => format!("requiring {value} first"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analytics::{fixtures::facts, matching::Evidence},
        models::analytics::SalaryPeriod,
    };

    fn crit(key: SortKey, direction: SortDirection, value: Option<&str>) -> SortCriterion {
        SortCriterion {
            key,
            direction,
            value: value.map(str::to_string),
        }
    }

    #[test]
    fn ranks_by_ordered_criteria_with_unknowns_last() {
        let evidence = Evidence::default();
        let salary = |amount: Option<f64>| {
            move |j: &mut crate::db::analytics::StoredJob| {
                j.salary_min = amount;
                j.salary_max = amount;
                j.salary_currency = amount.map(|_| "EUR".into());
                j.salary_period = amount.map(|_| SalaryPeriod::Year);
            }
        };
        let jobs = [
            facts(1, |j| {
                salary(Some(90_000.0))(j);
                j.work_mode = Some(WorkMode::Onsite);
            }),
            facts(2, |j| {
                salary(Some(120_000.0))(j);
                j.work_mode = Some(WorkMode::Remote);
            }),
            facts(3, |j| {
                salary(None)(j);
                j.work_mode = Some(WorkMode::Remote);
            }),
            facts(4, |j| {
                salary(Some(95_000.0))(j);
                j.work_mode = Some(WorkMode::Remote);
                j.location = Some("Vienna, Austria".into());
            }),
        ];
        let evaluated: Vec<Job> = jobs.iter().map(|f| Job::evaluate(f, &evidence)).collect();
        let order = |criteria: &[SortCriterion]| {
            let mut refs: Vec<&Job> = evaluated.iter().collect();
            refs.sort_by(|a, b| compare(a, b, criteria, Some("EUR")).then(a.id().cmp(&b.id())));
            refs.iter().map(|j| j.id()).collect::<Vec<_>>()
        };
        // Highest salary first; the job without salary last in both directions.
        assert_eq!(
            order(&[crit(SortKey::Salary, SortDirection::Desc, None)]),
            [2, 4, 1, 3]
        );
        assert_eq!(
            order(&[crit(SortKey::Salary, SortDirection::Asc, None)]),
            [1, 4, 2, 3]
        );
        // Remote first, then salary.
        assert_eq!(
            order(&[
                crit(SortKey::WorkMode, SortDirection::Desc, None),
                crit(SortKey::Salary, SortDirection::Desc, None)
            ]),
            [2, 4, 3, 1]
        );
        // Austria first, then highest salary.
        assert_eq!(
            order(&[
                crit(SortKey::PreferCountry, SortDirection::Desc, Some("Austria")),
                crit(SortKey::Salary, SortDirection::Desc, None)
            ]),
            [4, 2, 1, 3]
        );
        assert!(validate(&crit(SortKey::PreferCity, SortDirection::Desc, None)).is_err());
    }
}
