//! ReMa Business: offers, Find Clients, Find Contract Work, the Pipeline
//! and Go-to-Market Studio. Every persistent action takes an idempotency
//! key or the revision it was made against; nothing here sends anything.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    business::{
        experiments::{self, NewExperiment},
        gtm::{self, DraftRequest, NewPlan, PlanPart},
        model::{
            BusinessOverview, BusinessProfile, BusinessRun, ClientResults, ClientSearchInput,
            ContractResults, ContractSearchInput, DescribeInput, DescribeResult, Draft, Experiment,
            ExperimentContent, ExperimentMetrics, ExperimentStatus, GtmPlan, Offer, OfferContent,
            Opportunity, Pipeline, PlanContent, Redaction, RunKind,
        },
        offers,
        pipeline::{self, ManualOpportunity, NewActivity, StageChange},
        service::{self, PageProgress},
        store::{self, OpportunityEdit},
    },
    error::{AppError, AppResult},
    state::AppState,
    time::now_ms,
};

/// The page's last results (kept with their runs).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LastBusinessResults {
    pub clients: Option<ClientResults>,
    pub contracts: Option<ContractResults>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedOpportunity {
    pub opportunity: Opportunity,
    /// False when the opportunity already existed (saved again).
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanUpdate {
    pub name: String,
    pub geography: String,
    pub content: PlanContent,
    pub expected_revision: u32,
}

fn progress(state: &AppState, run_id: &str) -> PageProgress {
    PageProgress {
        events: state.events.clone(),
        run_id: run_id.to_string(),
    }
}

// ── Overview, profile and offers ─────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub fn business_overview(state: State<'_, AppState>) -> AppResult<BusinessOverview> {
    service::overview(&state)
}

#[tauri::command]
#[specta::specta]
pub fn business_save_profile(
    state: State<'_, AppState>,
    profile: BusinessProfile,
) -> AppResult<BusinessProfile> {
    let mut profile = profile;
    profile.languages.retain(|l| !l.trim().is_empty());
    state
        .db
        .call(|c| store::save_profile(c, &profile, now_ms()))?;
    state.events.business_changed();
    state.db.call(|c| store::profile(c))
}

#[tauri::command]
#[specta::specta]
pub fn business_create_offer(
    state: State<'_, AppState>,
    content: OfferContent,
    idempotency_key: String,
) -> AppResult<Offer> {
    offers::create(&state, content, Some(&idempotency_key))
}

#[tauri::command]
#[specta::specta]
pub fn business_save_offer_draft(
    state: State<'_, AppState>,
    offer_id: String,
    content: OfferContent,
    expected_revision: u32,
) -> AppResult<Offer> {
    offers::save_draft(&state, &offer_id, content, expected_revision)
}

/// Saves the reviewed draft as the next immutable version (the user's
/// explicit review).
#[tauri::command]
#[specta::specta]
pub fn business_review_offer(
    state: State<'_, AppState>,
    offer_id: String,
    expected_revision: u32,
) -> AppResult<Offer> {
    offers::review(&state, &offer_id, expected_revision)
}

#[tauri::command]
#[specta::specta]
pub fn business_archive_offer(
    state: State<'_, AppState>,
    offer_id: String,
    archived: bool,
    expected_revision: u32,
) -> AppResult<Offer> {
    state
        .db
        .call(|c| store::set_offer_archived(c, &offer_id, archived, expected_revision, now_ms()))?;
    state.events.business_changed();
    offers::get(&state, &offer_id)
}

