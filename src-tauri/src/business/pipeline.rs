//! The Business Pipeline (B15, B16, B23, B29): one commercial source of
//! truth, separate from job Applications.
//!
//! ```text
//! New Lead → Qualified → Contacted → Discussion → Proposal → Won / Lost
//! ```
//!
//! Stages move only by the user's action. Contacted, Discussion, Proposal,
//! Won and Lost rest on actual, user-reported activity; a search, a draft,
//! a copied message or an opened link never moves a lead. Archived and Do
//! not contact are flags, not stages; research and drafting cannot undo a
//! suppression.

use serde::Deserialize;
use specta::Type;

use super::{
    clients, contracts,
    model::{
        Activity, ActivityType, Amount, ClientResults, ContactRef, ContractResults, OfferKind,
        Opportunity, OpportunityKind, Pipeline, PipelineStage, SourceNote,
    },
    new_id,
    store::{self, NewOpportunity},
};
use crate::{
    error::{AppError, AppResult},
    network::resolve,
    state::AppState,
    time::now_ms,
};

/// The contact id of a buyer role (no named person) at one company:
/// suppressing it must not reach the same role at other companies.
pub fn role_contact_id(company_key: &str, role: &str) -> String {
    format!("role:{company_key}:{}", role.trim().to_lowercase())
}

fn get(state: &AppState, id: &str) -> AppResult<Opportunity> {
    state
        .db
        .call(|c| store::opportunity(c, id))?
        .ok_or_else(|| AppError::not_found("The opportunity no longer exists."))
}

fn run_result<T: serde::de::DeserializeOwned>(state: &AppState, run_id: &str) -> AppResult<T> {
    let run = state
        .db
        .call(|c| store::run(c, run_id))?
        .ok_or_else(|| AppError::not_found("That research run no longer exists."))?;
    let result = run
        .result
        .ok_or_else(|| AppError::validation("That research run has no results to save."))?;
    serde_json::from_str(&result)
        .map_err(|_| AppError::database("The research result could not be read."))
}

/// Saves a prospect from a Find Clients run (the user's action). Saving
/// the same company, offer and use case again returns the existing
/// opportunity with the new assessment attached.
pub fn save_prospect(
    state: &AppState,
    run_id: &str,
    company_key: &str,
    use_case: Option<&str>,
) -> AppResult<(Opportunity, bool)> {
    let results: ClientResults = run_result(state, run_id)?;
    let prospect = results
        .confirmed
        .iter()
        .chain(&results.needs_verification)
        .chain(&results.excluded)
        .find(|p| p.company_key == company_key)
        .ok_or_else(|| AppError::not_found("That company is not in the research results."))?
        .clone();
    let offer_kind = state
        .db
        .call(|c| store::offer(c, &results.offer.offer_id))?
        .map(|o| o.kind);
    let use_case = use_case.map(str::trim).unwrap_or("").to_string();
    let contacts: Vec<ContactRef> = prospect
        .contacts
        .iter()
        .map(|c| ContactRef {
            id: match &c.name {
                Some(name) => resolve::person_id(name, Some(&prospect.company_name)),
                None => role_contact_id(&prospect.company_key, &c.role),
            },
            name: c.name.clone(),
            title: c.title.clone(),
            role: c.role.clone(),
            profile_url: c.profile_url.clone(),
            source_url: c.source_url.clone().or_else(|| c.contact_page.clone()),
        })
        .collect();
    let mut evidence = prospect.assessment.evidence.clone();
    for signal in &prospect.observed_signals {
        evidence.push(SourceNote {
            label: "Observed signal".into(),
            url: None,
            excerpt: Some(signal.clone()),
            retrieved_at: results.retrieved_at,
            contrary: false,
        });
    }
    let new = NewOpportunity {
        kind: match offer_kind {
            Some(OfferKind::DigitalProduct) => OpportunityKind::Product,
            _ => OpportunityKind::Service,
        },
        name: if use_case.is_empty() {
            format!("{} — {}", prospect.company_name, results.offer.name)
        } else {
            format!("{} — {use_case}", prospect.company_name)
        },
        company_key: Some(prospect.company_key.clone()),
        company_name: Some(prospect.company_name.clone()),
        offer: Some(results.offer.clone()),
        canonical_job_id: None,
        source_url: prospect.website.clone(),
        use_case: use_case.clone(),
        contacts,
        evidence,
        amount: None,
        contract: None,
        listing_status: None,
        idempotency_key: clients::identity_key(
            &prospect.company_key,
            &results.offer.offer_id,
            &use_case,
        ),
        researched_at: Some(results.retrieved_at),
    };
    let now = now_ms();
    let (id, created) = state.db.call(|c| {
        let (id, created) = store::save_opportunity(c, &new, now)?;
        store::attach_assessment(c, &prospect.assessment, &id)?;
        Ok((id, created))
    })?;
    state.events.business_changed();
    Ok((get(state, &id)?, created))
}

