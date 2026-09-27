//! The NetworkResearchService (NC §5, §20, §33, §56–§59): one entry point
//! for the Network Connect page, Chat, tools and scheduled tracking. It
//! plans the request, runs only the stages it needs (companies → jobs →
//! people → connections), resolves entities, keeps the evidence and hands
//! back one structured result. Every source is optional: a failing one is
//! reported, the rest still answer.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use tokio_util::sync::CancellationToken;

use super::{
    capabilities::{self, RelationshipAccess},
    companies,
    model::{Company, ConnectionsOutcome, NetworkResult, ResultStatus, Row, Stage, StageReport},
    people,
    planner::{self, NetworkIntent},
    policy::{self, DataClass, Operation, Purpose, POLICY_VERSION},
    relationships, resolve,
};
use crate::{
    career_search::plan,
    llm::Endpoint,
    retrieval::Progress,
    services::profile_context::{self, Field},
    state::AppState,
    time::now_ms,
};

/// Longest one research request may take.
const BUDGET: Duration = Duration::from_secs(120);

/// A research request.
#[derive(Debug, Clone, Default)]
pub struct Request {
    pub query: String,
    /// The job it is about (the page's or chat's context).
    pub job_url: Option<String>,
    /// The company it is about.
    pub company: Option<String>,
    /// The caller allows the user's Profile (Chat's Profile switch; the
    /// page only when the request refers to the user's background).
    pub profile_allowed: bool,
    pub purpose: Option<Purpose>,
}

/// The request's intent with the caller's context and, when allowed, the
/// user's target roles.
pub fn intent_for(state: &AppState, request: &Request) -> (NetworkIntent, Vec<String>) {
    let mut intent = planner::intent(&request.query);
    let mut notes = Vec::new();
    if let Some(url) = &request.job_url {
        intent.job_url = Some(url.clone());
    }
    if let Some(company) = request.company.as_ref().filter(|c| !c.trim().is_empty()) {
        intent.target_company = Some(company.trim().to_string());
    }
    if intent.uses_profile {
        if !request.profile_allowed {
            notes.push("Your Profile was not used: it is turned off for this request.".to_string());
        } else {
            match profile_context::load(state) {
                Ok(Some(context)) => {
                    let targets: Vec<String> = context
                        .get(Field::TargetRoles)
                        .iter()
                        .map(|f| f.text.clone())
                        .filter(|t| t.len() <= 60)
                        .take(3)
                        .collect();
                    if intent.roles.is_empty() {
                        if let Some(first) = targets.first() {
                            intent.roles.push(first.clone());
                            for related in plan::related_roles(first) {
                                if !intent.roles.contains(&related) {
                                    intent.roles.push(related);
                                }
                            }
                            intent.hiring = true;
                        }
                    }
                    notes.push(if targets.is_empty() {
                        "Your Profile names no target roles; ReMa used the request's words."
                            .to_string()
                    } else {
                        format!("From your Profile: target roles {}.", targets.join(", "))
                    });
                }
                _ => notes.push("Your Profile has no information ReMa could use.".into()),
            }
        }
        // "my AI/data background": the words before "background".
        if intent.roles.is_empty() {
            let lower = intent.text.to_lowercase();
            if let Some(i) = lower.find(" background") {
                let before = &intent.text[..i];
                if let Some(area) = before.split_whitespace().last() {
                    let area = area.trim_matches(|c: char| !c.is_alphanumeric() && c != '/');
                    if let Some(first) = area.split('/').next().filter(|a| a.len() >= 2) {
                        let first = if first.len() <= 3 {
                            first.to_uppercase()
                        } else {
                            first.to_string()
                        };
                        intent.roles.push(first.clone());
                        intent.roles.extend(plan::related_roles(&first));
                        intent.hiring = true;
                    }
                }
            }
        }
    }
    (intent, notes)
}