#[tauri::command]
#[specta::specta]
pub fn business_delete_offer(state: State<'_, AppState>, offer_id: String) -> AppResult<()> {
    state.db.call(|c| store::delete_offer(c, &offer_id))?;
    state.events.business_changed();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn business_offer_version(
    state: State<'_, AppState>,
    offer_id: String,
    version: u32,
) -> AppResult<OfferContent> {
    offers::reviewed(&state, &offer_id, Some(version)).map(|(_, c)| c)
}

/// A product URL or description → a draft offer (or a refresh proposal
/// for an existing one). A document is only read through
/// [`business_describe_offer_document`]: the page never passes file paths.
#[tauri::command]
#[specta::specta]
pub async fn business_describe_offer(
    state: State<'_, AppState>,
    input: DescribeInput,
) -> AppResult<DescribeResult> {
    let mut input = input;
    input.document_path = None;
    describe(&state, input).await
}

/// Lets the user pick a document (PDF, Word, text or Markdown) with the
/// system file dialog and reads it into a draft offer. `None` if the user
/// cancelled.
#[tauri::command]
#[specta::specta]
pub async fn business_describe_offer_document(
    app: AppHandle,
    state: State<'_, AppState>,
    input: DescribeInput,
) -> AppResult<Option<DescribeResult>> {
    let picked = app
        .dialog()
        .file()
        .set_title("Choose a document about your product or service")
        .add_filter("Documents", &["pdf", "docx", "txt", "md", "markdown"])
        .blocking_pick_file()
        .and_then(|picked| picked.into_path().ok());
    let Some(path) = picked else {
        return Ok(None);
    };
    let mut input = input;
    input.url = None;
    input.document_path = Some(path.to_string_lossy().into_owned());
    describe(&state, input).await.map(Some)
}

async fn describe(state: &AppState, input: DescribeInput) -> AppResult<DescribeResult> {
    let state: AppState = state.clone();
    let cancel = state.business.begin(&input.run_id);
    let progress = progress(&state, &input.run_id);
    let model = service::default_model(&state).await;
    let run_id = input.run_id.clone();
    let result = service::describe_offer(
        &state,
        input,
        model.as_ref().map(|(e, m)| (e, m.as_str())),
        &progress,
        &cancel,
    )
    .await;
    state.business.end(&run_id);
    result
}

// ── Research ─────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn business_find_clients(
    state: State<'_, AppState>,
    input: ClientSearchInput,
) -> AppResult<ClientResults> {
    let state: AppState = (*state).clone();
    let cancel = state.business.begin(&input.run_id);
    let progress = progress(&state, &input.run_id);
    let model = service::default_model(&state).await;
    let run_id = input.run_id.clone();
    let result = service::find_clients(
        &state,
        input,
        model.as_ref().map(|(e, m)| (e, m.as_str())),
        &progress,
        &cancel,
    )
    .await;
    state.business.end(&run_id);
    result
}

#[tauri::command]
#[specta::specta]
pub async fn business_find_contracts(
    state: State<'_, AppState>,
    input: ContractSearchInput,
) -> AppResult<ContractResults> {
    let state: AppState = (*state).clone();
    let cancel = state.business.begin(&input.run_id);
    let progress = progress(&state, &input.run_id);
    let model = service::default_model(&state).await;
    let run_id = input.run_id.clone();
    let result = service::find_contracts(
        &state,
        input,
        model.as_ref().map(|(e, m)| (e, m.as_str())),
        &progress,
        &cancel,
    )
    .await;
    state.business.end(&run_id);
    result
}

#[tauri::command]
#[specta::specta]
pub fn business_cancel(state: State<'_, AppState>, run_id: String) -> bool {
    state.business.cancel(&run_id)
}

#[tauri::command]
#[specta::specta]
pub fn business_last_results(state: State<'_, AppState>) -> AppResult<LastBusinessResults> {
    Ok(LastBusinessResults {
        clients: service::latest(&state, RunKind::Clients)?,
        contracts: service::latest(&state, RunKind::Contracts)?,
    })
}

#[tauri::command]
#[specta::specta]
pub fn business_runs(state: State<'_, AppState>, limit: u32) -> AppResult<Vec<BusinessRun>> {
    state.db.call(|c| store::runs(c, limit.clamp(1, 200)))
}

// ── Pipeline ─────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub fn business_pipeline(state: State<'_, AppState>) -> AppResult<Pipeline> {
    pipeline::pipeline(&state)
}

#[tauri::command]
#[specta::specta]
pub fn business_save_prospect(
    state: State<'_, AppState>,
    run_id: String,
    company_key: String,
    use_case: Option<String>,
) -> AppResult<SavedOpportunity> {
    let (opportunity, created) =
        pipeline::save_prospect(&state, &run_id, &company_key, use_case.as_deref())?;
    Ok(SavedOpportunity {
        opportunity,
        created,
    })
}

#[tauri::command]
#[specta::specta]
pub fn business_save_contract(
    state: State<'_, AppState>,
    run_id: String,
    key: String,
) -> AppResult<SavedOpportunity> {
    let (opportunity, created) = pipeline::save_contract(&state, &run_id, &key)?;
    Ok(SavedOpportunity {
        opportunity,
        created,
    })
}

#[tauri::command]
#[specta::specta]
pub fn business_create_opportunity(
    state: State<'_, AppState>,
    input: ManualOpportunity,
) -> AppResult<Opportunity> {
    pipeline::create_manual(&state, input)
}