/// Saves a listing from a Find Contract Work run (never an employment
/// Application).
pub fn save_contract(state: &AppState, run_id: &str, key: &str) -> AppResult<(Opportunity, bool)> {
    let results: ContractResults = run_result(state, run_id)?;
    let listing = results
        .confirmed
        .iter()
        .chain(&results.needs_verification)
        .chain(&results.not_matching)
        .find(|r| r.key == key)
        .ok_or_else(|| AppError::not_found("That listing is not in the search results."))?
        .clone();
    let t = &listing.terms;
    let amount = match (t.rate_min, t.rate_max) {
        (None, None) => None,
        (lo, hi) => Some(Amount {
            value: match (lo, hi) {
                (Some(a), Some(b)) if (a - b).abs() < f64::EPSILON => format!("{a}"),
                (Some(a), Some(b)) => format!("{a}–{b}"),
                (Some(a), None) => format!("from {a}"),
                (None, Some(b)) => format!("up to {b}"),
                (None, None) => String::new(),
            },
            currency: t.currency.clone(),
            basis: t
                .rate_unit
                .map(|u| {
                    format!(
                        "per {}",
                        match u {
                            super::model::RateUnit::Hour => "hour",
                            super::model::RateUnit::Day => "day",
                            super::model::RateUnit::Month => "month",
                            super::model::RateUnit::Project => "project",
                        }
                    )
                })
                .unwrap_or_else(|| "unit not stated".into()),
            source: "advertised".into(),
        }),
    };
    let now = now_ms();
    let new = NewOpportunity {
        kind: OpportunityKind::Contract,
        name: listing.title.clone(),
        company_key: t.end_client.as_deref().map(resolve::company_key),
        // An undisclosed end client stays undisclosed.
        company_name: t.end_client.clone(),
        offer: None,
        canonical_job_id: Some(listing.key.clone()),
        source_url: Some(listing.url.clone()),
        use_case: String::new(),
        contacts: Vec::new(),
        evidence: vec![SourceNote {
            label: format!("Listing ({})", listing.source),
            url: Some(listing.url.clone()),
            excerpt: listing.scope.clone(),
            retrieved_at: results.retrieved_at,
            contrary: false,
        }],
        amount,
        contract: Some(t.clone()),
        listing_status: Some(format!(
            "Open when found ({})",
            crate::analytics::normalize::date_of(results.retrieved_at)
        )),
        idempotency_key: contracts::identity_key(&listing),
        researched_at: Some(results.retrieved_at),
    };
    let (id, created) = state.db.call(|c| store::save_opportunity(c, &new, now))?;
    state.events.business_changed();
    Ok((get(state, &id)?, created))
}

/// An opportunity the user adds by hand.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManualOpportunity {
    pub kind: OpportunityKind,
    pub name: String,
    pub company_name: Option<String>,
    pub offer_id: Option<String>,
    pub use_case: String,
    pub source_url: Option<String>,
    pub amount: Option<Amount>,
    pub idempotency_key: String,
}

