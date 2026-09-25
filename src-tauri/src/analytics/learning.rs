//! Learning recommendations for the gaps that matter in the selected jobs.
//!
//! 1. Rust picks the gaps: the skill-gap priorities of the current dataset,
//!    narrowed by the user's criteria (minimum share of jobs, ignore
//!    single mentions, focus list, missing and/or partial).
//! 2. The default model researches resources for exactly those gaps. Its
//!    JSON is validated: unknown skills, malformed links, unknown resource
//!    types and oversized text are dropped; the "why" shown for each
//!    recommendation is the deterministic reasons from step 1, never the
//!    model's words.
//! 3. Links are checked for reachability, and the result is stored with the
//!    dataset, the gap snapshot and when it was generated. Opening the
//!    dashboard never re-runs research; the user refreshes it explicitly.

use std::{collections::HashSet, time::Duration};

use futures_util::{stream, StreamExt};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    dataset::{skill_key, Dataset},
    gap::{aggregate, level_of, priorities},
    normalize, page,
};
use crate::{
    db::analytics::{self as repo, StoredResearch},
    error::{AppError, AppResult},
    models::analytics::{
        AnalyticsQuery, LearningCriteria, LearningGap, LearningRecommendation, LearningResearch,
        LearningResource, MatchState, RequirementKind, ResourceType, SkillGapOptions,
    },
};

pub const MAX_GAPS: u32 = 15;

/// The gaps worth researching for the analyzed jobs.
pub fn gaps(
    ds: &Dataset,
    criteria: &LearningCriteria,
    options: &SkillGapOptions,
) -> Vec<LearningGap> {
    let agg = aggregate(ds, options.weight_by_rank);
    let total = agg.with_requirements;
    let focus: HashSet<String> = criteria.focus.iter().map(|f| skill_key(f)).collect();
    let learnable = |kind: RequirementKind| {
        matches!(
            kind,
            RequirementKind::Skill
                | RequirementKind::Certification
                | RequirementKind::Language
                | RequirementKind::Other
        )
    };
    priorities(ds, &agg, options)
        .into_iter()
        .filter(|p| learnable(p.kind))
        .filter(|p| {
            p.state == MatchState::Missing
                || (criteria.include_partial && p.state == MatchState::Partial)
        })
        .filter(|p| p.percent >= criteria.min_percent)
        .filter(|p| !criteria.ignore_single || total <= 1 || p.jobs >= 2)
        .filter(|p| focus.is_empty() || focus.contains(&skill_key(&p.name)))
        .take(criteria.max_gaps.clamp(1, MAX_GAPS) as usize)
        .map(|p| LearningGap {
            name: p.name,
            kind: p.kind,
            category: p.category,
            state: p.state,
            jobs: p.jobs,
            total_jobs: total,
            percent: p.percent,
            score: p.score,
            level: level_of(p.score),
            reasons: p.reasons,
        })
        .collect()
}

/// Identifies a selection + criteria, to find earlier research for it.
pub fn dataset_key(
    query: &AnalyticsQuery,
    criteria: &LearningCriteria,
    options: &SkillGapOptions,
) -> String {
    #[derive(Serialize)]
    struct Key<'a> {
        query: &'a AnalyticsQuery,
        criteria: &'a LearningCriteria,
        weight_by_rank: bool,
    }
    let mut query = query.clone();
    // Ranking only changes the jobs when a top-N limit or weighting applies.
    if query.limit.is_none() && !options.weight_by_rank {
        query.ranking.clear();
    }
    let json = serde_json::to_string(&Key {
        query: &query,
        criteria,
        weight_by_rank: options.weight_by_rank,
    })
    .unwrap_or_default();
    Sha256::digest(json.as_bytes())
        .iter()
        .take(12)
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ── Research ──────────────────────────────────────────────────────────

pub const RULES: &str = r#"You are ReMa's learning-resource researcher. The user wants to close specific skill gaps for the jobs they selected; the gaps were calculated from those jobs and are given to you. Do not add, remove or reorder gaps.

For each gap:
- "path": 2–5 short, concrete learning steps in order (e.g. "Terraform fundamentals", "Terraform with AWS", "Modules, state and CI").
- "resources": 2–4 high-quality resources. Prefer official documentation and vendor training, recognised certifications, university courses, respected platforms and books, and practical labs or well-known open-source projects. Avoid generic course spam, affiliate links and listicles.
- Only resources you are confident exist, with their official https URL. If unsure of an exact URL, use the official page of the provider rather than inventing a deep link.
- "type": one of "certification", "course", "university", "documentation", "book", "lab", "tutorial", "project", "program".
- "level": "beginner", "intermediate" or "advanced" (or null). "cost": "free", "paid" or "mixed" (or null).
- "note": one sentence on why this resource fits these jobs.

Return JSON only: {"recommendations": [{"skill": "...", "path": ["..."], "resources": [{"title": "...", "provider": "...", "url": "https://...", "type": "...", "level": null, "cost": null, "note": "..."}]}]}"#;

