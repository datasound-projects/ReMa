//! Test fixtures for the analytics engine.

use super::{
    dataset::{currency_code, JobFacts, Requirement},
    normalize,
};
use crate::{
    db::analytics::StoredJob,
    models::analytics::{DetailsStatus, Importance, RequirementCategory, RequirementKind},
};

pub fn facts(id: i64, f: impl FnOnce(&mut StoredJob)) -> JobFacts {
    let mut job = StoredJob {
        id,
        title: format!("Job {id}"),
        normalized_title: format!("Job {id}"),
        company: None,
        company_key: None,
        location: None,
        city: None,
        country: None,
        work_mode: None,
        employment_type: None,
        seniority: None,
        salary_min: None,
        salary_max: None,
        salary_currency: None,
        salary_period: None,
        date_posted: None,
        date_discovered: 0,
        source: None,
        source_url: None,
        details: DetailsStatus::Done,
    };
    f(&mut job);
    let place = job
        .location
        .as_deref()
        .map(normalize::place)
        .unwrap_or_default();
    JobFacts {
        role_key: job.normalized_title.to_lowercase(),
        countries: place.countries,
        cities: place.cities,
        salary: (job.salary_min.is_some() || job.salary_max.is_some()).then(|| normalize::Salary {
            min: job.salary_min,
            max: job.salary_max,
            currency: job.salary_currency.as_deref().and_then(currency_code),
            period: job.salary_period,
        }),
        requirements: Vec::new(),
        runs: vec![1],
        job,
    }
}

pub fn with_skills(mut f: JobFacts, names: &[&str]) -> JobFacts {
    f.requirements = names
        .iter()
        .map(|n| Requirement {
            kind: RequirementKind::Skill,
            category: RequirementCategory::Other,
            name: n.to_string(),
            key: n.to_lowercase(),
            importance: Importance::Required,
            years: None,
            level: None,
            original: n.to_string(),
        })
        .collect();
    f
}