pub fn create_manual(state: &AppState, input: ManualOpportunity) -> AppResult<Opportunity> {
    if input.name.trim().is_empty() {
        return Err(AppError::validation("Give the opportunity a name."));
    }
    if input.idempotency_key.trim().is_empty() {
        return Err(AppError::validation("A request key is required."));
    }
    let offer = match &input.offer_id {
        Some(id) => {
            let offer = state
                .db
                .call(|c| store::offer(c, id))?
                .ok_or_else(|| AppError::not_found("The offer no longer exists."))?;
            let version = offer.current_version.ok_or_else(|| {
                AppError::validation("Review the offer before linking opportunities to it.")
            })?;
            Some(state.db.call(|c| store::offer_ref(c, id, version))?)
        }
        None => None,
    };
    let company_name = input
        .company_name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    let new = NewOpportunity {
        kind: input.kind,
        name: input.name.trim().to_string(),
        company_key: company_name.as_deref().map(resolve::company_key),
        company_name,
        offer,
        canonical_job_id: None,
        source_url: input
            .source_url
            .as_deref()
            .and_then(crate::analytics::normalize::web_url),
        use_case: input.use_case.trim().to_string(),
        contacts: Vec::new(),
        evidence: Vec::new(),
        amount: input.amount,
        contract: None,
        listing_status: None,
        idempotency_key: format!("manual:{}", input.idempotency_key.trim()),
        researched_at: None,
    };
    let now = now_ms();
    let (id, _) = state.db.call(|c| store::save_opportunity(c, &new, now))?;
    state.events.business_changed();
    get(state, &id)
}

fn rank(stage: PipelineStage) -> u8 {
    match stage {
        PipelineStage::NewLead => 0,
        PipelineStage::Qualified => 1,
        PipelineStage::Contacted => 2,
        PipelineStage::Discussion => 3,
        PipelineStage::Proposal => 4,
        PipelineStage::Won | PipelineStage::Lost => 5,
    }
}

/// The activity a stage rests on (B15).
fn required(stage: PipelineStage) -> &'static [ActivityType] {
    match stage {
        PipelineStage::NewLead | PipelineStage::Qualified => &[],
        PipelineStage::Contacted => &[ActivityType::Contact],
        PipelineStage::Discussion => &[
            ActivityType::Reply,
            ActivityType::PositiveReply,
            ActivityType::MeetingHeld,
        ],
        PipelineStage::Proposal => &[ActivityType::ProposalSent],
        PipelineStage::Won => &[ActivityType::Won],
        PipelineStage::Lost => &[ActivityType::Lost],
    }
}

/// A stage change the user makes.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StageChange {
    pub to: PipelineStage,
    /// Required for moving back, reopening or switching Won/Lost.
    pub reason: Option<String>,
    /// What actually happened, when the stage needs it and none is
    /// recorded yet (e.g. a meeting for Discussion).
    pub activity: Option<ActivityType>,
    /// When it happened (not in the future).
    pub occurred_at: Option<i64>,
    pub person: Option<String>,
    /// For Won: the accepted value (not money received).
    pub amount: Option<Amount>,
    /// The experiment the recorded activity belongs to; the variant comes
    /// from the frozen assignment, never from the caller.
    pub experiment_id: Option<String>,
    pub expected_revision: u32,
    pub idempotency_key: String,
}

/// The experiment an activity is attributed to, with the account's variant
/// from the frozen assignment (B23): only for an account in its cohort.
fn attribution(
    state: &AppState,
    opportunity: &Opportunity,
    experiment_id: Option<&String>,
) -> AppResult<(Option<String>, Option<String>)> {
    let Some(eid) = experiment_id else {
        return Ok((None, None));
    };
    let experiment = state
        .db
        .call(|c| store::experiment(c, eid))?
        .ok_or_else(|| AppError::not_found("The experiment no longer exists."))?;
    let account = super::experiments::account_key(opportunity);
    let variant = super::experiments::assigned_variant(&experiment, &account);
    if variant.is_none()
        && !experiment
            .content
            .cohort
            .iter()
            .any(|e| e.account_key == account)
    {
        return Err(AppError::validation(
            "This account is not in the experiment's cohort.",
        ));
    }
    Ok((Some(eid.clone()), variant))
}

