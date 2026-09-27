//! The BusinessOrchestrator (B2, B26, B31): one lifecycle for every
//! Business research request from the page, Chat or a scheduled task.
//! The run is stored before work begins, with the offer version, criteria,
//! scoring and data policy versions and safe model metadata; progress is
//! reported as it happens; the result stored is the copy the data policy
//! allows for storage; an interrupted run is reconciled as interrupted,
//! never as complete.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::{
    clients, contracts, fit, gtm, ingest,
    model::{
        BusinessOverview, ClientResults, ClientSearchInput, ContractResults, ContractSearchInput,
        DescribeInput, DescribeResult, GtmPlan, IngestPage, OfferKind, OfferRef, PageStatus,
        RunKind, RunStatus,
    },
    offers,
    store::{self, RunRecord},
};
use crate::{
    error::{AppError, AppResult},
    events::EventSink,
    llm::Endpoint,
    network::policy::POLICY_VERSION,
    retrieval::Progress,
    services::{documents, providers},
    state::AppState,
    time::now_ms,
};

/// Status lines to the Business page.
pub struct PageProgress {
    pub events: Arc<dyn EventSink>,
    pub run_id: String,
}

impl Progress for PageProgress {
    fn status(&self, text: &str) {
        self.events.business_progress(&self.run_id, text);
    }
}

/// The default model, when one is set up (research works without it).
pub async fn default_model(state: &AppState) -> Option<(Endpoint, String)> {
    let model = providers::current_default_model(state).ok().flatten()?;
    let endpoint = providers::resolve_endpoint(state, &model.provider_id)
        .await
        .ok()?;
    Some((endpoint, model.model_id))
}

/// Safe model metadata for run history: provider name and model, never a
/// key, token or address.
pub fn model_label(model: Option<(&Endpoint, &str)>) -> Option<String> {
    model.map(|(e, id)| format!("{} · {id}", e.name))
}

fn begin(state: &AppState, run: &RunRecord) -> AppResult<()> {
    if run.id.trim().is_empty() || run.id.len() > 100 {
        return Err(AppError::validation("A run id is required."));
    }
    state.db.call(|c| store::insert_run(c, run))
}

fn end(
    state: &AppState,
    run_id: &str,
    status: RunStatus,
    result: Option<String>,
    sources: &[String],
    failures: &[String],
) {
    let _ = state.db.call(|c| {
        store::finish_run(
            c,
            run_id,
            status,
            result.as_deref(),
            sources,
            failures,
            now_ms(),
        )
    });
}

/// A failed search that failed for lack of a network is "offline" (B27).
fn offline(failures: &[String]) -> bool {
    !failures.is_empty()
        && failures.iter().all(|f| {
            let f = f.to_lowercase();
            f.contains("could not connect")
                || f.contains("does not resolve")
                || f.contains("offline")
                || f.contains("network error")
        })
}

fn run_record(
    id: &str,
    kind: RunKind,
    offer: Option<&OfferRef>,
    query: &str,
    criteria: String,
    scoring: Option<&str>,
    model: Option<String>,
) -> RunRecord {
    RunRecord {
        id: id.to_string(),
        kind,
        offer_id: offer.map(|o| o.offer_id.clone()),
        offer_version: offer.map(|o| o.version),
        query: crate::analytics::normalize::clip(query.trim(), 2_000),
        criteria,
        scoring_policy: scoring.map(str::to_string),
        data_policy: POLICY_VERSION.into(),
        model,
        status: RunStatus::Running,
        result: None,
        sources: Vec::new(),
        failures: Vec::new(),
        started_at: now_ms(),
        finished_at: None,
    }
}

/// Reads a document the user picked, the way Profile documents are read
/// (PDF, Word, text or Markdown).
fn document_text(path: &str) -> AppResult<String> {
    let (name, bytes) = documents::read_source(std::path::Path::new(path))?;
    let format = documents::detect_format(&name, &bytes)?;
    documents::extract_text(format, &bytes)?.ok_or_else(|| {
        AppError::validation("This document has no readable text. Use a PDF, Word or text file.")
    })
}