fn report(stage: Stage, summary: String, sources: Vec<String>, failed: Vec<String>) -> StageReport {
    StageReport {
        stage,
        summary,
        sources,
        failed,
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Runs a research request.
pub async fn research(
    state: &AppState,
    request: &Request,
    model: Option<(&Endpoint, &str)>,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> NetworkResult {
    let started = Instant::now();
    let now = now_ms();
    let (intent, mut notes) = intent_for(state, request);
    let purpose = request.purpose.unwrap_or(Purpose::ProfessionalResearch);
    let stages = intent.stages();
    let mut result = NetworkResult {
        query: request.query.trim().to_string(),
        criteria: intent.criteria(),
        status: ResultStatus::Complete,
        companies: Vec::new(),
        jobs: Vec::new(),
        people: Vec::new(),
        connections: Vec::new(),
        connections_outcome: ConnectionsOutcome::NotRequested,
        rows: Vec::new(),
        stages: Vec::new(),
        notes: Vec::new(),
        retrieved_at: now,
        policy_version: POLICY_VERSION.to_string(),
    };
    let deadline = Instant::now() + BUDGET;
    let ctx = state
        .rema_mcp
        .ctx(&state.info.version, cancel, deadline, false);
    let mut failures = 0usize;
    let mut answered = 0usize;

    // Jobs (ReMa's job layer), grouped by employer.
    let mut records = HashMap::new();
    let mut found_companies: Vec<Company> = Vec::new();
    let mut hiring_checked = false;
    if stages.contains(&Stage::Jobs) {
        let jobs = companies::jobs(state, &intent, model, progress, cancel).await;
        if cancel.is_cancelled() {
            return cancelled(result);
        }
        hiring_checked = jobs.searched;
        if jobs.searched {
            answered += 1;
        } else {
            failures += 1;
        }
        if let Some(r) = jobs.report {
            result.stages.push(r);
        }
        records = jobs.records;
        result.jobs = jobs.jobs;
        found_companies.extend(jobs.companies);
    }

    // Companies: the one named, Wikidata, the model's own search.
    if stages.contains(&Stage::Companies) {
        progress.status("Finding companies…");
        let mut sources = Vec::new();
        let mut failed = Vec::new();
        if let Some(name) = &intent.target_company {
            found_companies.push(companies::named(name, now));
        }
        let discovery = intent.company_discovery || !intent.industries.is_empty();
        if discovery && intent.target_company.is_none() {
            match companies::wikidata(&ctx, &intent).await {
                Ok(list) => {
                    if !list.is_empty() {
                        sources.push("Wikidata".to_string());
                    }
                    answered += 1;
                    found_companies.extend(list);
                }
                Err(reason) => {
                    failures += 1;
                    failed.push(reason);
                }
            }
            let wants_model = !intent.industries.is_empty()
                || intent.size.is_some()
                || !stages.contains(&Stage::Jobs);
            if let Some((endpoint, model_id)) =
                model.filter(|(e, _)| crate::retrieval::native::supported(e) && wants_model)
            {
                match companies::search(state, endpoint, model_id, &intent, progress, cancel).await
                {
                    Ok((list, engine, _)) => {
                        sources.push(engine);
                        answered += 1;
                        found_companies.extend(list);
                    }
                    Err(reason) => {
                        failures += 1;
                        failed.push(reason);
                    }
                }
            }
        }
        if cancel.is_cancelled() {
            return cancelled(result);
        }
        let mut merged = resolve::merge_companies(std::mem::take(&mut found_companies));
        let enrich_limit = merged.len().min((intent.limit as usize + 10).min(40));
        let (enrich_failed, known) =
            companies::enrich(state, &ctx, &mut merged[..enrich_limit]).await;
        failed.extend(enrich_failed);
        if known > 0 && !sources.iter().any(|s| s == "Wikidata") {
            sources.push("Wikidata".into());
        }
        let total = merged.len();
        let (kept, dropped) = companies::apply_criteria(merged, &intent, hiring_checked);
        let unverified = kept.iter().filter(|c| !c.unverified.is_empty()).count();
        if unverified > 0 {
            notes.push(format!(
                "{} could not be fully verified against the criteria (shown after the confirmed \
                 matches, with what is unknown).",
                plural(unverified, "company", "companies")
            ));
        }
        if dropped > 0 {
            notes.push(format!(
                "{} left out: a stated location, size or opening count did not match.",
                plural(dropped, "company", "companies")
            ));
        }
        result.stages.push(report(
            Stage::Companies,
            format!(
                "{} kept of {total} found",
                plural(kept.len(), "company", "companies")
            ),
            sources,
            failed,
        ));
        found_companies = kept;
    } else {
        let mut merged = resolve::merge_companies(std::mem::take(&mut found_companies));
        if stages.contains(&Stage::People) {
            // Team pages need the companies' own sites.
            let n = merged.len().min(12);
            let _ = companies::enrich(state, &ctx, &mut merged[..n]).await;
        }
        let (kept, _) = companies::apply_criteria(merged, &intent, hiring_checked);
        found_companies = kept;
    }
    // Jobs follow their companies.
    let kept_ids: Vec<String> = found_companies.iter().map(|c| c.id.clone()).collect();
    if !found_companies.is_empty() {
        result.jobs.retain(|j| {
            j.company_id
                .as_ref()
                .is_some_and(|id| kept_ids.contains(id))
        });
    }
    result.companies = found_companies;

    // People.
    if stages.contains(&Stage::People) && !result.companies.is_empty() {
        let found = people::find(
            state,
            &ctx,
            &intent,
            &result.companies,
            &result.jobs,
            &records,
            model,
            progress,
            cancel,
        )
        .await;
        if cancel.is_cancelled() {
            return cancelled(result);
        }
        if found.sources.is_empty() && !found.failed.is_empty() {
            failures += 1;
        } else {
            answered += 1;
        }
        if found.unspecialized > 0 {
            notes.push(format!(
                "{} left out: no public evidence that they recruit for these roles (a recruiter \
                 title alone is not evidence of a specialization).",
                plural(found.unspecialized, "recruiter", "recruiters")
            ));
        }
        if result.companies.len() > 12 {
            notes.push(
                "People were researched for the first 12 companies; ask about a company to see \
                 more."
                    .into(),
            );
        }
        result.stages.push(report(
            Stage::People,
            format!(
                "{} found",
                plural(found.people.len(), "relevant person", "relevant people")
            ),
            found.sources,
            found.failed,
        ));
        // The policy decides what may be shown for this purpose.
        let (kept, removed) = policy::keep(found.people, purpose, Operation::Display, |p| {
            (p.source, p.class)
        });
        if removed > 0 {
            notes.push(format!(
                "{} not shown: the provider's terms do not allow this use.",
                plural(removed, "person", "people")
            ));
        }
        result.people = kept;
    }

    // Connections (only where a provider shares them).
    if stages.contains(&Stage::Connections) {
        progress.status("Checking your permitted connections…");
        if intent.second_degree {
            notes.push(
                "ReMa cannot look up second-degree connections: LinkedIn only shares the \
                 connections of the signed-in member (first degree), and ReMa does not simulate \
                 them."
                    .into(),
            );
        }
        let providers = capabilities::all(state).await.unwrap_or_default();
        result.connections_outcome = match capabilities::relationship_access(&providers) {
            RelationshipAccess::Unavailable(reason) => ConnectionsOutcome::Unavailable { reason },
            RelationshipAccess::Available(provider)
                if !policy::check(
                    policy::DataSource::LinkedinApi,
                    DataClass::FirstDegreeConnection,
                    purpose,
                    Operation::Fetch,
                )
                .allowed =>
            {
                let _ = provider;
                ConnectionsOutcome::Unavailable {
                    reason: "Connection data from LinkedIn cannot be used for this purpose.".into(),
                }
            }
            RelationshipAccess::Available(provider) => {
                match relationships::connections(state).await {
                    Ok(members) => {
                        let matched = relationships::match_companies(&members, &result.companies);
                        relationships::mark_people(&mut result.people, &matched);
                        let outcome = ConnectionsOutcome::Checked {
                            provider,
                            checked: members.len() as u32,
                            matched: matched.len() as u32,
                        };
                        result.connections = matched;
                        outcome
                    }
                    Err(error) => ConnectionsOutcome::Failed {
                        reason: error.to_string(),
                    },
                }
            }
        };
        result.stages.push(report(
            Stage::Connections,
            match &result.connections_outcome {
                ConnectionsOutcome::Checked { matched, .. } => format!(
                    "{} at these companies",
                    plural(
                        *matched as usize,
                        "first-degree connection",
                        "first-degree connections"
                    )
                ),
                ConnectionsOutcome::Unavailable { .. } => "Connection list not available".into(),
                ConnectionsOutcome::Failed { .. } => "The connection list could not be read".into(),
                ConnectionsOutcome::NotRequested => String::new(),
            },
            match &result.connections_outcome {
                ConnectionsOutcome::Checked { .. } => vec!["LinkedIn".into()],
                _ => Vec::new(),
            },
            match &result.connections_outcome {
                ConnectionsOutcome::Failed { reason } => vec![reason.clone()],
                _ => Vec::new(),
            },
        ));
    }

    result.rows = rows(&result);
    result.notes = notes;
    if started.elapsed() >= BUDGET {
        result
            .notes
            .push("The research reached its time limit; results may be incomplete.".into());
    }
    result.status = if answered == 0 && failures > 0 {
        ResultStatus::Failed
    } else if result.companies.is_empty()
        && result.people.is_empty()
        && result.connections.is_empty()
    {
        ResultStatus::NoVerifiedMatches
    } else if failures > 0 || result.stages.iter().any(|s| !s.failed.is_empty()) {
        ResultStatus::Partial
    } else {
        ResultStatus::Complete
    };
    result
}

fn cancelled(mut result: NetworkResult) -> NetworkResult {
    result.status = ResultStatus::Cancelled;
    result
}

/// The unified table: company → job → person (NC §21, §29).
pub fn rows(result: &NetworkResult) -> Vec<Row> {
    let mut out = Vec::new();
    for company in &result.companies {
        let jobs: Vec<&str> = result
            .jobs
            .iter()
            .filter(|j| j.company_id.as_deref() == Some(company.id.as_str()))
            .map(|j| j.id.as_str())
            .collect();
        let people: Vec<_> = result
            .people
            .iter()
            .filter(|p| p.company_id.as_deref() == Some(company.id.as_str()))
            .collect();
        if people.is_empty() {
            out.push(Row {
                company_id: Some(company.id.clone()),
                job_id: jobs.first().map(|j| j.to_string()),
                person_id: None,
            });
        }
        for person in people {
            out.push(Row {
                company_id: Some(company.id.clone()),
                job_id: person
                    .job_id
                    .clone()
                    .or_else(|| jobs.first().map(|j| j.to_string())),
                person_id: Some(person.id.clone()),
            });
        }
        if out.len() >= 150 {
            break;
        }
    }
    out
}

/// The result as a model or a stored record may see it (NC §38, §39;
/// B28): data the policy denies for that use is removed, and so are the
/// session-only connections — only their count remains.
pub fn view_for(result: &NetworkResult, purpose: Purpose, operation: Operation) -> NetworkResult {
    let mut out = result.clone();
    let (people, removed) = policy::keep(out.people, purpose, operation, |p| (p.source, p.class));
    out.people = people;
    let allowed = policy::check(
        policy::DataSource::LinkedinApi,
        DataClass::FirstDegreeConnection,
        purpose,
        operation,
    )
    .allowed;
    if !allowed {
        out.connections.clear();
        for person in &mut out.people {
            person.relationship = None;
        }
    }
    if removed > 0 {
        out.notes.push(format!(
            "{} left out of this copy: the provider's terms do not allow this use.",
            plural(removed, "person", "people")
        ));
    }
    out.rows = rows(&out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::connectors::ProviderId;
    use crate::network::model::{Connection, Relationship};
    use crate::network::policy::Persistence;

    fn result_with_connection() -> NetworkResult {
        let company = companies::named("Nordlicht AI", 1);
        let relationship = Relationship {
            provider: ProviderId::Linkedin,
            degree: 1,
            label: "LinkedIn first-degree connection".into(),
            fetched_at: 1,
            persistence: Persistence::Session,
        };
        NetworkResult {
            query: "Do I know anyone at Nordlicht AI?".into(),
            criteria: Default::default(),
            status: ResultStatus::Complete,
            companies: vec![company.clone()],
            jobs: vec![],
            people: vec![],
            connections: vec![Connection {
                name: "Jane Example".into(),
                headline: Some("ML Engineer at Nordlicht AI".into()),
                company_id: Some(company.id.clone()),
                company_name: Some(company.name.clone()),
                profile_url: None,
                relationship,
                match_reason: "x".into(),
            }],
            connections_outcome: ConnectionsOutcome::Checked {
                provider: ProviderId::Linkedin,
                checked: 10,
                matched: 1,
            },
            rows: vec![],
            stages: vec![],
            notes: vec![],
            retrieved_at: 1,
            policy_version: POLICY_VERSION.into(),
        }
    }

    #[test]
    fn connections_never_reach_a_model_or_storage() {
        let result = result_with_connection();
        for operation in [Operation::ModelProcess, Operation::Store, Operation::Export] {
            let view = view_for(&result, Purpose::ProfessionalResearch, operation);
            assert!(view.connections.is_empty(), "{operation:?}");
            assert!(!serde_json::to_string(&view)
                .unwrap()
                .contains("Jane Example"));
        }
        let shown = view_for(&result, Purpose::ProfessionalResearch, Operation::Display);
        assert_eq!(
            shown.connections.len(),
            1,
            "shown to the user in the session"
        );
    }

    #[tokio::test]
    async fn a_relationship_question_without_access_says_so_and_checks_nothing() {
        let (state, _) = crate::state::testing::state(std::sync::Arc::new(
            crate::llm::fake::FakeLanguageModel::replying(&[]),
        ));
        let result = research(
            &state,
            &Request {
                query: "Do I know anyone at Nordlicht AI?".into(),
                ..Request::default()
            },
            None,
            &crate::retrieval::Silent,
            &CancellationToken::new(),
        )
        .await;
        match &result.connections_outcome {
            ConnectionsOutcome::Unavailable { reason } => {
                assert!(reason.contains("cannot tell whom you know"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        assert!(result.connections.is_empty());
        assert_eq!(result.companies[0].name, "Nordlicht AI");
        assert!(
            result
                .stages
                .iter()
                .any(|s| s.stage == Stage::Connections
                    && s.summary == "Connection list not available")
        );
    }
}