pub fn change_stage(state: &AppState, id: &str, change: StageChange) -> AppResult<Opportunity> {
    let opportunity = get(state, id)?;
    let from = opportunity.stage;
    let to = change.to;
    if from == to {
        return Err(AppError::validation(format!(
            "The opportunity is already in {}.",
            to.label()
        )));
    }
    let reason = change
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string);
    let backward = rank(to) < rank(from) || rank(from) == 5;
    if backward && reason.is_none() {
        return Err(AppError::validation(
            "Say why the opportunity moves back or reopens; the reason is kept in its history.",
        ));
    }
    if opportunity.do_not_contact
        && matches!(
            to,
            PipelineStage::Contacted | PipelineStage::Discussion | PipelineStage::Proposal
        )
    {
        return Err(AppError::validation(
            "This opportunity is marked Do not contact. Lift the suppression yourself before \
             recording contact.",
        ));
    }
    let now = now_ms();
    let occurred_at = change.occurred_at.unwrap_or(now);
    if occurred_at > now + 86_400_000 {
        return Err(AppError::validation(
            "Only record what already happened: a planned meeting is not a meeting held.",
        ));
    }
    let needs = required(to);
    let supported = needs.is_empty()
        || opportunity
            .activities
            .iter()
            .any(|a| needs.contains(&a.kind));
    let record = if supported {
        None
    } else {
        let kind = match change.activity {
            Some(kind) if needs.contains(&kind) => kind,
            _ if needs.len() == 1 => needs[0],
            _ => {
                return Err(AppError::validation(format!(
                    "Record what actually happened to move to {}: {}.",
                    to.label(),
                    needs
                        .iter()
                        .map(|k| k.label().to_lowercase())
                        .collect::<Vec<_>>()
                        .join(", or ")
                )))
            }
        };
        let (experiment_id, variant) =
            attribution(state, &opportunity, change.experiment_id.as_ref())?;
        Some(Activity {
            id: new_id("act"),
            opportunity_id: id.to_string(),
            kind,
            person: change.person.clone(),
            occurred_at,
            recorded_at: now,
            source: "user_reported".into(),
            detail: match (&change.amount, kind) {
                (Some(a), ActivityType::Won) => Some(format!(
                    "Accepted value {} {} ({}); not money received.",
                    a.currency.as_deref().unwrap_or(""),
                    a.value,
                    a.basis
                )),
                _ => reason.clone(),
            },
            from_stage: None,
            to_stage: None,
            experiment_id,
            variant,
        })
    };
    let audit = Activity {
        id: new_id("act"),
        opportunity_id: id.to_string(),
        kind: ActivityType::StageChange,
        person: None,
        occurred_at: now,
        recorded_at: now,
        source: "user_reported".into(),
        detail: reason.clone(),
        from_stage: Some(from),
        to_stage: Some(to),
        experiment_id: None,
        variant: None,
    };
    let key = change.idempotency_key.trim().to_string();
    if key.is_empty() {
        return Err(AppError::validation("A request key is required."));
    }
    let amount = change.amount.clone();
    state.db.call(|c| {
        let tx = c.transaction()?;
        // A retried request finds its audit entry and changes nothing.
        let (_, new) = store::insert_activity(&tx, &audit, &format!("{key}:stage"))?;
        if !new {
            return Ok(());
        }
        if let Some(activity) = &record {
            store::insert_activity(&tx, activity, &format!("{key}:activity"))?;
        }
        store::set_stage(&tx, id, to, change.expected_revision, now)?;
        if let (PipelineStage::Won, Some(amount)) = (to, amount) {
            tx.execute(
                "UPDATE business_opportunities SET amount = ?1 WHERE id = ?2",
                rusqlite::params![serde_json::to_string(&amount).unwrap_or_default(), id],
            )?;
        }
        tx.commit()?;
        Ok(())
    })?;
    state.events.business_changed();
    get(state, id)
}

/// Actual activity the user reports (B23).
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewActivity {
    pub kind: ActivityType,
    pub person: Option<String>,
    pub occurred_at: i64,
    pub detail: Option<String>,
    /// The experiment this contact belongs to; the variant comes from the
    /// frozen assignment, never from the caller.
    pub experiment_id: Option<String>,
    pub idempotency_key: String,
}