#[tauri::command]
#[specta::specta]
pub fn business_edit_opportunity(
    state: State<'_, AppState>,
    id: String,
    edit: OpportunityEdit,
    expected_revision: u32,
) -> AppResult<Opportunity> {
    pipeline::edit(&state, &id, edit, expected_revision)
}

#[tauri::command]
#[specta::specta]
pub fn business_change_stage(
    state: State<'_, AppState>,
    id: String,
    change: StageChange,
) -> AppResult<Opportunity> {
    pipeline::change_stage(&state, &id, change)
}

#[tauri::command]
#[specta::specta]
pub fn business_record_activity(
    state: State<'_, AppState>,
    id: String,
    activity: NewActivity,
) -> AppResult<SavedOpportunity> {
    let (opportunity, created) = pipeline::record_activity(&state, &id, activity)?;
    Ok(SavedOpportunity {
        opportunity,
        created,
    })
}

#[tauri::command]
#[specta::specta]
pub fn business_delete_activity(
    state: State<'_, AppState>,
    id: String,
    activity_id: String,
) -> AppResult<Opportunity> {
    pipeline::delete_activity(&state, &id, &activity_id)
}

#[tauri::command]
#[specta::specta]
pub fn business_do_not_contact(
    state: State<'_, AppState>,
    id: String,
    reason: Option<String>,
) -> AppResult<Opportunity> {
    pipeline::do_not_contact(&state, &id, reason.as_deref())
}

#[tauri::command]
#[specta::specta]
pub fn business_suppress_contact(
    state: State<'_, AppState>,
    id: String,
    contact_id: String,
    reason: Option<String>,
) -> AppResult<Opportunity> {
    pipeline::suppress_contact(&state, &id, &contact_id, reason.as_deref())
}

/// Lifts a Do-not-contact entry (only ever the user's own action).
#[tauri::command]
#[specta::specta]
pub fn business_lift_suppression(
    state: State<'_, AppState>,
    suppression_id: String,
) -> AppResult<()> {
    pipeline::lift_suppression(&state, &suppression_id)
}

#[tauri::command]
#[specta::specta]
pub fn business_delete_contact(
    state: State<'_, AppState>,
    id: String,
    contact_id: String,
    expected_revision: u32,
) -> AppResult<Opportunity> {
    pipeline::delete_contact(&state, &id, &contact_id, expected_revision)
}