/// The research request: the deterministic snapshot and the job context.
pub fn prompt(ds: &Dataset, gaps: &[LearningGap]) -> String {
    let mut roles: Vec<(String, u32)> = Vec::new();
    for job in &ds.jobs {
        let title = &job.facts.job.normalized_title;
        match roles
            .iter_mut()
            .find(|(r, _)| r.eq_ignore_ascii_case(title))
        {
            Some((_, n)) => *n += 1,
            None => roles.push((title.clone(), 1)),
        }
    }
    roles.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let roles: Vec<String> = roles
        .iter()
        .take(5)
        .map(|(r, n)| format!("{r} ({n})"))
        .collect();
    let mut lines = vec![
        format!("Selected jobs: {} — {} jobs.", ds.label, ds.jobs.len()),
        format!("Most common roles: {}.", roles.join(", ")),
        String::new(),
        "Gaps to research (highest priority first):".into(),
    ];
    for g in gaps {
        lines.push(format!(
            "- {} — {} of {} jobs ({}%), {} in the user's Profile, priority {:?}.",
            g.name,
            g.jobs,
            g.total_jobs,
            g.percent.round(),
            if g.state == MatchState::Missing {
                "missing"
            } else {
                "only partly covered"
            },
            g.level
        ));
    }
    lines.join("\n")
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    recommendations: Vec<RawRecommendation>,
}

#[derive(Deserialize)]
struct RawRecommendation {
    #[serde(default)]
    skill: String,
    #[serde(default)]
    path: Vec<String>,
    #[serde(default)]
    resources: Vec<RawResource>,
}

#[derive(Deserialize)]
struct RawResource {
    #[serde(default)]
    title: String,
    provider: Option<String>,
    #[serde(default)]
    url: String,
    #[serde(default, rename = "type")]
    kind: String,
    level: Option<String>,
    cost: Option<String>,
    note: Option<String>,
}

/// A public https/http web address (no local hosts, no IP literals).
fn resource_url(url: &str) -> Option<String> {
    let clean = normalize::web_url(url)?;
    let parsed = Url::parse(&clean).ok()?;
    let host = parsed.host_str()?;
    let local = host == "localhost"
        || host.ends_with(".local")
        || host.parse::<std::net::IpAddr>().is_ok()
        || !host.contains('.');
    (!local).then_some(clean)
}

fn one_of(value: Option<String>, allowed: &[&str]) -> Option<String> {
    let v = value?.trim().to_lowercase();
    allowed.contains(&v.as_str()).then_some(v)
}

/// Validates the model's answer against the gap snapshot.
pub fn parse(answer: &str, gaps: &[LearningGap]) -> AppResult<Vec<LearningRecommendation>> {
    let parsed: Answer = serde_json::from_str(crate::jobs::extract::json_object(answer)?)
        .map_err(|_| AppError::provider("the recommendations were not in the expected format"))?;
    let mut out: Vec<LearningRecommendation> = Vec::new();
    for raw in parsed.recommendations {
        let key = skill_key(&raw.skill);
        let Some(gap) = gaps
            .iter()
            .find(|g| skill_key(&g.name) == key || g.name.eq_ignore_ascii_case(raw.skill.trim()))
        else {
            continue;
        };
        if out.iter().any(|r| r.skill == gap.name) {
            continue;
        }
        let path: Vec<String> = raw
            .path
            .iter()
            .map(|s| normalize::clip(s, 160))
            .filter(|s| !s.is_empty())
            .take(6)
            .collect();
        let mut seen = HashSet::new();
        let resources: Vec<LearningResource> = raw
            .resources
            .into_iter()
            .filter_map(|r| {
                let url = resource_url(&r.url)?;
                let title = normalize::clip(&r.title, 140);
                let kind = ResourceType::parse(r.kind.trim().to_lowercase().as_str())?;
                (!title.is_empty() && seen.insert(url.clone())).then(|| LearningResource {
                    title,
                    provider: r
                        .provider
                        .map(|p| normalize::clip(&p, 80))
                        .filter(|p| !p.is_empty()),
                    url,
                    kind,
                    level: one_of(r.level, &["beginner", "intermediate", "advanced"]),
                    cost: one_of(r.cost, &["free", "paid", "mixed"]),
                    note: r
                        .note
                        .map(|n| normalize::clip(&n, 200))
                        .filter(|n| !n.is_empty()),
                    reachable: None,
                })
            })
            .take(5)
            .collect();
        if resources.is_empty() {
            continue;
        }
        out.push(LearningRecommendation {
            skill: gap.name.clone(),
            level: gap.level,
            why: gap.reasons.clone(),
            path,
            resources,
        });
    }
    // Keep the deterministic priority order.
    out.sort_by_key(|r| {
        gaps.iter()
            .position(|g| g.name == r.skill)
            .unwrap_or(usize::MAX)
    });
    Ok(out)
}