pub fn record_activity(
    state: &AppState,
    opportunity_id: &str,
    input: NewActivity,
) -> AppResult<(Opportunity, bool)> {
    let opportunity = get(state, opportunity_id)?;
    if matches!(input.kind, ActivityType::StageChange) {
        return Err(AppError::validation(
            "Change the stage with its own action.",
        ));
    }
    let now = now_ms();
    if input.occurred_at > now + 86_400_000 {
        return Err(AppError::validation(
            "Only record what already happened: a scheduled meeting is not a meeting held.",
        ));
    }
    if input.idempotency_key.trim().is_empty() {
        return Err(AppError::validation("A request key is required."));
    }
    let (experiment_id, variant) = attribution(state, &opportunity, input.experiment_id.as_ref())?;
    let activity = Activity {
        id: new_id("act"),
        opportunity_id: opportunity_id.to_string(),
        kind: input.kind,
        person: input
            .person
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty()),
        occurred_at: input.occurred_at,
        recorded_at: now,
        source: "user_reported".into(),
        detail: input
            .detail
            .map(|d| crate::analytics::normalize::clip(d.trim(), 1_000))
            .filter(|d| !d.is_empty()),
        from_stage: None,
        to_stage: None,
        experiment_id,
        variant,
    };
    let key = input.idempotency_key.trim().to_string();
    let (_, created) = state
        .db
        .call(|c| store::insert_activity(c, &activity, &key))?;
    if created {
        state.events.business_changed();
    }
    Ok((get(state, opportunity_id)?, created))
}

pub fn delete_activity(
    state: &AppState,
    opportunity_id: &str,
    activity_id: &str,
) -> AppResult<Opportunity> {
    let opportunity = get(state, opportunity_id)?;
    let activity = opportunity
        .activities
        .iter()
        .find(|a| a.id == activity_id)
        .ok_or_else(|| AppError::not_found("That activity no longer exists."))?;
    if activity.kind == ActivityType::StageChange {
        return Err(AppError::validation(
            "Stage history is kept; change the stage again instead.",
        ));
    }
    state.db.call(|c| store::delete_activity(c, activity_id))?;
    state.events.business_changed();
    get(state, opportunity_id)
}

pub fn edit(
    state: &AppState,
    id: &str,
    edit: store::OpportunityEdit,
    expected_revision: u32,
) -> AppResult<Opportunity> {
    state
        .db
        .call(|c| store::edit_opportunity(c, id, &edit, expected_revision, now_ms()))?;
    state.events.business_changed();
    get(state, id)
}

/// Marks the opportunity's company Do not contact (a suppression research
/// and drafting cannot undo).
pub fn do_not_contact(state: &AppState, id: &str, reason: Option<&str>) -> AppResult<Opportunity> {
    let opportunity = get(state, id)?;
    let now = now_ms();
    state.db.call(|c| {
        match (&opportunity.company_key, &opportunity.company_name) {
            (Some(key), Some(name)) => {
                store::suppress(c, "company", key, name, reason, now)?;
            }
            _ => {
                store::suppress(
                    c,
                    "company",
                    &format!("opportunity:{id}"),
                    &opportunity.name,
                    reason,
                    now,
                )?;
            }
        }
        store::set_do_not_contact(c, id, now)
    })?;
    state.events.business_changed();
    get(state, id)
}

/// Marks one contact Do not contact.
pub fn suppress_contact(
    state: &AppState,
    id: &str,
    contact_id: &str,
    reason: Option<&str>,
) -> AppResult<Opportunity> {
    let opportunity = get(state, id)?;
    let contact = opportunity
        .contacts
        .iter()
        .find(|c| c.id == contact_id)
        .ok_or_else(|| AppError::not_found("That contact is not on this opportunity."))?;
    let who = contact.name.clone().unwrap_or_else(|| contact.role.clone());
    let label = match &opportunity.company_name {
        Some(company) => format!("{who} at {company}"),
        None => who,
    };
    state
        .db
        .call(|c| store::suppress(c, "person", contact_id, &label, reason, now_ms()).map(|_| ()))?;
    state.events.business_changed();
    get(state, id)
}

/// Lifts a suppression: only by the user's explicit action.
pub fn lift_suppression(state: &AppState, suppression_id: &str) -> AppResult<()> {
    let now = now_ms();
    state.db.call(|c| {
        if let Some(s) = store::remove_suppression(c, suppression_id)? {
            if s.scope == "company" {
                c.execute(
                    "UPDATE business_opportunities SET do_not_contact = 0, updated_at = ?1,
                         revision = revision + 1
                     WHERE company_key = ?2 OR ?2 = 'opportunity:' || id",
                    rusqlite::params![now, s.key],
                )?;
            }
            store::record_redaction(
                c,
                None,
                "suppression_lifted",
                "A Do not contact entry was lifted by the user.",
                now,
            )?;
        }
        Ok(())
    })?;
    state.events.business_changed();
    Ok(())
}