/// Product website or description → draft offer (B4). A new offer is
/// created, or an existing offer's draft gets a proposal it can review.
pub async fn describe_offer(
    state: &AppState,
    input: DescribeInput,
    model: Option<(&Endpoint, &str)>,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> AppResult<DescribeResult> {
    let url = input
        .url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(str::to_string);
    let mut text = input
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    if let Some(path) = input
        .document_path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
    {
        let doc = document_text(path)?;
        text = Some(match text {
            Some(t) => format!("{t}\n\n{doc}"),
            None => doc,
        });
    }
    if url.is_none() && text.is_none() {
        return Err(AppError::validation(
            "Give a product or service web address, a description or a document.",
        ));
    }
    let existing = match &input.offer_id {
        Some(id) => Some(offers::get(state, id)?),
        None => None,
    };
    let kind = input
        .kind
        .or(existing.as_ref().map(|o| o.kind))
        .unwrap_or(OfferKind::Service);
    let run = run_record(
        &input.run_id,
        RunKind::OfferIngest,
        None,
        url.as_deref().unwrap_or("Description"),
        "{}".into(),
        None,
        model_label(model),
    );
    begin(state, &run)?;
    let mut pages: Vec<IngestPage> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut sources: Vec<ingest::SourceText> = Vec::new();
    let mut draft = None;
    let mut failures: Vec<String> = Vec::new();
    if let Some(url) = &url {
        match ingest::crawl(state, url, &ingest::LIMITS, progress, cancel).await {
            Ok(crawl) => {
                pages = crawl.report.clone();
                if let Some(stopped) = &crawl.stopped {
                    notes.push(stopped.clone());
                }
                if crawl.pages.is_empty() {
                    failures.push(
                        pages
                            .first()
                            .and_then(|p| p.detail.clone())
                            .unwrap_or_else(|| "The product page could not be read.".into()),
                    );
                } else {
                    let (d, extract_notes) = ingest::extract(&crawl, &input.name, kind);
                    notes.extend(extract_notes);
                    sources.extend(crawl.pages.iter().map(|p| ingest::SourceText {
                        url: Some(p.url.clone()),
                        text: p.text.clone(),
                        retrieved_at: p.retrieved_at,
                    }));
                    draft = Some(d);
                }
            }
            Err(error) => failures.push(error.to_string()),
        }
    }
    if let Some(text) = &text {
        let from_text = ingest::extract_from_text(text, &input.name, kind);
        sources.push(ingest::SourceText {
            url: None,
            text: text.clone(),
            retrieved_at: now_ms(),
        });
        draft = Some(match draft {
            Some(mut d) => {
                for (field, claim) in from_text.claims() {
                    ingest::merge_claims(&mut d, vec![(field.to_string(), claim.clone(), None)]);
                }
                d
            }
            None => from_text,
        });
    }
    if cancel.is_cancelled() {
        end(
            state,
            &input.run_id,
            RunStatus::Cancelled,
            None,
            &[],
            &failures,
        );
        return Err(AppError::validation("Stopped."));
    }
    let mut draft = draft.unwrap_or_else(|| {
        super::model::OfferContent::empty(
            if input.name.trim().is_empty() {
                url.as_deref()
                    .and_then(crate::network::resolve::own_domain)
                    .unwrap_or_else(|| "New offer".into())
            } else {
                input.name.clone()
            }
            .as_str(),
            kind,
        )
    });
    if !input.name.trim().is_empty() {
        draft.name = input.name.trim().to_string();
    }
    if draft.name.trim().is_empty() {
        draft.name = "New offer".into();
    }
    // The model helps extract; only quoted claims survive.
    if let Some((endpoint, model_id)) = model.filter(|_| !sources.is_empty()) {
        progress.status("Extracting claims…");
        match ingest::extract_with_model(state, endpoint, model_id, &sources, &mut draft, cancel)
            .await
        {
            Ok(dropped) if dropped > 0 => notes.push(format!(
                "{dropped} proposed claim(s) were left out: no exact supporting quote from the \
                 pages."
            )),
            Ok(_) => {}
            Err(error) => notes.push(format!(
                "The model could not help with extraction ({error}); ReMa's own reading is shown."
            )),
        }
    }
    let status = if sources.is_empty() {
        RunStatus::Failed
    } else if pages
        .iter()
        .any(|p| !matches!(p.status, PageStatus::Read | PageStatus::Truncated))
        || !failures.is_empty()
    {
        RunStatus::Partial
    } else {
        RunStatus::NeedsReview
    };
    if status == RunStatus::Failed {
        notes.push(
            "ReMa could not read the product site. Describe the offer yourself in the editor; \
             saving works without website access."
                .into(),
        );
    }
    let (offer, diff) = match existing {
        Some(existing) => {
            let base = existing
                .draft
                .clone()
                .or(existing.reviewed.clone())
                .unwrap_or_else(|| super::model::OfferContent::empty(&existing.name, kind));
            let (merged, diff) = offers::merge_refresh(&base, &draft, now_ms());
            // Only against the draft read above: newer edits are never
            // overwritten (the refresh is then dropped and said so).
            match offers::save_draft(state, &existing.id, merged, existing.revision) {
                Ok(offer) => (offer, Some(diff)),
                Err(AppError::Conflict(message)) => {
                    end(
                        state,
                        &input.run_id,
                        RunStatus::Failed,
                        None,
                        &[],
                        std::slice::from_ref(&message),
                    );
                    return Err(AppError::conflict(format!(
                        "{message} The website refresh was not applied."
                    )));
                }
                Err(e) => return Err(e),
            }
        }
        None => (
            offers::create(state, draft, input.idempotency_key.as_deref())?,
            None,
        ),
    };
    let content = offer
        .draft
        .clone()
        .unwrap_or_else(|| super::model::OfferContent::empty(&offer.name, offer.kind));
    let missing = ingest::missing(&content);
    let summary = serde_json::json!({
        "offerId": offer.id,
        "pages": pages,
        "missing": missing,
        "notes": notes,
        "diff": diff,
    });
    end(
        state,
        &input.run_id,
        status,
        Some(summary.to_string()),
        &pages
            .iter()
            .filter(|p| matches!(p.status, PageStatus::Read | PageStatus::Truncated))
            .map(|p| p.url.clone())
            .collect::<Vec<_>>(),
        &failures,
    );
    Ok(DescribeResult {
        run_id: input.run_id,
        status,
        offer,
        pages,
        missing,
        notes,
        diff,
    })
}

/// Find Clients for a reviewed offer version.
pub async fn find_clients(
    state: &AppState,
    input: ClientSearchInput,
    model: Option<(&Endpoint, &str)>,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> AppResult<ClientResults> {
    let (offer, content) = offers::reviewed(state, &input.offer_id, input.offer_version)?;
    progress.status(&format!("Offer: {} v{}", offer.name, offer.version));
    let run = run_record(
        &input.run_id,
        RunKind::Clients,
        Some(&offer),
        &input.query,
        serde_json::to_string(&input.criteria).unwrap_or_default(),
        Some(fit::SCORING_POLICY),
        model_label(model),
    );
    begin(state, &run)?;
    let request = clients::Request {
        query: input.query.clone(),
        criteria: input.criteria.clone(),
        find_people: input.find_people,
    };
    let mut results = clients::find(
        state,
        &input.run_id,
        &request,
        &offer,
        &content,
        model,
        progress,
        cancel,
    )
    .await;
    if results.status == RunStatus::Failed && offline(&results.failures) {
        results.status = RunStatus::Offline;
    }
    // Research snapshots: the assessments of this run (B25).
    let _ = state.db.call(|c| {
        for p in results
            .confirmed
            .iter()
            .chain(&results.needs_verification)
            .chain(&results.excluded)
        {
            store::insert_assessment(c, &p.assessment, Some(&input.run_id), None)?;
        }
        Ok(())
    });
    end(
        state,
        &input.run_id,
        results.status,
        serde_json::to_string(&results).ok(),
        &results.sources,
        &results.failures,
    );
    state.events.business_changed();
    Ok(results)
}

/// Find Contract Work.
pub async fn find_contracts(
    state: &AppState,
    input: ContractSearchInput,
    model: Option<(&Endpoint, &str)>,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> AppResult<ContractResults> {
    let (criteria, notes) = match input.criteria.clone() {
        Some(c) => (c, Vec::new()),
        None => contracts::parse_criteria(&input.query),
    };
    let run = run_record(
        &input.run_id,
        RunKind::Contracts,
        None,
        &input.query,
        serde_json::to_string(&criteria).unwrap_or_default(),
        None,
        model_label(model),
    );
    begin(state, &run)?;
    let mut results =
        contracts::find(state, &input.run_id, criteria, model, progress, cancel).await;
    results.notes.splice(0..0, notes);
    let mut seen = std::collections::HashSet::new();
    results.notes.retain(|n| seen.insert(n.clone()));
    if results.status == RunStatus::Failed && offline(&results.failures) {
        results.status = RunStatus::Offline;
    }
    end(
        state,
        &input.run_id,
        results.status,
        serde_json::to_string(&results).ok(),
        &results.sources,
        &results.failures,
    );
    state.events.business_changed();
    Ok(results)
}

/// Target accounts for a plan's segment through Find Clients (B21).
#[allow(clippy::too_many_arguments)]
pub async fn target_accounts(
    state: &AppState,
    run_id: &str,
    plan_id: &str,
    segment_id: &str,
    model: Option<(&Endpoint, &str)>,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> AppResult<GtmPlan> {
    let plan = gtm::get(state, plan_id)?;
    let segment = plan
        .content
        .segments
        .iter()
        .find(|s| s.id == segment_id)
        .cloned()
        .ok_or_else(|| AppError::not_found("That segment is not in the plan."))?;
    let request = gtm::target_request(&plan, &segment);
    let results = find_clients(
        state,
        ClientSearchInput {
            run_id: run_id.to_string(),
            offer_id: plan.offer.offer_id.clone(),
            offer_version: Some(plan.offer.version),
            query: request.query.clone(),
            criteria: request.criteria.clone(),
            find_people: false,
        },
        model,
        progress,
        cancel,
    )
    .await?;
    if matches!(results.status, RunStatus::Cancelled) {
        return Err(AppError::validation("Stopped."));
    }
    let mut content = plan.content.clone();
    content
        .target_accounts
        .retain(|a| a.segment_id.as_deref() != Some(segment_id));
    content
        .target_accounts
        .extend(gtm::accounts_from(&results, &segment));
    for s in &mut content.segments {
        if s.id == segment_id {
            s.selected = true;
        }
    }
    gtm::save(
        state,
        plan_id,
        &plan.name,
        &plan.geography,
        content,
        plan.revision,
    )
}

/// Reassesses a saved client opportunity against a reviewed offer version
/// (the newest by default): a new assessment is added; earlier ones stay.
pub async fn reassess(
    state: &AppState,
    opportunity_id: &str,
    offer_version: Option<u32>,
    progress: &dyn Progress,
    cancel: &CancellationToken,
) -> AppResult<super::model::Opportunity> {
    let o = state
        .db
        .call(|c| store::opportunity(c, opportunity_id))?
        .ok_or_else(|| AppError::not_found("The opportunity no longer exists."))?;
    let (Some(offer), Some(company_name)) = (o.offer.clone(), o.company_name.clone()) else {
        return Err(AppError::validation(
            "Only client opportunities with a company and an offer can be reassessed.",
        ));
    };
    let (offer, content) = offers::reviewed(state, &offer.offer_id, offer_version)?;
    progress.status(&format!(
        "Reassessing {company_name} for {} v{}…",
        offer.name, offer.version
    ));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let ctx = state
        .rema_mcp
        .ctx(&state.info.version, cancel, deadline, false);
    let mut company = crate::network::companies::named(&company_name, now_ms());
    company.matched_because.clear();
    company.website = o.source_url.clone();
    let mut list = vec![company];
    let _ = crate::network::companies::enrich(state, &ctx, &mut list).await;
    let company = list.remove(0);
    let inspection = clients::inspect(state, &company, deadline, cancel).await;
    if cancel.is_cancelled() {
        return Err(AppError::validation("Stopped."));
    }
    let (assessment, _) = clients::assess(
        &company,
        &inspection,
        &offer,
        &content,
        &super::model::ClientCriteria::default(),
        &super::model::Locations::default(),
        false,
        now_ms(),
    );
    state.db.call(|c| {
        store::insert_assessment(c, &assessment, None, Some(opportunity_id))?;
        c.execute(
            "UPDATE business_opportunities SET last_researched_at = ?1 WHERE id = ?2",
            rusqlite::params![now_ms(), opportunity_id],
        )?;
        Ok(())
    })?;
    state.events.business_changed();
    state
        .db
        .call(|c| store::opportunity(c, opportunity_id))?
        .ok_or_else(|| AppError::not_found("The opportunity no longer exists."))
}

/// The newest stored result of a kind (the page after a restart).
pub fn latest<T: serde::de::DeserializeOwned>(
    state: &AppState,
    kind: RunKind,
) -> AppResult<Option<T>> {
    let run = state.db.call(|c| store::latest_run(c, kind))?;
    Ok(run
        .and_then(|r| r.result)
        .and_then(|text| serde_json::from_str(&text).ok()))
}

/// Runs a restart left queued or running: marked interrupted (B31).
pub fn reconcile(state: &AppState) -> AppResult<usize> {
    state.db.call(|c| store::reconcile_interrupted(c, now_ms()))
}

pub fn overview(state: &AppState) -> AppResult<BusinessOverview> {
    state.db.call(|c| {
        Ok(BusinessOverview {
            profile: store::profile(c)?,
            offers: store::offers(c)?,
            plans: store::plans(c)?,
            experiments: store::experiments(c)?,
            drafts: store::drafts(c)?,
        })
    })
}