/// Checks that each resource link opens (public addresses only).
pub async fn check_links(
    client: &reqwest::Client,
    recommendations: &mut [LearningRecommendation],
    allow_private: bool,
) {
    let urls: Vec<String> = recommendations
        .iter()
        .flat_map(|r| r.resources.iter().map(|x| x.url.clone()))
        .collect();
    let results: Vec<(String, bool)> = stream::iter(urls)
        .map(|url| async move {
            let ok = tokio::time::timeout(
                Duration::from_secs(15),
                page::fetch(client, &url, allow_private),
            )
            .await
            .is_ok_and(|r| r.is_ok() || r.is_err_and(|e| e.to_string().contains("not a web page")));
            (url, ok)
        })
        .buffer_unordered(6)
        .collect()
        .await;
    for r in recommendations.iter_mut() {
        for resource in &mut r.resources {
            resource.reachable = results
                .iter()
                .find(|(u, _)| *u == resource.url)
                .map(|(_, ok)| *ok);
        }
    }
}

/// The stored research as the view shows it.
pub fn to_view(stored: StoredResearch, key: &str, current: &[LearningGap]) -> LearningResearch {
    let gaps: Vec<LearningGap> = serde_json::from_str(&stored.gap_snapshot).unwrap_or_default();
    let is_current = stored.dataset_key == key;
    let names = |g: &[LearningGap]| g.iter().map(|x| x.name.clone()).collect::<HashSet<_>>();
    LearningResearch {
        id: stored.id,
        dataset_label: stored.dataset_label,
        job_count: stored.job_count,
        generated_at: stored.generated_at,
        status: stored.status,
        error: stored.error,
        model: stored.model_id,
        stale: is_current && names(&gaps) != names(current),
        current: is_current,
        recommendations: serde_json::from_str(&stored.recommendations).unwrap_or_default(),
        gaps,
    }
}

pub fn store_failure(conn: &rusqlite::Connection, id: i64, error: &AppError) -> AppResult<()> {
    let message: String = error.to_string().chars().take(300).collect();
    repo::finish_research(
        conn,
        id,
        crate::models::analytics::ResearchStatus::Failed,
        None,
        Some(&message),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::analytics::{PriorityLevel, RequirementCategory};

    fn gap(name: &str) -> LearningGap {
        LearningGap {
            name: name.into(),
            kind: RequirementKind::Skill,
            category: RequirementCategory::CloudInfrastructure,
            state: MatchState::Missing,
            jobs: 22,
            total_jobs: 35,
            percent: 62.9,
            score: 0.63,
            level: PriorityLevel::High,
            reasons: vec!["Required by 22 of 35 jobs (63%)".into()],
        }
    }

    #[test]
    fn keeps_only_valid_recommendations_for_known_gaps() {
        let gaps = [gap("Terraform"), gap("AWS")];
        let answer = r#"Here you go:
{"recommendations": [
 {"skill": "aws", "path": ["Cloud basics", "Core services"], "resources": [
   {"title": "AWS Skill Builder", "provider": "AWS", "url": "https://skillbuilder.aws/", "type": "course", "level": "Beginner", "cost": "free", "note": "Official."},
   {"title": "Local", "url": "http://localhost:8080/x", "type": "course"},
   {"title": "Bad type", "url": "https://example.com/a", "type": "podcast"}]},
 {"skill": "Terraform", "path": ["Fundamentals"], "resources": [
   {"title": "Terraform Associate", "provider": "HashiCorp", "url": "https://developer.hashicorp.com/certifications/infrastructure-automation", "type": "certification"},
   {"title": "Duplicate", "url": "https://developer.hashicorp.com/certifications/infrastructure-automation", "type": "documentation"}]},
 {"skill": "Cooking", "path": [], "resources": [{"title": "x", "url": "https://x.com", "type": "course"}]}
]}"#;
        let recs = parse(answer, &gaps).unwrap();
        assert_eq!(
            recs.iter().map(|r| r.skill.as_str()).collect::<Vec<_>>(),
            ["Terraform", "AWS"]
        );
        assert_eq!(recs[0].resources.len(), 1, "duplicate URL dropped");
        assert_eq!(
            recs[1].resources.len(),
            1,
            "local and invalid resources dropped"
        );
        assert_eq!(recs[1].resources[0].level.as_deref(), Some("beginner"));
        assert_eq!(
            recs[1].why,
            ["Required by 22 of 35 jobs (63%)"],
            "reasons come from Rust"
        );
    }

    #[test]
    fn dataset_keys_ignore_ranking_unless_it_selects_jobs() {
        let criteria = LearningCriteria::default();
        let options = SkillGapOptions::default();
        let mut a = AnalyticsQuery::default();
        let mut b = a.clone();
        b.ranking.push(crate::models::analytics::SortCriterion {
            key: crate::models::analytics::SortKey::Salary,
            direction: crate::models::analytics::SortDirection::Desc,
            value: None,
        });
        assert_eq!(
            dataset_key(&a, &criteria, &options),
            dataset_key(&b, &criteria, &options)
        );
        a.limit = Some(20);
        b.limit = Some(20);
        assert_ne!(
            dataset_key(&a, &criteria, &options),
            dataset_key(&b, &criteria, &options)
        );
    }
}