/// Deletes a contact and what depends on it (B29): drafts addressed to
/// them are deleted, a person's name is removed from stored research, and
/// a record without the data notes the deletion.
pub fn delete_contact(
    state: &AppState,
    id: &str,
    contact_id: &str,
    expected_revision: u32,
) -> AppResult<Opportunity> {
    let now = now_ms();
    state.db.call(|c| {
        let tx = c.transaction()?;
        let removed = store::remove_contact(&tx, id, contact_id, expected_revision, now)?;
        let mut drafts = 0;
        let mut redacted = 0;
        match removed.as_ref().map(|r| (r.name.clone(), r.role.clone())) {
            Some((Some(name), _)) => {
                drafts = tx.execute(
                    "DELETE FROM business_drafts WHERE opportunity_id = ?1 AND recipient LIKE ?2",
                    rusqlite::params![id, format!("%{name}%")],
                )?;
                redacted = store::redact_text(&tx, &name, now)?;
            }
            // A buyer role names nobody: only the drafts addressed to it go.
            Some((None, role)) => {
                drafts = tx.execute(
                    "DELETE FROM business_drafts
                     WHERE opportunity_id = ?1 AND recipient = ?2 COLLATE NOCASE",
                    rusqlite::params![id, role],
                )?;
            }
            None => {}
        }
        store::record_redaction(
            &tx,
            Some(id),
            "contact_deleted",
            &format!(
                "A contact was deleted: {drafts} draft(s) deleted, {redacted} stored record(s) \
                 redacted."
            ),
            now,
        )?;
        tx.commit()?;
        Ok(())
    })?;
    state.events.business_changed();
    get(state, id)
}

pub fn delete(state: &AppState, id: &str) -> AppResult<()> {
    let now = now_ms();
    state.db.call(|c| {
        store::delete_opportunity(c, id)?;
        store::record_redaction(
            c,
            None,
            "opportunity_deleted",
            "An opportunity was deleted with its activity, drafts and assessments.",
            now,
        )
    })?;
    state.events.business_changed();
    Ok(())
}

pub fn pipeline(state: &AppState) -> AppResult<Pipeline> {
    let (opportunities, suppressions) = state
        .db
        .call(|c| Ok((store::opportunities(c)?, store::suppressions(c)?)))?;
    let counts = PipelineStage::ALL
        .iter()
        .map(|s| {
            (
                *s,
                opportunities
                    .iter()
                    .filter(|o| !o.archived && o.stage == *s)
                    .count() as u32,
            )
        })
        .collect();
    Ok(Pipeline {
        opportunities,
        counts,
        suppressions,
    })
}

/// Checks a saved listing's availability (B14): a closed or unreachable
/// listing keeps its record, notes and stage; only its status changes.
pub async fn refresh_listing(state: &AppState, id: &str) -> AppResult<Opportunity> {
    let opportunity = get(state, id)?;
    let url = opportunity
        .source_url
        .clone()
        .filter(|_| opportunity.kind == OpportunityKind::Contract)
        .ok_or_else(|| AppError::validation("Only saved listings can be checked."))?;
    let session = crate::rema_mcp::engine::Session {
        state: state.clone(),
        discovery: crate::rema_mcp::engine::Discovery::own(),
    };
    let input = crate::rema_mcp::contract::GetJobInput {
        id: None,
        url: Some(url),
        refresh: Some(true),
        description_cursor: None,
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    let now = now_ms();
    let date = crate::analytics::normalize::date_of(now);
    let status = match crate::rema_mcp::engine::get_job(&session, &input, &cancel).await {
        Ok(detail) if detail.stale => {
            format!("Not reachable when checked ({date}); last known open")
        }
        Ok(_) => format!("Open when checked ({date})"),
        Err(e)
            if matches!(
                e.code,
                crate::rema_mcp::contract::ErrorCode::JobNotFound
                    | crate::rema_mcp::contract::ErrorCode::JobExpired
            ) =>
        {
            format!("Closed when checked ({date})")
        }
        Err(_) => format!("Not reachable when checked ({date})"),
    };
    state
        .db
        .call(|c| store::set_listing_status(c, id, &status, now))?;
    state.events.business_changed();
    get(state, id)
}
