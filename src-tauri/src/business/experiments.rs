//! GTM experiments (B22) and their metrics (B23). An experiment is a
//! versioned hypothesis with a fixed account cohort and variants frozen
//! before any contact; starting it begins recordkeeping only — it sends
//! nothing, schedules nothing and spends nothing.
//!
//! Metrics are computed from recorded activity, never from model prose:
//! account-level, within the cohort and the observation window, each rate
//! shown as numerator / denominator ("Not available" for zero). A reply
//! counts only after a recorded contact; an account's win counts once,
//! for the experiment that first contacted it; ambiguous events stay
//! unassigned.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use specta::Type;

use super::{
    model::{
        Activity, ActivityType, Assignment, CohortEntry, Experiment, ExperimentContent,
        ExperimentMetrics, ExperimentStatus, Opportunity, Rate, Variant, VariantMetrics,
    },
    new_id, store,
};
use crate::{
    analytics::normalize,
    error::{AppError, AppResult},
    state::AppState,
    time::now_ms,
};

const DAY: i64 = 86_400_000;
/// Below this many contacted accounts per variant, no winner is named.
pub const SMALL_COHORT: u32 = 20;

/// The account an opportunity belongs to (one company, whatever the
/// number of people contacted).
pub fn account_key(o: &Opportunity) -> String {
    o.company_key
        .clone()
        .unwrap_or_else(|| format!("opportunity:{}", o.id))
}

