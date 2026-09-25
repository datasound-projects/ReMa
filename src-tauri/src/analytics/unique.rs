//! Unique requirements analysis: every distinct normalized requirement of
//! the analyzed jobs, with counts, frequency classes and Profile state.
//!
//! Frequency classes use explicit thresholds over the jobs that have
//! requirement data: Very common ≥ 70 %, Common ≥ 40 %, Occasional ≥ 15 %,
//! Rare < 15 %.

use std::collections::HashMap;

use super::{
    dataset::Dataset,
    gap::{aggregate, percent, Aggregate},
};
use crate::models::analytics::{
    CategoryCount, ClassCount, FrequencyClass, MatchState, RequirementCategory, RequirementKind,
    RequirementRow, RequirementSort, RequirementTableQuery, RequirementsView, SortDirection,
};

pub const VERY_COMMON: f64 = 70.0;
pub const COMMON: f64 = 40.0;
pub const OCCASIONAL: f64 = 15.0;
/// Below this many jobs, "rare" says nothing.
const MIN_JOBS_FOR_RARITY: u32 = 5;

pub fn class(percent: f64) -> FrequencyClass {
    if percent >= VERY_COMMON {
        FrequencyClass::VeryCommon
    } else if percent >= COMMON {
        FrequencyClass::Common
    } else if percent >= OCCASIONAL {
        FrequencyClass::Occasional
    } else {
        FrequencyClass::Rare
    }
}

fn is_technical(category: RequirementCategory) -> bool {
    use RequirementCategory::*;
    matches!(
        category,
        TechnicalSkills
            | ProgrammingLanguages
            | Frameworks
            | CloudInfrastructure
            | AiMl
            | DataEngineering
            | Databases
    )
}

fn row(a: &Aggregate, total: u32, profile: bool) -> RequirementRow {
    let percent = percent(a.jobs, total);
    RequirementRow {
        name: a.name.clone(),
        kind: a.kind,
        category: a.category,
        jobs: a.jobs,
        percent,
        class: class(percent),
        required: a.required,
        preferred: a.preferred,
        state: profile.then(|| a.state()),
        level: a.common_level(),
        examples: a.examples.clone(),
    }
}

fn state_order(s: Option<MatchState>) -> u8 {
    match s {
        Some(MatchState::Missing) => 0,
        Some(MatchState::Partial) => 1,
        Some(MatchState::Matched) => 2,
        Some(MatchState::Unknown) => 3,
        None => 4,
    }
}

pub fn view(ds: &Dataset, profile: bool, table: &RequirementTableQuery) -> RequirementsView {
    let agg = aggregate(ds, false);
    let total = agg.with_requirements;
    let all: Vec<RequirementRow> = agg.items.iter().map(|a| row(a, total, profile)).collect();

    let mut categories: HashMap<RequirementCategory, (u32, u32)> = HashMap::new();
    for r in &all {
        let c = categories.entry(r.category).or_default();
        c.0 += 1;
        c.1 += r.jobs;
    }
    let mut categories: Vec<CategoryCount> = categories
        .into_iter()
        .map(|(category, (requirements, mentions))| CategoryCount {
            category,
            requirements,
            mentions,
        })
        .collect();
    categories.sort_by(|a, b| {
        b.mentions
            .cmp(&a.mentions)
            .then_with(|| a.category.cmp(&b.category))
    });

    let classes = [
        (FrequencyClass::VeryCommon, VERY_COMMON),
        (FrequencyClass::Common, COMMON),
        (FrequencyClass::Occasional, OCCASIONAL),
        (FrequencyClass::Rare, 0.0),
    ]
    .into_iter()
    .map(|(class, threshold)| ClassCount {
        class,
        requirements: all.iter().filter(|r| r.class == class).count() as u32,
        threshold,
    })
    .collect();

    // Summary from the full (unfiltered) table.
    let jobs = ds.jobs.len() as u32;
    let mut summary = vec![if total < jobs {
        format!("{jobs} jobs analyzed (requirements known for {total}).")
    } else {
        format!("{jobs} jobs analyzed.")
    }];
    if total > 0 {
        let common = all.iter().filter(|r| r.percent >= COMMON).count();
        summary.push(format!(
            "{common} requirements appear in at least {COMMON}% of roles."
        ));
        let top: Vec<&str> = all.iter().take(4).map(|r| r.name.as_str()).collect();
        summary.push(format!("Most common: {}", top.join(", ")));
        if profile {
            let missing: Vec<&str> = all
                .iter()
                .filter(|r| r.state == Some(MatchState::Missing) && r.required > 0)
                .take(3)
                .map(|r| r.name.as_str())
                .collect();
            if !missing.is_empty() {
                summary.push(format!(
                    "Most important requirements missing from Profile: {}",
                    missing.join(", ")
                ));
            }
        }
        if total >= MIN_JOBS_FOR_RARITY {
            let rare: Vec<&str> = all
                .iter()
                .filter(|r| r.class == FrequencyClass::Rare)
                .filter(|r| {
                    matches!(
                        r.kind,
                        RequirementKind::Skill | RequirementKind::Certification
                    ) && is_technical(r.category)
                })
                .take(3)
                .map(|r| r.name.as_str())
                .collect();
            if !rare.is_empty() {
                summary.push(format!(
                    "Rare but potentially differentiating: {}",
                    rare.join(", ")
                ));
            }
        }
    }

    // Table controls.
    let search = table.search.trim().to_lowercase();
    let mut rows: Vec<RequirementRow> = all
        .iter()
        .filter(|r| {
            search.is_empty()
                || r.name.to_lowercase().contains(&search)
                || r.examples
                    .iter()
                    .any(|e| e.to_lowercase().contains(&search))
        })
        .filter(|r| table.categories.is_empty() || table.categories.contains(&r.category))
        .filter(|r| table.classes.is_empty() || table.classes.contains(&r.class))
        .filter(|r| table.states.is_empty() || r.state.is_some_and(|s| table.states.contains(&s)))
        .cloned()
        .collect();
    rows.sort_by(|a, b| {
        let primary = match table.sort {
            RequirementSort::Jobs => a.jobs.cmp(&b.jobs),
            RequirementSort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            RequirementSort::Category => a.category.cmp(&b.category),
            RequirementSort::State => state_order(a.state).cmp(&state_order(b.state)),
        };
        let primary = if table.direction == SortDirection::Desc {
            primary.reverse()
        } else {
            primary
        };
        primary
            .then_with(|| b.jobs.cmp(&a.jobs))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    RequirementsView {
        jobs,
        jobs_with_requirements: total,
        total: all.len() as u32,
        rows,
        categories,
        classes,
        profile_available: profile,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_with_explicit_thresholds() {
        assert_eq!(class(70.0), FrequencyClass::VeryCommon);
        assert_eq!(class(69.9), FrequencyClass::Common);
        assert_eq!(class(40.0), FrequencyClass::Common);
        assert_eq!(class(15.0), FrequencyClass::Occasional);
        assert_eq!(class(14.9), FrequencyClass::Rare);
    }
}