#[tauri::command]
#[specta::specta]
pub fn business_delete_opportunity(state: State<'_, AppState>, id: String) -> AppResult<()> {
    pipeline::delete(&state, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn business_refresh_listing(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<Opportunity> {
    let state: AppState = (*state).clone();
    pipeline::refresh_listing(&state, &id).await
}

#[tauri::command]
#[specta::specta]
pub async fn business_reassess(
    state: State<'_, AppState>,
    id: String,
    run_id: String,
    offer_version: Option<u32>,
) -> AppResult<Opportunity> {
    let state: AppState = (*state).clone();
    let cancel = state.business.begin(&run_id);
    let progress = progress(&state, &run_id);
    let result = service::reassess(&state, &id, offer_version, &progress, &cancel).await;
    state.business.end(&run_id);
    result
}

#[tauri::command]
#[specta::specta]
pub fn business_redactions(state: State<'_, AppState>) -> AppResult<Vec<Redaction>> {
    state.db.call(|c| store::redactions(c))
}

// ── Go-to-Market Studio ──────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub fn business_create_plan(state: State<'_, AppState>, input: NewPlan) -> AppResult<GtmPlan> {
    gtm::create(&state, input)
}

#[tauri::command]
#[specta::specta]
pub fn business_save_plan(
    state: State<'_, AppState>,
    id: String,
    update: PlanUpdate,
) -> AppResult<GtmPlan> {
    gtm::save(
        &state,
        &id,
        &update.name,
        &update.geography,
        update.content,
        update.expected_revision,
    )
}

#[tauri::command]
#[specta::specta]
pub fn business_delete_plan(state: State<'_, AppState>, id: String) -> AppResult<()> {
    gtm::delete(&state, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn business_research_plan(
    state: State<'_, AppState>,
    id: String,
    part: PlanPart,
    run_id: String,
) -> AppResult<GtmPlan> {
    let state: AppState = (*state).clone();
    let cancel = state.business.begin(&run_id);
    let progress = progress(&state, &run_id);
    let model = service::default_model(&state).await;
    let result = gtm::research(
        &state,
        &id,
        part,
        model.as_ref().map(|(e, m)| (e, m.as_str())),
        &progress,
        &cancel,
    )
    .await;
    state.business.end(&run_id);
    result
}

#[tauri::command]
#[specta::specta]
pub async fn business_target_accounts(
    state: State<'_, AppState>,
    plan_id: String,
    segment_id: String,
    run_id: String,
) -> AppResult<GtmPlan> {
    let state: AppState = (*state).clone();
    let cancel = state.business.begin(&run_id);
    let progress = progress(&state, &run_id);
    let model = service::default_model(&state).await;
    let result = service::target_accounts(
        &state,
        &run_id,
        &plan_id,
        &segment_id,
        model.as_ref().map(|(e, m)| (e, m.as_str())),
        &progress,
        &cancel,
    )
    .await;
    state.business.end(&run_id);
    result
}

/// A positioning draft from reviewed capabilities (never changes the
/// offer).
#[tauri::command]
#[specta::specta]
pub fn business_positioning(
    state: State<'_, AppState>,
    plan_id: String,
    segment_id: String,
    alternative: Option<u32>,
) -> AppResult<String> {
    let plan = gtm::get(&state, &plan_id)?;
    let (offer, content) =
        offers::reviewed(&state, &plan.offer.offer_id, Some(plan.offer.version))?;
    let segment = plan
        .content
        .segments
        .iter()
        .find(|s| s.id == segment_id)
        .ok_or_else(|| AppError::not_found("That segment is not in the plan."))?;
    let alternative = alternative.and_then(|i| plan.content.alternatives.get(i as usize));
    Ok(gtm::positioning(&offer, &content, segment, alternative))
}

#[tauri::command]
#[specta::specta]
pub async fn business_create_draft(
    state: State<'_, AppState>,
    request: DraftRequest,
    run_id: String,
) -> AppResult<Draft> {
    let state: AppState = (*state).clone();
    let cancel = state.business.begin(&run_id);
    let model = service::default_model(&state).await;
    let result = gtm::draft(
        &state,
        request,
        model.as_ref().map(|(e, m)| (e, m.as_str())),
        &cancel,
    )
    .await;
    state.business.end(&run_id);
    result
}

#[tauri::command]
#[specta::specta]
pub fn business_update_draft(
    state: State<'_, AppState>,
    id: String,
    subject: Option<String>,
    body: String,
    expected_revision: u32,
) -> AppResult<Draft> {
    state.db.call(|c| {
        store::update_draft_text(
            c,
            &id,
            subject.as_deref(),
            &body,
            expected_revision,
            now_ms(),
        )
    })?;
    state.events.business_changed();
    state
        .db
        .call(|c| store::draft(c, &id))?
        .ok_or_else(|| AppError::not_found("The draft no longer exists."))
}

#[tauri::command]
#[specta::specta]
pub fn business_delete_draft(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.db.call(|c| store::delete_draft(c, &id))?;
    state.events.business_changed();
    Ok(())
}

// ── Experiments ──────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub fn business_create_experiment(
    state: State<'_, AppState>,
    input: NewExperiment,
) -> AppResult<Experiment> {
    experiments::create(&state, input)
}

#[tauri::command]
#[specta::specta]
pub fn business_update_experiment(
    state: State<'_, AppState>,
    id: String,
    content: ExperimentContent,
    expected_revision: u32,
) -> AppResult<Experiment> {
    experiments::update(&state, &id, content, expected_revision)
}

#[tauri::command]
#[specta::specta]
pub fn business_set_experiment_status(
    state: State<'_, AppState>,
    id: String,
    status: ExperimentStatus,
    expected_revision: u32,
) -> AppResult<Experiment> {
    experiments::set_status(&state, &id, status, expected_revision)
}

#[tauri::command]
#[specta::specta]
pub fn business_amend_experiment(
    state: State<'_, AppState>,
    id: String,
    idempotency_key: String,
) -> AppResult<Experiment> {
    experiments::amend(&state, &id, &idempotency_key)
}

#[tauri::command]
#[specta::specta]
pub fn business_delete_experiment(state: State<'_, AppState>, id: String) -> AppResult<()> {
    experiments::delete(&state, &id)
}

#[tauri::command]
#[specta::specta]
pub fn business_experiment_metrics(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<ExperimentMetrics> {
    experiments::metrics(&state, &id)
}