/// The variant an account was assigned when the experiment was frozen.
pub fn assigned_variant(e: &Experiment, account: &str) -> Option<String> {
    e.frozen_at?;
    e.content
        .cohort
        .iter()
        .find(|c| c.account_key == account)
        .and_then(|c| c.variant.clone())
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewExperiment {
    pub plan_id: String,
    pub segment_id: Option<String>,
    pub content: ExperimentContent,
    pub idempotency_key: String,
}

fn validate(content: &mut ExperimentContent) -> AppResult<()> {
    content.hypothesis = normalize::clip(content.hypothesis.trim(), 1_000);
    content.channel = normalize::clip(content.channel.trim(), 200);
    content.variants.retain(|v| !v.label.trim().is_empty());
    for (i, v) in content.variants.iter_mut().enumerate() {
        if v.id.trim().is_empty() {
            v.id = format!("v{}", i + 1);
        }
        v.label = normalize::clip(v.label.trim(), 80);
    }
    let mut ids = HashSet::new();
    if !content.variants.iter().all(|v| ids.insert(v.id.clone())) {
        return Err(AppError::validation("Each variant needs its own id."));
    }
    let mut seen = HashSet::new();
    content
        .cohort
        .retain(|c| seen.insert(c.account_key.clone()));
    if content.observation_days == 0 {
        content.observation_days = 21;
    }
    Ok(())
}

pub fn create(state: &AppState, mut input: NewExperiment) -> AppResult<Experiment> {
    validate(&mut input.content)?;
    let plan = state
        .db
        .call(|c| store::plan(c, &input.plan_id))?
        .ok_or_else(|| AppError::not_found("The plan no longer exists."))?;
    if input.idempotency_key.trim().is_empty() {
        return Err(AppError::validation("A request key is required."));
    }
    let now = now_ms();
    let experiment = Experiment {
        id: new_id("exp"),
        plan_id: plan.id.clone(),
        offer: plan.offer.clone(),
        segment_id: input.segment_id.clone(),
        version: 1,
        status: ExperimentStatus::Draft,
        content: input.content,
        frozen_at: None,
        created_at: now,
        updated_at: now,
        revision: 0,
    };
    let key = input.idempotency_key.trim().to_string();
    let (id, _) = state
        .db
        .call(|c| store::insert_experiment(c, &experiment, Some(&key)))?;
    state.events.business_changed();
    get(state, &id)
}

pub fn get(state: &AppState, id: &str) -> AppResult<Experiment> {
    state
        .db
        .call(|c| store::experiment(c, id))?
        .ok_or_else(|| AppError::not_found("The experiment no longer exists."))
}

/// Edits a plan that is not frozen; after freezing only the outcome and
/// limitations may change (a changed threshold or window needs an
/// amendment, B22).
pub fn update(
    state: &AppState,
    id: &str,
    mut content: ExperimentContent,
    expected_revision: u32,
) -> AppResult<Experiment> {
    validate(&mut content)?;
    let mut e = get(state, id)?;
    if e.frozen_at.is_some() {
        let before = &e.content;
        let changed = before.hypothesis != content.hypothesis
            || before.channel != content.channel
            || before.variants != content.variants
            || before.assignment != content.assignment
            || before.cohort != content.cohort
            || before.primary_metric != content.primary_metric
            || before.success_threshold != content.success_threshold
            || before.planned_start != content.planned_start
            || before.planned_end != content.planned_end
            || before.observation_days != content.observation_days;
        if changed {
            return Err(AppError::validation(
                "This experiment is frozen. Create an amendment (a new version) to change its \
                 plan; the original stays as it was run.",
            ));
        }
        e.content.outcome_summary = content.outcome_summary;
        e.content.limitations = content.limitations;
        e.content.stop_conditions = content.stop_conditions;
    } else {
        e.content = content;
    }
    let now = now_ms();
    state
        .db
        .call(|c| store::update_experiment(c, &e, expected_revision, now))?;
    state.events.business_changed();
    get(state, id)
}

/// A reproducible shuffle (the order is stored once frozen).
fn shuffled(mut items: Vec<String>, seed: &str) -> Vec<String> {
    let mut x: u64 = seed.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    for i in (1..items.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let j = (x % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
    items
}

/// Assigns variants to the cohort (B22): randomized by account and
/// balanced, the user's manual choices, or in order (sequential).
pub fn assign(e: &Experiment) -> AppResult<Vec<CohortEntry>> {
    let variants: Vec<&Variant> = e.content.variants.iter().collect();
    if variants.is_empty() {
        return Err(AppError::validation("Add at least one message variant."));
    }
    if e.content.cohort.is_empty() {
        return Err(AppError::validation(
            "Add the accounts of the cohort first.",
        ));
    }
    let mut cohort = e.content.cohort.clone();
    match e.content.assignment {
        Assignment::Manual => {
            if let Some(missing) = cohort.iter().find(|c| {
                c.variant
                    .as_ref()
                    .is_none_or(|v| !variants.iter().any(|x| &x.id == v))
            }) {
                return Err(AppError::validation(format!(
                    "Assign a variant to {} before starting.",
                    missing.account_name
                )));
            }
        }
        Assignment::RandomByAccount => {
            let order = shuffled(
                cohort.iter().map(|c| c.account_key.clone()).collect(),
                &e.id,
            );
            for (i, key) in order.iter().enumerate() {
                if let Some(entry) = cohort.iter_mut().find(|c| &c.account_key == key) {
                    entry.variant = Some(variants[i % variants.len()].id.clone());
                }
            }
        }
        Assignment::Sequential => {
            let per = cohort.len().div_ceil(variants.len());
            for (i, entry) in cohort.iter_mut().enumerate() {
                entry.variant = Some(
                    variants[(i / per.max(1)).min(variants.len() - 1)]
                        .id
                        .clone(),
                );
            }
        }
    }
    Ok(cohort)
}

/// Moves an experiment through draft → planned → running ⇄ paused →
/// completed / cancelled. Running freezes the variants and assignments.
pub fn set_status(
    state: &AppState,
    id: &str,
    status: ExperimentStatus,
    expected_revision: u32,
) -> AppResult<Experiment> {
    use ExperimentStatus::*;
    let mut e = get(state, id)?;
    let allowed = matches!(
        (e.status, status),
        (Draft, Planned)
            | (Planned, Draft)
            | (Planned, Running)
            | (Draft, Running)
            | (Running, Paused)
            | (Paused, Running)
            | (Running, Completed)
            | (Paused, Completed)
            | (Draft | Planned | Running | Paused, Cancelled)
    );
    if !allowed {
        return Err(AppError::validation(format!(
            "An experiment cannot move from {:?} to {:?}.",
            e.status, status
        )));
    }
    if matches!(status, Planned | Running) {
        if e.content.hypothesis.trim().is_empty() {
            return Err(AppError::validation("State the hypothesis first."));
        }
        if e.content.primary_metric.trim().is_empty() {
            return Err(AppError::validation("Choose the primary metric first."));
        }
    }
    if status == Running && e.frozen_at.is_none() {
        e.content.cohort = assign(&e)?;
        e.frozen_at = Some(now_ms());
        if e.content.planned_start.is_none() {
            e.content.planned_start = e.frozen_at;
        }
    }
    e.status = status;
    let now = now_ms();
    state
        .db
        .call(|c| store::update_experiment(c, &e, expected_revision, now))?;
    state.events.business_changed();
    get(state, id)
}

/// A new version of a frozen experiment (B22): the same plan with the
/// cohort's assignments cleared, as a draft; the original is unchanged.
pub fn amend(state: &AppState, id: &str, idempotency_key: &str) -> AppResult<Experiment> {
    let e = get(state, id)?;
    let now = now_ms();
    let mut content = e.content.clone();
    for entry in &mut content.cohort {
        entry.variant = None;
    }
    content.outcome_summary = None;
    let amended = Experiment {
        id: new_id("exp"),
        plan_id: e.plan_id.clone(),
        offer: e.offer.clone(),
        segment_id: e.segment_id.clone(),
        version: e.version + 1,
        status: ExperimentStatus::Draft,
        content,
        frozen_at: None,
        created_at: now,
        updated_at: now,
        revision: 0,
    };
    let (new_id, _) = state.db.call(|c| {
        store::insert_experiment(c, &amended, Some(&format!("amend:{id}:{idempotency_key}")))
    })?;
    state.events.business_changed();
    get(state, &new_id)
}

pub fn delete(state: &AppState, id: &str) -> AppResult<()> {
    state.db.call(|c| store::delete_experiment(c, id))?;
    state.events.business_changed();
    Ok(())
}

pub fn rate(numerator: u32, denominator: u32) -> Rate {
    if denominator == 0 {
        return Rate {
            numerator,
            denominator,
            value: None,
            display: "Not available".into(),
        };
    }
    let value = f64::from(numerator) / f64::from(denominator);
    Rate {
        numerator,
        denominator,
        value: Some(value),
        display: format!("{numerator} / {denominator} ({:.0}%)", value * 100.0),
    }
}

/// The evaluation window: from freezing (or the planned start, if later)
/// to the planned end plus the observation period.
pub fn window(e: &Experiment) -> (Option<i64>, Option<i64>) {
    let Some(frozen) = e.frozen_at else {
        return (None, None);
    };
    let start = e.content.planned_start.map_or(frozen, |s| s.max(frozen));
    let end = e.content.planned_end.unwrap_or(start).max(start)
        + i64::from(e.content.observation_days) * DAY;
    (Some(start), Some(end))
}

#[derive(Default)]
struct AccountFacts {
    contacted: bool,
    replied: bool,
    positive: bool,
    met: bool,
    proposed: bool,
    won: bool,
    people: HashSet<String>,
}

/// Account-level metrics from recorded activity (B23).
pub fn compute(
    e: &Experiment,
    opportunities: &[Opportunity],
    all_experiments: &[Experiment],
    now: i64,
) -> ExperimentMetrics {
    let (start, end) = window(e);
    let mut unattributed = Vec::new();
    let cohort: HashMap<String, Option<String>> = e
        .content
        .cohort
        .iter()
        .map(|c| (c.account_key.clone(), c.variant.clone()))
        .collect();
    // Activities per cohort account.
    let mut by_account: HashMap<String, Vec<&Activity>> = HashMap::new();
    for o in opportunities {
        let key = account_key(o);
        if cohort.contains_key(&key) {
            by_account
                .entry(key)
                .or_default()
                .extend(o.activities.iter());
        }
    }
    let in_window = |a: &Activity| match (start, end) {
        (Some(s), Some(t)) => a.occurred_at >= s && a.occurred_at <= t,
        _ => false,
    };
    // Which experiment first contacted each account (primary attribution).
    let first_contact_experiment = |acts: &[&Activity]| {
        acts.iter()
            .filter(|a| a.kind == ActivityType::Contact && a.experiment_id.is_some())
            .min_by_key(|a| a.occurred_at)
            .and_then(|a| a.experiment_id.clone())
    };
    let mut facts: HashMap<String, AccountFacts> = HashMap::new();
    for (account, acts) in &by_account {
        let mut f = AccountFacts::default();
        let contacts: Vec<&&Activity> = acts
            .iter()
            .filter(|a| {
                a.kind == ActivityType::Contact
                    && a.experiment_id.as_deref() == Some(e.id.as_str())
                    && in_window(a)
            })
            .collect();
        let first = contacts.iter().map(|a| a.occurred_at).min();
        if let Some(first) = first {
            f.contacted = true;
            for c in &contacts {
                f.people
                    .insert(c.person.clone().unwrap_or_else(|| "(unnamed)".into()));
            }
            let other_experiments: HashSet<String> = acts
                .iter()
                .filter(|a| a.kind == ActivityType::Contact)
                .filter_map(|a| a.experiment_id.clone())
                .filter(|x| x != &e.id)
                .collect();
            for a in acts.iter().filter(|a| {
                !matches!(
                    a.kind,
                    ActivityType::Contact | ActivityType::StageChange | ActivityType::Note
                )
            }) {
                let tagged_here = a.experiment_id.as_deref() == Some(e.id.as_str());
                let tagged_elsewhere = a.experiment_id.is_some() && !tagged_here;
                if tagged_elsewhere {
                    continue;
                }
                if !tagged_here && !other_experiments.is_empty() {
                    unattributed.push(format!(
                        "{} for {account}: the account was also contacted in another experiment, \
                         so it is not credited automatically.",
                        a.kind.label()
                    ));
                    continue;
                }
                if a.occurred_at < first {
                    unattributed.push(format!(
                        "{} for {account} happened before the first recorded contact.",
                        a.kind.label()
                    ));
                    continue;
                }
                if !in_window(a) {
                    unattributed.push(format!(
                        "{} for {account} is outside the observation window.",
                        a.kind.label()
                    ));
                    continue;
                }
                match a.kind {
                    ActivityType::Reply => f.replied = true,
                    ActivityType::PositiveReply => {
                        f.replied = true;
                        f.positive = true;
                    }
                    ActivityType::MeetingHeld => f.met = true,
                    ActivityType::ProposalSent => f.proposed = true,
                    ActivityType::Won => {
                        let primary = first_contact_experiment(acts);
                        if primary.as_deref() == Some(e.id.as_str()) {
                            f.won = true;
                        } else {
                            unattributed.push(format!(
                                "The win at {account} is credited to the experiment that first \
                                 contacted it."
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        facts.insert(account.clone(), f);
    }
    let summarize = |variant: Option<&str>| -> VariantMetrics {
        let accounts: Vec<&String> = cohort
            .iter()
            .filter(|(_, v)| variant.is_none() || v.as_deref() == variant)
            .map(|(k, _)| k)
            .collect();
        let count = |pred: &dyn Fn(&AccountFacts) -> bool| {
            accounts
                .iter()
                .filter(|k| facts.get(**k).is_some_and(pred))
                .count() as u32
        };
        let contacted = count(&|f| f.contacted);
        let replied = count(&|f| f.contacted && f.replied);
        let positive = count(&|f| f.contacted && f.positive);
        let met = count(&|f| f.contacted && f.met);
        let proposed = count(&|f| f.contacted && f.proposed);
        let won = count(&|f| f.contacted && f.won);
        VariantMetrics {
            variant: variant.map(str::to_string),
            accounts_in_cohort: accounts.len() as u32,
            accounts_contacted: contacted,
            accounts_replied: replied,
            accounts_positive_reply: positive,
            accounts_met: met,
            accounts_proposed: proposed,
            accounts_won: won,
            reply_rate: rate(replied, contacted),
            positive_reply_rate: rate(positive, contacted),
            meeting_rate: rate(met, contacted),
            proposal_rate: rate(proposed, contacted),
            win_rate: rate(won, contacted),
            people_contacted: accounts
                .iter()
                .filter_map(|k| facts.get(*k))
                .map(|f| f.people.len() as u32)
                .sum(),
        }
    };
    let overall = summarize(None);
    let variants: Vec<VariantMetrics> = e
        .content
        .variants
        .iter()
        .map(|v| summarize(Some(&v.id)))
        .collect();
    let mut limitations =
        vec!["Activity is user-reported; ReMa does not verify it independently.".to_string()];
    if e.frozen_at.is_none() {
        limitations.push("The experiment has not started: nothing is counted yet.".into());
    }
    if e.content.assignment != Assignment::RandomByAccount {
        limitations.push(
            "Non-random assignment: differences between variants are observational; they do not \
             show that a message caused them."
                .into(),
        );
    }
    if variants.iter().any(|v| v.accounts_contacted < SMALL_COHORT) {
        limitations.push(format!(
            "Small cohort (fewer than {SMALL_COHORT} contacted accounts per variant): differences \
             may be chance; no winning variant is established."
        ));
    }
    if let Some(end) = end {
        if now < end {
            limitations.push(format!(
                "The observation window is still open (ends {}); follow-up is incomplete.",
                normalize::date_of(end)
            ));
        }
    }
    let _ = all_experiments;
    unattributed.sort();
    unattributed.dedup();
    ExperimentMetrics {
        experiment_id: e.id.clone(),
        window_start: start,
        window_end: end,
        overall,
        variants,
        limitations,
        unattributed,
        computed_at: now,
    }
}

pub fn metrics(state: &AppState, id: &str) -> AppResult<ExperimentMetrics> {
    let e = get(state, id)?;
    let (opportunities, experiments) = state
        .db
        .call(|c| Ok((store::opportunities(c)?, store::experiments(c)?)))?;
    Ok(compute(&e, &opportunities, &experiments, now_ms()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::model::{OfferRef, OpportunityKind, PipelineStage};

    fn experiment(assignment: Assignment, accounts: &[&str]) -> Experiment {
        Experiment {
            id: "exp_1".into(),
            plan_id: "plan_1".into(),
            offer: OfferRef {
                offer_id: "off_1".into(),
                version: 1,
                name: "Offer".into(),
            },
            segment_id: None,
            version: 1,
            status: ExperimentStatus::Draft,
            content: ExperimentContent {
                hypothesis: "Ops leads reply to a workflow question".into(),
                channel: "Direct professional outreach".into(),
                variants: vec![
                    Variant {
                        id: "a".into(),
                        label: "A".into(),
                        message: "x".into(),
                    },
                    Variant {
                        id: "b".into(),
                        label: "B".into(),
                        message: "y".into(),
                    },
                    Variant {
                        id: "c".into(),
                        label: "C".into(),
                        message: "z".into(),
                    },
                ],
                assignment,
                cohort: accounts
                    .iter()
                    .map(|k| CohortEntry {
                        account_key: (*k).into(),
                        account_name: (*k).into(),
                        opportunity_id: None,
                        variant: None,
                    })
                    .collect(),
                primary_metric: "reply_rate".into(),
                success_threshold: Some("At least 20% replies".into()),
                planned_start: None,
                planned_end: None,
                observation_days: 30,
                sample_target: Some(30),
                budget: None,
                effort_budget: None,
                stop_conditions: vec![],
                exclusions: vec![],
                outcome_summary: None,
                limitations: vec![],
            },
            frozen_at: None,
            created_at: 0,
            updated_at: 0,
            revision: 0,
        }
    }

    fn opportunity(account: &str, activities: Vec<Activity>) -> Opportunity {
        Opportunity {
            id: format!("opp_{account}"),
            kind: OpportunityKind::Service,
            name: account.into(),
            company_key: Some(account.into()),
            company_name: Some(account.into()),
            offer: None,
            canonical_job_id: None,
            source_url: None,
            use_case: String::new(),
            stage: PipelineStage::Contacted,
            archived: false,
            do_not_contact: false,
            contacts: vec![],
            evidence: vec![],
            amount: None,
            contract: None,
            listing_status: None,
            next_step: String::new(),
            notes: String::new(),
            created_at: 0,
            updated_at: 0,
            last_researched_at: None,
            last_commercial_activity_at: None,
            revision: 0,
            assessment: None,
            assessments: 0,
            activities,
        }
    }

    fn act(
        kind: ActivityType,
        at: i64,
        experiment: Option<&str>,
        person: Option<&str>,
    ) -> Activity {
        Activity {
            id: new_id("act"),
            opportunity_id: String::new(),
            kind,
            person: person.map(str::to_string),
            occurred_at: at,
            recorded_at: at,
            source: "user_reported".into(),
            detail: None,
            from_stage: None,
            to_stage: None,
            experiment_id: experiment.map(str::to_string),
            variant: None,
        }
    }

    #[test]
    fn random_assignment_is_stable_balanced_and_by_account() {
        let accounts: Vec<String> = (0..30).map(|i| format!("acct{i}")).collect();
        let refs: Vec<&str> = accounts.iter().map(String::as_str).collect();
        let e = experiment(Assignment::RandomByAccount, &refs);
        let first = assign(&e).unwrap();
        let second = assign(&e).unwrap();
        assert_eq!(first, second, "the same experiment assigns the same way");
        for v in ["a", "b", "c"] {
            assert_eq!(
                first
                    .iter()
                    .filter(|c| c.variant.as_deref() == Some(v))
                    .count(),
                10
            );
        }
        let manual = experiment(Assignment::Manual, &["x"]);
        assert!(
            assign(&manual).is_err(),
            "manual needs every account assigned"
        );
    }

    #[test]
    fn scenario_four_two_replies_from_five_contacts_is_forty_percent() {
        let accounts = ["a1", "a2", "a3", "a4", "a5", "a6"];
        let mut e = experiment(Assignment::RandomByAccount, &accounts);
        e.content.cohort = assign(&e).unwrap();
        e.frozen_at = Some(1_000);
        let day = DAY;
        let mut opps = Vec::new();
        for (i, a) in accounts.iter().enumerate().take(5) {
            let mut acts = vec![act(
                ActivityType::Contact,
                1_000 + day,
                Some("exp_1"),
                Some("P1"),
            )];
            if i == 0 {
                // A second person at the same account: still one account.
                acts.push(act(
                    ActivityType::Contact,
                    1_000 + day,
                    Some("exp_1"),
                    Some("P2"),
                ));
                acts.push(act(ActivityType::Reply, 1_000 + 2 * day, None, Some("P1")));
            }
            if i == 1 {
                acts.push(act(
                    ActivityType::PositiveReply,
                    1_000 + 3 * day,
                    Some("exp_1"),
                    None,
                ));
            }
            opps.push(opportunity(a, acts));
        }
        opps.push(opportunity("a6", vec![]));
        let m = compute(&e, &opps, &[], 1_000 + 5 * day);
        assert_eq!(m.overall.accounts_in_cohort, 6);
        assert_eq!(m.overall.accounts_contacted, 5);
        assert_eq!(m.overall.reply_rate.display, "2 / 5 (40%)");
        assert_eq!(m.overall.positive_reply_rate.display, "1 / 5 (20%)");
        assert_eq!(m.overall.meeting_rate.display, "0 / 5 (0%)");
        assert_eq!(m.overall.people_contacted, 6);
        assert!(m.limitations.iter().any(|l| l.contains("user-reported")));
        assert!(m.limitations.iter().any(|l| l.contains("Small cohort")));
    }

    #[test]
    fn zero_denominators_early_replies_and_duplicate_wins() {
        let e0 = experiment(Assignment::Sequential, &["a"]);
        let m = compute(&e0, &[], &[], 0);
        assert_eq!(m.overall.reply_rate.display, "Not available");
        assert!(m.limitations.iter().any(|l| l.contains("Non-random")));

        let mut e = experiment(Assignment::RandomByAccount, &["a", "b"]);
        e.content.cohort = assign(&e).unwrap();
        e.frozen_at = Some(10 * DAY);
        let opps = vec![
            // A reply before the contact is not attributed.
            opportunity(
                "a",
                vec![
                    act(ActivityType::Reply, 11 * DAY, Some("exp_1"), None),
                    act(ActivityType::Contact, 12 * DAY, Some("exp_1"), None),
                ],
            ),
            // Contacted first by another experiment: the win is theirs.
            opportunity(
                "b",
                vec![
                    act(ActivityType::Contact, 5 * DAY, Some("exp_0"), None),
                    act(ActivityType::Contact, 12 * DAY, Some("exp_1"), None),
                    act(ActivityType::Won, 13 * DAY, Some("exp_1"), None),
                ],
            ),
        ];
        let m = compute(&e, &opps, &[], 50 * DAY);
        assert_eq!(m.overall.accounts_contacted, 2);
        assert_eq!(m.overall.accounts_replied, 0);
        assert_eq!(m.overall.accounts_won, 0);
        assert!(m
            .unattributed
            .iter()
            .any(|u| u.contains("before the first recorded contact")));
        assert!(m
            .unattributed
            .iter()
            .any(|u| u.contains("first contacted it")));
    }
}
