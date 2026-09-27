//! Business records in ReMa's database (B24): the Business Profile,
//! offers with immutable reviewed versions, research runs and fit
//! assessments, one pipeline of opportunities with user-reported
//! activity, suppressions, local drafts, GTM plans and experiments.
//!
//! IDs are made here, never by a model. Every persistent action takes an
//! idempotency key (a retry returns the first result) and every user edit
//! the revision it was made against (a stale edit is refused, never
//! merged over newer changes).

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{de::DeserializeOwned, Serialize};

use super::{
    model::{
        Activity, ActivityType, Amount, BusinessProfile, BusinessRun, ContactRef, ContractTerms,
        Draft, Experiment, ExperimentContent, ExperimentStatus, FitAssessment, GtmPlan, Offer,
        OfferContent, OfferKind, OfferRef, Opportunity, OpportunityKind, PipelineStage,
        PlanContent, Redaction, RunKind, RunStatus, SourceNote, Suppression,
    },
    new_id,
};
use crate::error::{AppError, AppResult};

fn to_json<T: Serialize + ?Sized>(value: &T) -> AppResult<String> {
    serde_json::to_string(value).map_err(|e| AppError::internal(e.to_string()))
}

fn from_json<T: DeserializeOwned>(text: &str) -> AppResult<T> {
    serde_json::from_str(text)
        .map_err(|e| AppError::database(format!("stored Business data is unreadable: {e}")))
}

fn exists(c: &Connection, table: &str, id: &str) -> AppResult<bool> {
    Ok(c.query_row(
        &format!("SELECT 1 FROM {table} WHERE id = ?1"),
        [id],
        |_| Ok(()),
    )
    .optional()?
    .is_some())
}

/// Why an update changed nothing: the record is gone, or someone changed
/// it since the caller read it.
fn stale(c: &Connection, table: &str, id: &str, what: &str) -> AppError {
    match exists(c, table, id) {
        Ok(true) => AppError::conflict(format!(
            "This {what} changed since you opened it. Reload it and apply your change again."
        )),
        _ => AppError::not_found(format!("The {what} no longer exists.")),
    }
}

// ── Business Profile ─────────────────────────────────────────────────

pub fn profile(c: &Connection) -> AppResult<BusinessProfile> {
    let found = c
        .query_row(
            "SELECT business_name, website, service_area, languages, capacity, availability,
                    constraints, updated_at
             FROM business_profile WHERE id = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((name, website, area, languages, capacity, availability, constraints, at)) = found
    else {
        return Ok(BusinessProfile::default());
    };
    Ok(BusinessProfile {
        business_name: name,
        website,
        service_area: area,
        languages: from_json(&languages)?,
        capacity,
        availability,
        constraints,
        updated_at: at,
    })
}

pub fn save_profile(c: &Connection, p: &BusinessProfile, now: i64) -> AppResult<()> {
    c.execute(
        "INSERT INTO business_profile (id, business_name, website, service_area, languages,
             capacity, availability, constraints, updated_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET business_name = ?1, website = ?2, service_area = ?3,
             languages = ?4, capacity = ?5, availability = ?6, constraints = ?7, updated_at = ?8",
        params![
            p.business_name,
            p.website,
            p.service_area,
            to_json(&p.languages)?,
            p.capacity,
            p.availability,
            p.constraints,
            now
        ],
    )?;
    Ok(())
}

// ── Offers ───────────────────────────────────────────────────────────

/// A new offer with its first draft (the same offer again for a retried
/// key).
pub fn insert_offer(
    c: &Connection,
    draft: &OfferContent,
    idempotency_key: Option<&str>,
    now: i64,
) -> AppResult<String> {
    if let Some(key) = idempotency_key {
        let found: Option<String> = c
            .query_row(
                "SELECT id FROM business_offers WHERE idempotency_key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = found {
            return Ok(id);
        }
    }
    let id = new_id("off");
    c.execute(
        "INSERT INTO business_offers (id, name, kind, current_version, draft, archived,
             idempotency_key, created_at, updated_at, revision)
         VALUES (?1, ?2, ?3, NULL, ?4, 0, ?5, ?6, ?6, 0)",
        params![
            id,
            draft.name,
            draft.kind.as_str(),
            to_json(draft)?,
            idempotency_key,
            now
        ],
    )?;
    Ok(id)
}

type OfferRow = (
    String,
    String,
    String,
    Option<u32>,
    Option<String>,
    bool,
    i64,
    i64,
    u32,
);

fn offer_row(r: &Row) -> rusqlite::Result<OfferRow> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
    ))
}

const OFFER_COLUMNS: &str =
    "id, name, kind, current_version, draft, archived, created_at, updated_at, revision";

fn offer_from(c: &Connection, row: OfferRow) -> AppResult<Offer> {
    let (id, name, kind, current, draft, archived, created, updated, revision) = row;
    let (reviewed, reviewed_at) = match current {
        Some(v) => match offer_version(c, &id, v)? {
            Some((content, at)) => (Some(content), Some(at)),
            None => (None, None),
        },
        None => (None, None),
    };
    Ok(Offer {
        id,
        name,
        kind: OfferKind::parse(&kind).unwrap_or(OfferKind::Service),
        current_version: current,
        draft: draft.as_deref().map(from_json).transpose()?,
        reviewed,
        reviewed_at,
        archived,
        created_at: created,
        updated_at: updated,
        revision,
    })
}

pub fn offer(c: &Connection, id: &str) -> AppResult<Option<Offer>> {
    let row = c
        .query_row(
            &format!("SELECT {OFFER_COLUMNS} FROM business_offers WHERE id = ?1"),
            [id],
            offer_row,
        )
        .optional()?;
    row.map(|r| offer_from(c, r)).transpose()
}

pub fn offers(c: &Connection) -> AppResult<Vec<Offer>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {OFFER_COLUMNS} FROM business_offers ORDER BY archived, updated_at DESC"
    ))?;
    let rows: Vec<OfferRow> = stmt
        .query_map([], offer_row)?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter().map(|r| offer_from(c, r)).collect()
}

/// Every offer's name (archived ones too), for recognizing requests that
/// name the user's own offer.
pub fn offer_names(c: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = c.prepare("SELECT name FROM business_offers")?;
    let names = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(names)
}

/// Saves the draft under review (the reviewed versions do not change).
pub fn save_offer_draft(
    c: &Connection,
    id: &str,
    draft: &OfferContent,
    expected_revision: u32,
    now: i64,
) -> AppResult<()> {
    let changed = c.execute(
        "UPDATE business_offers SET draft = ?1, name = ?2, kind = ?3, updated_at = ?4,
             revision = revision + 1
         WHERE id = ?5 AND revision = ?6",
        params![
            to_json(draft)?,
            draft.name,
            draft.kind.as_str(),
            now,
            id,
            expected_revision
        ],
    )?;
    if changed == 0 {
        return Err(stale(c, "business_offers", id, "offer"));
    }
    Ok(())
}

/// Stores the reviewed content as the next immutable version.
pub fn add_offer_version(
    c: &mut Connection,
    id: &str,
    content: &OfferContent,
    expected_revision: u32,
    now: i64,
) -> AppResult<u32> {
    let tx = c.transaction()?;
    let next: u32 = tx.query_row(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM business_offer_versions WHERE offer_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let changed = tx.execute(
        "UPDATE business_offers SET current_version = ?1, draft = ?2, name = ?3, kind = ?4,
             updated_at = ?5, revision = revision + 1
         WHERE id = ?6 AND revision = ?7",
        params![
            next,
            to_json(content)?,
            content.name,
            content.kind.as_str(),
            now,
            id,
            expected_revision
        ],
    )?;
    if changed == 0 {
        let error = stale(&tx, "business_offers", id, "offer");
        return Err(error);
    }
    tx.execute(
        "INSERT INTO business_offer_versions (offer_id, version, content, reviewed_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![id, next, to_json(content)?, now],
    )?;
    tx.commit()?;
    Ok(next)
}

pub fn offer_version(
    c: &Connection,
    id: &str,
    version: u32,
) -> AppResult<Option<(OfferContent, i64)>> {
    let row: Option<(String, i64)> = c
        .query_row(
            "SELECT content, reviewed_at FROM business_offer_versions
             WHERE offer_id = ?1 AND version = ?2",
            params![id, version],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    row.map(|(content, at)| Ok((from_json(&content)?, at)))
        .transpose()
}

pub fn set_offer_archived(
    c: &Connection,
    id: &str,
    archived: bool,
    expected_revision: u32,
    now: i64,
) -> AppResult<()> {
    let changed = c.execute(
        "UPDATE business_offers SET archived = ?1, updated_at = ?2, revision = revision + 1
         WHERE id = ?3 AND revision = ?4",
        params![archived, now, id, expected_revision],
    )?;
    if changed == 0 {
        return Err(stale(c, "business_offers", id, "offer"));
    }
    Ok(())
}

pub fn delete_offer(c: &Connection, id: &str) -> AppResult<()> {
    c.execute("DELETE FROM business_offers WHERE id = ?1", [id])?;
    Ok(())
}

/// The offer's name for a reference ("Deleted offer" when it is gone).
pub fn offer_ref(c: &Connection, id: &str, version: u32) -> AppResult<OfferRef> {
    let name: Option<String> = c
        .query_row(
            "SELECT name FROM business_offers WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(OfferRef {
        offer_id: id.to_string(),
        version,
        name: name.unwrap_or_else(|| "Deleted offer".into()),
    })
}

// ── Research runs ────────────────────────────────────────────────────

/// A run as stored (the result is the policy-filtered copy).
#[derive(Debug, Clone)]
pub struct RunRecord {
    pub id: String,
    pub kind: RunKind,
    pub offer_id: Option<String>,
    pub offer_version: Option<u32>,
    pub query: String,
    pub criteria: String,
    pub scoring_policy: Option<String>,
    pub data_policy: String,
    pub model: Option<String>,
    pub status: RunStatus,
    pub result: Option<String>,
    pub sources: Vec<String>,
    pub failures: Vec<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

/// Persists a run before any work begins (B31).
pub fn insert_run(c: &Connection, run: &RunRecord) -> AppResult<()> {
    c.execute(
        "INSERT INTO business_research_runs (id, kind, offer_id, offer_version, query, criteria,
             scoring_policy, data_policy, model, status, result, sources, failures, started_at,
             finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            run.id,
            run.kind.as_str(),
            run.offer_id,
            run.offer_version,
            run.query,
            run.criteria,
            run.scoring_policy,
            run.data_policy,
            run.model,
            run.status.as_str(),
            run.result,
            to_json(&run.sources)?,
            to_json(&run.failures)?,
            run.started_at,
            run.finished_at
        ],
    )?;
    Ok(())
}

/// Ends a running run. A run that already ended (reconciled after a
/// restart, say) is left as it is.
pub fn finish_run(
    c: &Connection,
    id: &str,
    status: RunStatus,
    result: Option<&str>,
    sources: &[String],
    failures: &[String],
    now: i64,
) -> AppResult<bool> {
    let changed = c.execute(
        "UPDATE business_research_runs SET status = ?1, result = ?2, sources = ?3, failures = ?4,
             finished_at = ?5
         WHERE id = ?6 AND status IN ('queued', 'running')",
        params![
            status.as_str(),
            result,
            to_json(sources)?,
            to_json(failures)?,
            now,
            id
        ],
    )?;
    Ok(changed > 0)
}

const RUN_COLUMNS: &str = "id, kind, offer_id, offer_version, query, criteria, scoring_policy,
    data_policy, model, status, result, sources, failures, started_at, finished_at";

fn run_row(r: &Row) -> rusqlite::Result<(RunRecord, String, String)> {
    let kind: String = r.get(1)?;
    let status: String = r.get(9)?;
    Ok((
        RunRecord {
            id: r.get(0)?,
            kind: RunKind::parse(&kind),
            offer_id: r.get(2)?,
            offer_version: r.get(3)?,
            query: r.get(4)?,
            criteria: r.get(5)?,
            scoring_policy: r.get(6)?,
            data_policy: r.get(7)?,
            model: r.get(8)?,
            status: RunStatus::parse(&status),
            result: r.get(10)?,
            sources: Vec::new(),
            failures: Vec::new(),
            started_at: r.get(13)?,
            finished_at: r.get(14)?,
        },
        r.get(11)?,
        r.get(12)?,
    ))
}

fn run_from(row: (RunRecord, String, String)) -> AppResult<RunRecord> {
    let (mut run, sources, failures) = row;
    run.sources = from_json(&sources)?;
    run.failures = from_json(&failures)?;
    Ok(run)
}

pub fn run(c: &Connection, id: &str) -> AppResult<Option<RunRecord>> {
    c.query_row(
        &format!("SELECT {RUN_COLUMNS} FROM business_research_runs WHERE id = ?1"),
        [id],
        run_row,
    )
    .optional()?
    .map(run_from)
    .transpose()
}

/// The newest run of a kind that has a result.
pub fn latest_run(c: &Connection, kind: RunKind) -> AppResult<Option<RunRecord>> {
    c.query_row(
        &format!(
            "SELECT {RUN_COLUMNS} FROM business_research_runs
             WHERE kind = ?1 AND result IS NOT NULL ORDER BY started_at DESC LIMIT 1"
        ),
        [kind.as_str()],
        run_row,
    )
    .optional()?
    .map(run_from)
    .transpose()
}

pub fn runs(c: &Connection, limit: u32) -> AppResult<Vec<BusinessRun>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {RUN_COLUMNS} FROM business_research_runs ORDER BY started_at DESC LIMIT ?1"
    ))?;
    let rows: Vec<(RunRecord, String, String)> = stmt
        .query_map([limit], run_row)?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter()
        .map(|row| {
            let run = run_from(row)?;
            let offer = match (&run.offer_id, run.offer_version) {
                (Some(id), Some(v)) => Some(offer_ref(c, id, v)?),
                _ => None,
            };
            Ok(BusinessRun {
                id: run.id,
                kind: run.kind,
                offer,
                query: run.query,
                status: run.status,
                model: run.model,
                sources: run.sources,
                failures: run.failures,
                started_at: run.started_at,
                finished_at: run.finished_at,
            })
        })
        .collect()
}

/// Runs that were queued or running when ReMa stopped: marked as
/// interrupted (never complete). Returns how many.
pub fn reconcile_interrupted(c: &Connection, now: i64) -> AppResult<usize> {
    let mut stmt = c.prepare(
        "SELECT id, failures FROM business_research_runs WHERE status IN ('queued', 'running')",
    )?;
    let open: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, failures) in &open {
        let mut list: Vec<String> = from_json(failures).unwrap_or_default();
        list.push("Interrupted: ReMa closed before this run finished.".into());
        c.execute(
            "UPDATE business_research_runs SET status = 'failed', failures = ?1, finished_at = ?2
             WHERE id = ?3",
            params![to_json(&list)?, now, id],
        )?;
    }
    Ok(open.len())
}

// ── Assessments ──────────────────────────────────────────────────────

pub fn insert_assessment(
    c: &Connection,
    a: &FitAssessment,
    run_id: Option<&str>,
    opportunity_id: Option<&str>,
) -> AppResult<()> {
    c.execute(
        "INSERT OR IGNORE INTO business_assessments (id, run_id, opportunity_id, company_key,
             company_name, offer_id, offer_version, policy_version, content, coverage, score,
             created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            a.id,
            run_id,
            opportunity_id,
            a.company_key,
            a.company_name,
            a.offer.offer_id,
            a.offer.version,
            a.policy_version,
            to_json(a)?,
            a.coverage,
            a.score,
            a.created_at
        ],
    )?;
    Ok(())
}

/// Links an assessment made during research to the opportunity it was
/// saved as (a copy when it already belongs to another).
pub fn attach_assessment(c: &Connection, a: &FitAssessment, opportunity_id: &str) -> AppResult<()> {
    let owner: Option<Option<String>> = c
        .query_row(
            "SELECT opportunity_id FROM business_assessments WHERE id = ?1",
            [&a.id],
            |r| r.get(0),
        )
        .optional()?;
    match owner {
        Some(None) => {
            c.execute(
                "UPDATE business_assessments SET opportunity_id = ?1 WHERE id = ?2",
                params![opportunity_id, a.id],
            )?;
        }
        Some(Some(existing)) if existing == opportunity_id => {}
        _ => {
            let mut copy = a.clone();
            copy.id = new_id("fit");
            insert_assessment(c, &copy, None, Some(opportunity_id))?;
        }
    }
    Ok(())
}

pub fn assessments_for(c: &Connection, opportunity_id: &str) -> AppResult<Vec<FitAssessment>> {
    let mut stmt = c.prepare(
        "SELECT content FROM business_assessments WHERE opportunity_id = ?1
         ORDER BY created_at DESC",
    )?;
    let rows: Vec<String> = stmt
        .query_map([opportunity_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    rows.iter().map(|t| from_json(t)).collect()
}

// ── Opportunities ────────────────────────────────────────────────────

/// A research result or listing the user saves (B16).
#[derive(Debug, Clone)]
pub struct NewOpportunity {
    pub kind: OpportunityKind,
    pub name: String,
    pub company_key: Option<String>,
    pub company_name: Option<String>,
    pub offer: Option<OfferRef>,
    pub canonical_job_id: Option<String>,
    pub source_url: Option<String>,
    pub use_case: String,
    pub contacts: Vec<ContactRef>,
    pub evidence: Vec<SourceNote>,
    pub amount: Option<Amount>,
    pub contract: Option<ContractTerms>,
    pub listing_status: Option<String>,
    /// The opportunity's identity (company + offer + use case, or the
    /// canonical posting): saving it again returns the same record.
    pub idempotency_key: String,
    pub researched_at: Option<i64>,
}

/// Saves an opportunity; for a known identity, the existing record gets
/// the new evidence and contacts (never ones the user removed) and its
/// research time — the user's stage, notes and flags stay as they are.
/// Returns the id and whether it was created.
pub fn save_opportunity(
    c: &Connection,
    new: &NewOpportunity,
    now: i64,
) -> AppResult<(String, bool)> {
    let found: Option<(String, String, String, String)> = c
        .query_row(
            "SELECT id, evidence, contacts, removed_contacts FROM business_opportunities
             WHERE idempotency_key = ?1",
            [&new.idempotency_key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    if let Some((id, evidence, contacts, removed)) = found {
        let mut evidence: Vec<SourceNote> = from_json(&evidence)?;
        for note in &new.evidence {
            if !evidence
                .iter()
                .any(|e| e.url == note.url && e.label == note.label)
            {
                evidence.push(note.clone());
            }
        }
        let mut contacts: Vec<ContactRef> = from_json(&contacts)?;
        let removed: Vec<String> = from_json(&removed)?;
        for contact in &new.contacts {
            if !removed.contains(&contact.id) && !contacts.iter().any(|x| x.id == contact.id) {
                contacts.push(contact.clone());
            }
        }
        c.execute(
            "UPDATE business_opportunities SET evidence = ?1, contacts = ?2,
                 last_researched_at = MAX(COALESCE(last_researched_at, 0), ?3)
             WHERE id = ?4",
            params![
                to_json(&evidence)?,
                to_json(&contacts)?,
                new.researched_at.unwrap_or(now),
                id
            ],
        )?;
        return Ok((id, false));
    }
    let id = new_id("opp");
    c.execute(
        "INSERT INTO business_opportunities (id, kind, name, company_key, company_name, offer_id,
             offer_version, canonical_job_id, source_url, use_case, stage, archived,
             do_not_contact, contacts, removed_contacts, evidence, amount, contract,
             listing_status, next_step, notes, idempotency_key, created_at, updated_at,
             last_researched_at, last_commercial_activity_at, revision)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'new_lead', 0, ?11, ?12, '[]', ?13,
             ?14, ?15, ?16, '', '', ?17, ?18, ?18, ?19, NULL, 0)",
        params![
            id,
            new.kind.as_str(),
            new.name,
            new.company_key,
            new.company_name,
            new.offer.as_ref().map(|o| o.offer_id.clone()),
            new.offer.as_ref().map(|o| o.version),
            new.canonical_job_id,
            new.source_url,
            new.use_case,
            new.company_key
                .as_deref()
                .map(|k| suppressed(c, "company", k))
                .transpose()?
                .unwrap_or(false),
            to_json(&new.contacts)?,
            to_json(&new.evidence)?,
            new.amount.as_ref().map(to_json).transpose()?,
            new.contract.as_ref().map(to_json).transpose()?,
            new.listing_status,
            new.idempotency_key,
            now,
            new.researched_at
        ],
    )?;
    Ok((id, true))
}

const OPPORTUNITY_COLUMNS: &str = "id, kind, name, company_key, company_name, offer_id,
    offer_version, canonical_job_id, source_url, use_case, stage, archived, do_not_contact,
    contacts, evidence, amount, contract, listing_status, next_step, notes, created_at,
    updated_at, last_researched_at, last_commercial_activity_at, revision";

struct OpportunityRow {
    id: String,
    kind: String,
    name: String,
    company_key: Option<String>,
    company_name: Option<String>,
    offer_id: Option<String>,
    offer_version: Option<u32>,
    canonical_job_id: Option<String>,
    source_url: Option<String>,
    use_case: String,
    stage: String,
    archived: bool,
    do_not_contact: bool,
    contacts: String,
    evidence: String,
    amount: Option<String>,
    contract: Option<String>,
    listing_status: Option<String>,
    next_step: String,
    notes: String,
    created_at: i64,
    updated_at: i64,
    last_researched_at: Option<i64>,
    last_commercial_activity_at: Option<i64>,
    revision: u32,
}

fn opportunity_row(r: &Row) -> rusqlite::Result<OpportunityRow> {
    Ok(OpportunityRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        company_key: r.get(3)?,
        company_name: r.get(4)?,
        offer_id: r.get(5)?,
        offer_version: r.get(6)?,
        canonical_job_id: r.get(7)?,
        source_url: r.get(8)?,
        use_case: r.get(9)?,
        stage: r.get(10)?,
        archived: r.get(11)?,
        do_not_contact: r.get(12)?,
        contacts: r.get(13)?,
        evidence: r.get(14)?,
        amount: r.get(15)?,
        contract: r.get(16)?,
        listing_status: r.get(17)?,
        next_step: r.get(18)?,
        notes: r.get(19)?,
        created_at: r.get(20)?,
        updated_at: r.get(21)?,
        last_researched_at: r.get(22)?,
        last_commercial_activity_at: r.get(23)?,
        revision: r.get(24)?,
    })
}

fn opportunity_from(c: &Connection, row: OpportunityRow) -> AppResult<Opportunity> {
    let offer = match (&row.offer_id, row.offer_version) {
        (Some(id), Some(v)) => Some(offer_ref(c, id, v)?),
        _ => None,
    };
    let assessments = assessments_for(c, &row.id)?;
    Ok(Opportunity {
        kind: OpportunityKind::parse(&row.kind),
        name: row.name,
        company_key: row.company_key,
        company_name: row.company_name,
        offer,
        canonical_job_id: row.canonical_job_id,
        source_url: row.source_url,
        use_case: row.use_case,
        stage: PipelineStage::parse(&row.stage).unwrap_or(PipelineStage::NewLead),
        archived: row.archived,
        do_not_contact: row.do_not_contact,
        contacts: from_json(&row.contacts)?,
        evidence: from_json(&row.evidence)?,
        amount: row.amount.as_deref().map(from_json).transpose()?,
        contract: row.contract.as_deref().map(from_json).transpose()?,
        listing_status: row.listing_status,
        next_step: row.next_step,
        notes: row.notes,
        created_at: row.created_at,
        updated_at: row.updated_at,
        last_researched_at: row.last_researched_at,
        last_commercial_activity_at: row.last_commercial_activity_at,
        revision: row.revision,
        assessment: assessments.first().cloned(),
        assessments: assessments.len() as u32,
        activities: activities_for(c, &row.id)?,
        id: row.id,
    })
}

pub fn opportunity(c: &Connection, id: &str) -> AppResult<Option<Opportunity>> {
    let row = c
        .query_row(
            &format!("SELECT {OPPORTUNITY_COLUMNS} FROM business_opportunities WHERE id = ?1"),
            [id],
            opportunity_row,
        )
        .optional()?;
    row.map(|r| opportunity_from(c, r)).transpose()
}

pub fn opportunity_by_key(c: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(c.query_row(
        "SELECT id FROM business_opportunities WHERE idempotency_key = ?1",
        [key],
        |r| r.get(0),
    )
    .optional()?)
}

pub fn opportunities(c: &Connection) -> AppResult<Vec<Opportunity>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {OPPORTUNITY_COLUMNS} FROM business_opportunities ORDER BY updated_at DESC"
    ))?;
    let rows: Vec<OpportunityRow> = stmt
        .query_map([], opportunity_row)?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter().map(|r| opportunity_from(c, r)).collect()
}

/// The fields a user edits (B24): everything else is research-owned or
/// changes through its own action (stage, activity, do-not-contact).
#[derive(Debug, Clone, PartialEq, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpportunityEdit {
    pub name: String,
    pub use_case: String,
    pub next_step: String,
    pub notes: String,
    pub amount: Option<Amount>,
    pub archived: bool,
}

pub fn edit_opportunity(
    c: &Connection,
    id: &str,
    edit: &OpportunityEdit,
    expected_revision: u32,
    now: i64,
) -> AppResult<()> {
    if edit.name.trim().is_empty() {
        return Err(AppError::validation("Give the opportunity a name."));
    }
    let changed = c.execute(
        "UPDATE business_opportunities SET name = ?1, use_case = ?2, next_step = ?3, notes = ?4,
             amount = ?5, archived = ?6, updated_at = ?7, revision = revision + 1
         WHERE id = ?8 AND revision = ?9",
        params![
            edit.name.trim(),
            edit.use_case.trim(),
            edit.next_step.trim(),
            edit.notes,
            edit.amount.as_ref().map(to_json).transpose()?,
            edit.archived,
            now,
            id,
            expected_revision
        ],
    )?;
    if changed == 0 {
        return Err(stale(c, "business_opportunities", id, "opportunity"));
    }
    Ok(())
}

/// Sets the stage (the caller records the activity in the same
/// transaction).
pub fn set_stage(
    c: &Connection,
    id: &str,
    stage: PipelineStage,
    expected_revision: u32,
    now: i64,
) -> AppResult<()> {
    let changed = c.execute(
        "UPDATE business_opportunities SET stage = ?1, updated_at = ?2, revision = revision + 1
         WHERE id = ?3 AND revision = ?4",
        params![stage.as_str(), now, id, expected_revision],
    )?;
    if changed == 0 {
        return Err(stale(c, "business_opportunities", id, "opportunity"));
    }
    Ok(())
}

/// Removes a contact from an opportunity and remembers that it was
/// removed (later research does not add it back).
pub fn remove_contact(
    c: &Connection,
    id: &str,
    contact_id: &str,
    expected_revision: u32,
    now: i64,
) -> AppResult<Option<ContactRef>> {
    let (contacts, removed): (String, String) = c
        .query_row(
            "SELECT contacts, removed_contacts FROM business_opportunities WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("The opportunity no longer exists."))?;
    let mut contacts: Vec<ContactRef> = from_json(&contacts)?;
    let mut removed: Vec<String> = from_json(&removed)?;
    let gone = contacts
        .iter()
        .position(|x| x.id == contact_id)
        .map(|i| contacts.remove(i));
    if !removed.iter().any(|r| r == contact_id) {
        removed.push(contact_id.to_string());
    }
    let changed = c.execute(
        "UPDATE business_opportunities SET contacts = ?1, removed_contacts = ?2, updated_at = ?3,
             revision = revision + 1
         WHERE id = ?4 AND revision = ?5",
        params![
            to_json(&contacts)?,
            to_json(&removed)?,
            now,
            id,
            expected_revision
        ],
    )?;
    if changed == 0 {
        return Err(stale(c, "business_opportunities", id, "opportunity"));
    }
    Ok(gone)
}

/// A listing's availability as last checked (research-owned: the user's
/// notes and fields are not touched).
pub fn set_listing_status(c: &Connection, id: &str, status: &str, now: i64) -> AppResult<()> {
    c.execute(
        "UPDATE business_opportunities SET listing_status = ?1, last_researched_at = ?2
         WHERE id = ?3",
        params![status, now, id],
    )?;
    Ok(())
}

pub fn set_do_not_contact(c: &Connection, id: &str, now: i64) -> AppResult<()> {
    c.execute(
        "UPDATE business_opportunities SET do_not_contact = 1, updated_at = ?1,
             revision = revision + 1
         WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn delete_opportunity(c: &Connection, id: &str) -> AppResult<()> {
    c.execute("DELETE FROM business_opportunities WHERE id = ?1", [id])?;
    Ok(())
}

// ── Activities ───────────────────────────────────────────────────────

/// Records user-reported activity once per idempotency key. Returns the
/// id and whether it was new.
pub fn insert_activity(
    c: &Connection,
    a: &Activity,
    idempotency_key: &str,
) -> AppResult<(String, bool)> {
    let found: Option<String> = c
        .query_row(
            "SELECT id FROM business_activities WHERE idempotency_key = ?1",
            [idempotency_key],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = found {
        return Ok((id, false));
    }
    c.execute(
        "INSERT INTO business_activities (id, opportunity_id, kind, person, occurred_at,
             recorded_at, source, detail, from_stage, to_stage, experiment_id, variant,
             idempotency_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'user_reported', ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            a.id,
            a.opportunity_id,
            a.kind.as_str(),
            a.person,
            a.occurred_at,
            a.recorded_at,
            a.detail,
            a.from_stage.map(PipelineStage::as_str),
            a.to_stage.map(PipelineStage::as_str),
            a.experiment_id,
            a.variant,
            idempotency_key
        ],
    )?;
    let commercial = !matches!(a.kind, ActivityType::StageChange | ActivityType::Note);
    if commercial {
        c.execute(
            "UPDATE business_opportunities
             SET last_commercial_activity_at = MAX(COALESCE(last_commercial_activity_at, 0), ?1)
             WHERE id = ?2",
            params![a.occurred_at, a.opportunity_id],
        )?;
    }
    Ok((a.id.clone(), true))
}

fn activity_row(r: &Row) -> rusqlite::Result<Activity> {
    let kind: String = r.get(2)?;
    let from: Option<String> = r.get(8)?;
    let to: Option<String> = r.get(9)?;
    Ok(Activity {
        id: r.get(0)?,
        opportunity_id: r.get(1)?,
        kind: ActivityType::parse(&kind),
        person: r.get(3)?,
        occurred_at: r.get(4)?,
        recorded_at: r.get(5)?,
        source: r.get(6)?,
        detail: r.get(7)?,
        from_stage: from.as_deref().and_then(PipelineStage::parse),
        to_stage: to.as_deref().and_then(PipelineStage::parse),
        experiment_id: r.get(10)?,
        variant: r.get(11)?,
    })
}

const ACTIVITY_COLUMNS: &str = "id, opportunity_id, kind, person, occurred_at, recorded_at, source,
    detail, from_stage, to_stage, experiment_id, variant";

pub fn activities_for(c: &Connection, opportunity_id: &str) -> AppResult<Vec<Activity>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {ACTIVITY_COLUMNS} FROM business_activities WHERE opportunity_id = ?1
         ORDER BY occurred_at, recorded_at"
    ))?;
    let rows = stmt
        .query_map([opportunity_id], activity_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn all_activities(c: &Connection) -> AppResult<Vec<Activity>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {ACTIVITY_COLUMNS} FROM business_activities ORDER BY occurred_at, recorded_at"
    ))?;
    let rows = stmt
        .query_map([], activity_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn delete_activity(c: &Connection, id: &str) -> AppResult<()> {
    c.execute("DELETE FROM business_activities WHERE id = ?1", [id])?;
    Ok(())
}

// ── Suppressions ─────────────────────────────────────────────────────

pub fn suppress(
    c: &Connection,
    scope: &str,
    key: &str,
    label: &str,
    reason: Option<&str>,
    now: i64,
) -> AppResult<Suppression> {
    if !matches!(scope, "company" | "person") || key.trim().is_empty() {
        return Err(AppError::validation("Choose a company or a person."));
    }
    c.execute(
        "INSERT OR IGNORE INTO business_suppressions (id, scope, key, label, reason, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![new_id("dnc"), scope, key, label, reason, now],
    )?;
    if scope == "company" {
        c.execute(
            "UPDATE business_opportunities SET do_not_contact = 1, updated_at = ?1,
                 revision = revision + 1
             WHERE company_key = ?2 AND do_not_contact = 0",
            params![now, key],
        )?;
    }
    c.query_row(
        "SELECT id, scope, key, label, reason, created_at FROM business_suppressions
         WHERE scope = ?1 AND key = ?2",
        params![scope, key],
        suppression_row,
    )
    .map_err(AppError::from)
}

fn suppression_row(r: &Row) -> rusqlite::Result<Suppression> {
    Ok(Suppression {
        id: r.get(0)?,
        scope: r.get(1)?,
        key: r.get(2)?,
        label: r.get(3)?,
        reason: r.get(4)?,
        created_at: r.get(5)?,
    })
}

pub fn suppressions(c: &Connection) -> AppResult<Vec<Suppression>> {
    let mut stmt = c.prepare(
        "SELECT id, scope, key, label, reason, created_at FROM business_suppressions
         ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map([], suppression_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn suppressed(c: &Connection, scope: &str, key: &str) -> AppResult<bool> {
    Ok(c.query_row(
        "SELECT 1 FROM business_suppressions WHERE scope = ?1 AND key = ?2",
        params![scope, key],
        |_| Ok(()),
    )
    .optional()?
    .is_some())
}

/// Lifts a suppression (the user's own explicit action; research and
/// drafting never call this).
pub fn remove_suppression(c: &Connection, id: &str) -> AppResult<Option<Suppression>> {
    let found = c
        .query_row(
            "SELECT id, scope, key, label, reason, created_at FROM business_suppressions
             WHERE id = ?1",
            [id],
            suppression_row,
        )
        .optional()?;
    c.execute("DELETE FROM business_suppressions WHERE id = ?1", [id])?;
    Ok(found)
}

// ── Drafts ───────────────────────────────────────────────────────────

pub fn insert_draft(
    c: &Connection,
    d: &Draft,
    idempotency_key: Option<&str>,
) -> AppResult<(String, bool)> {
    if let Some(key) = idempotency_key {
        let found: Option<String> = c
            .query_row(
                "SELECT id FROM business_drafts WHERE idempotency_key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = found {
            return Ok((id, false));
        }
    }
    c.execute(
        "INSERT INTO business_drafts (id, opportunity_id, plan_id, experiment_id, variant,
             offer_id, offer_version, recipient, channel, subject, body, evidence,
             idempotency_key, created_at, updated_at, revision)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14, 0)",
        params![
            d.id,
            d.opportunity_id,
            d.plan_id,
            d.experiment_id,
            d.variant,
            d.offer.as_ref().map(|o| o.offer_id.clone()),
            d.offer.as_ref().map(|o| o.version),
            d.recipient,
            d.channel,
            d.subject,
            d.body,
            to_json(&d.evidence)?,
            idempotency_key,
            d.created_at
        ],
    )?;
    Ok((d.id.clone(), true))
}

const DRAFT_COLUMNS: &str = "id, opportunity_id, plan_id, experiment_id, variant, offer_id,
    offer_version, recipient, channel, subject, body, evidence, created_at, updated_at, revision";

type DraftRow = (Draft, Option<String>, Option<u32>, String);

fn draft_row(r: &Row) -> rusqlite::Result<DraftRow> {
    Ok((
        Draft {
            id: r.get(0)?,
            opportunity_id: r.get(1)?,
            plan_id: r.get(2)?,
            experiment_id: r.get(3)?,
            variant: r.get(4)?,
            offer: None,
            recipient: r.get(7)?,
            channel: r.get(8)?,
            subject: r.get(9)?,
            body: r.get(10)?,
            evidence: Vec::new(),
            created_at: r.get(12)?,
            updated_at: r.get(13)?,
            revision: r.get(14)?,
        },
        r.get(5)?,
        r.get(6)?,
        r.get(11)?,
    ))
}

fn draft_from(c: &Connection, row: DraftRow) -> AppResult<Draft> {
    let (mut draft, offer_id, version, evidence) = row;
    draft.offer = match (offer_id, version) {
        (Some(id), Some(v)) => Some(offer_ref(c, &id, v)?),
        _ => None,
    };
    draft.evidence = from_json(&evidence)?;
    Ok(draft)
}

pub fn draft(c: &Connection, id: &str) -> AppResult<Option<Draft>> {
    c.query_row(
        &format!("SELECT {DRAFT_COLUMNS} FROM business_drafts WHERE id = ?1"),
        [id],
        draft_row,
    )
    .optional()?
    .map(|r| draft_from(c, r))
    .transpose()
}

pub fn drafts(c: &Connection) -> AppResult<Vec<Draft>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {DRAFT_COLUMNS} FROM business_drafts ORDER BY updated_at DESC"
    ))?;
    let rows: Vec<DraftRow> = stmt
        .query_map([], draft_row)?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter().map(|r| draft_from(c, r)).collect()
}

pub fn update_draft_text(
    c: &Connection,
    id: &str,
    subject: Option<&str>,
    body: &str,
    expected_revision: u32,
    now: i64,
) -> AppResult<()> {
    let changed = c.execute(
        "UPDATE business_drafts SET subject = ?1, body = ?2, updated_at = ?3,
             revision = revision + 1
         WHERE id = ?4 AND revision = ?5",
        params![subject, body, now, id, expected_revision],
    )?;
    if changed == 0 {
        return Err(stale(c, "business_drafts", id, "draft"));
    }
    Ok(())
}

pub fn delete_draft(c: &Connection, id: &str) -> AppResult<()> {
    c.execute("DELETE FROM business_drafts WHERE id = ?1", [id])?;
    Ok(())
}

// ── GTM plans ────────────────────────────────────────────────────────

pub fn insert_plan(
    c: &Connection,
    plan: &GtmPlan,
    idempotency_key: Option<&str>,
) -> AppResult<(String, bool)> {
    if let Some(key) = idempotency_key {
        let found: Option<String> = c
            .query_row(
                "SELECT id FROM gtm_plans WHERE idempotency_key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = found {
            return Ok((id, false));
        }
    }
    c.execute(
        "INSERT INTO gtm_plans (id, offer_id, offer_version, name, geography, content,
             idempotency_key, created_at, updated_at, revision)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 0)",
        params![
            plan.id,
            plan.offer.offer_id,
            plan.offer.version,
            plan.name,
            plan.geography,
            to_json(&plan.content)?,
            idempotency_key,
            plan.created_at
        ],
    )?;
    Ok((plan.id.clone(), true))
}

type PlanRow = (String, String, u32, String, String, String, i64, i64, u32);

fn plan_row(r: &Row) -> rusqlite::Result<PlanRow> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
    ))
}

fn plan_from(c: &Connection, row: PlanRow) -> AppResult<GtmPlan> {
    let (id, offer_id, version, name, geography, content, created, updated, revision) = row;
    Ok(GtmPlan {
        id,
        offer: offer_ref(c, &offer_id, version)?,
        name,
        geography,
        content: from_json::<PlanContent>(&content)?,
        created_at: created,
        updated_at: updated,
        revision,
    })
}

const PLAN_COLUMNS: &str =
    "id, offer_id, offer_version, name, geography, content, created_at, updated_at, revision";

pub fn plan(c: &Connection, id: &str) -> AppResult<Option<GtmPlan>> {
    c.query_row(
        &format!("SELECT {PLAN_COLUMNS} FROM gtm_plans WHERE id = ?1"),
        [id],
        plan_row,
    )
    .optional()?
    .map(|r| plan_from(c, r))
    .transpose()
}

pub fn plans(c: &Connection) -> AppResult<Vec<GtmPlan>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {PLAN_COLUMNS} FROM gtm_plans ORDER BY updated_at DESC"
    ))?;
    let rows: Vec<PlanRow> = stmt
        .query_map([], plan_row)?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter().map(|r| plan_from(c, r)).collect()
}

pub fn update_plan(
    c: &Connection,
    id: &str,
    name: &str,
    geography: &str,
    content: &PlanContent,
    expected_revision: u32,
    now: i64,
) -> AppResult<()> {
    let changed = c.execute(
        "UPDATE gtm_plans SET name = ?1, geography = ?2, content = ?3, updated_at = ?4,
             revision = revision + 1
         WHERE id = ?5 AND revision = ?6",
        params![
            name,
            geography,
            to_json(content)?,
            now,
            id,
            expected_revision
        ],
    )?;
    if changed == 0 {
        return Err(stale(c, "gtm_plans", id, "plan"));
    }
    Ok(())
}

pub fn delete_plan(c: &Connection, id: &str) -> AppResult<()> {
    c.execute("DELETE FROM gtm_plans WHERE id = ?1", [id])?;
    Ok(())
}

// ── Experiments ──────────────────────────────────────────────────────

pub fn insert_experiment(
    c: &Connection,
    e: &Experiment,
    idempotency_key: Option<&str>,
) -> AppResult<(String, bool)> {
    if let Some(key) = idempotency_key {
        let found: Option<String> = c
            .query_row(
                "SELECT id FROM gtm_experiments WHERE idempotency_key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = found {
            return Ok((id, false));
        }
    }
    c.execute(
        "INSERT INTO gtm_experiments (id, plan_id, offer_id, offer_version, segment_id, version,
             content, status, frozen_at, idempotency_key, created_at, updated_at, revision)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, 0)",
        params![
            e.id,
            e.plan_id,
            e.offer.offer_id,
            e.offer.version,
            e.segment_id,
            e.version,
            to_json(&e.content)?,
            e.status.as_str(),
            e.frozen_at,
            idempotency_key,
            e.created_at
        ],
    )?;
    Ok((e.id.clone(), true))
}

type ExperimentRow = (
    String,
    String,
    String,
    u32,
    Option<String>,
    u32,
    String,
    String,
    Option<i64>,
    i64,
    i64,
    u32,
);

fn experiment_row(r: &Row) -> rusqlite::Result<ExperimentRow> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        r.get(11)?,
    ))
}

fn experiment_from(c: &Connection, row: ExperimentRow) -> AppResult<Experiment> {
    let (
        id,
        plan_id,
        offer_id,
        version,
        segment,
        ev,
        content,
        status,
        frozen,
        created,
        updated,
        rev,
    ) = row;
    Ok(Experiment {
        id,
        plan_id,
        offer: offer_ref(c, &offer_id, version)?,
        segment_id: segment,
        version: ev,
        status: ExperimentStatus::parse(&status),
        content: from_json::<ExperimentContent>(&content)?,
        frozen_at: frozen,
        created_at: created,
        updated_at: updated,
        revision: rev,
    })
}

const EXPERIMENT_COLUMNS: &str = "id, plan_id, offer_id, offer_version, segment_id, version,
    content, status, frozen_at, created_at, updated_at, revision";

pub fn experiment(c: &Connection, id: &str) -> AppResult<Option<Experiment>> {
    c.query_row(
        &format!("SELECT {EXPERIMENT_COLUMNS} FROM gtm_experiments WHERE id = ?1"),
        [id],
        experiment_row,
    )
    .optional()?
    .map(|r| experiment_from(c, r))
    .transpose()
}

pub fn experiments(c: &Connection) -> AppResult<Vec<Experiment>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {EXPERIMENT_COLUMNS} FROM gtm_experiments ORDER BY updated_at DESC"
    ))?;
    let rows: Vec<ExperimentRow> = stmt
        .query_map([], experiment_row)?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter().map(|r| experiment_from(c, r)).collect()
}

pub fn update_experiment(
    c: &Connection,
    e: &Experiment,
    expected_revision: u32,
    now: i64,
) -> AppResult<()> {
    let changed = c.execute(
        "UPDATE gtm_experiments SET content = ?1, status = ?2, frozen_at = ?3, segment_id = ?4,
             updated_at = ?5, revision = revision + 1
         WHERE id = ?6 AND revision = ?7",
        params![
            to_json(&e.content)?,
            e.status.as_str(),
            e.frozen_at,
            e.segment_id,
            now,
            e.id,
            expected_revision
        ],
    )?;
    if changed == 0 {
        return Err(stale(c, "gtm_experiments", &e.id, "experiment"));
    }
    Ok(())
}

pub fn delete_experiment(c: &Connection, id: &str) -> AppResult<()> {
    c.execute("DELETE FROM gtm_experiments WHERE id = ?1", [id])?;
    Ok(())
}

// ── Redactions ───────────────────────────────────────────────────────

pub fn record_redaction(
    c: &Connection,
    opportunity_id: Option<&str>,
    kind: &str,
    detail: &str,
    now: i64,
) -> AppResult<()> {
    c.execute(
        "INSERT INTO business_redactions (id, opportunity_id, kind, detail, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![new_id("red"), opportunity_id, kind, detail, now],
    )?;
    Ok(())
}

pub fn redactions(c: &Connection) -> AppResult<Vec<Redaction>> {
    let mut stmt = c.prepare(
        "SELECT id, opportunity_id, kind, detail, created_at FROM business_redactions
         ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Redaction {
                id: r.get(0)?,
                opportunity_id: r.get(1)?,
                kind: r.get(2)?,
                detail: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Replaces every occurrence of a removed person's name in stored run
/// results, assessments, drafts, plans, experiments, opportunity notes and
/// activity history (B29: history keeps its metadata, not the removed
/// personal data). Returns how many records changed.
pub fn redact_text(c: &Connection, needle: &str, now: i64) -> AppResult<usize> {
    let needle = needle.trim();
    if needle.chars().count() < 3 {
        return Ok(0);
    }
    let pattern = format!("%{needle}%");
    let mut changed = 0;
    for (table, column) in [
        ("business_research_runs", "result"),
        ("business_assessments", "content"),
        ("business_drafts", "body"),
        ("business_drafts", "recipient"),
        ("business_activities", "person"),
        ("business_activities", "detail"),
        ("gtm_plans", "content"),
        ("gtm_experiments", "content"),
    ] {
        changed += c.execute(
            &format!(
                "UPDATE {table} SET {column} = REPLACE({column}, ?1, '[removed]')
                 WHERE {column} LIKE ?2"
            ),
            params![needle, pattern],
        )?;
    }
    // User-editable fields: bump the revision so an edit form opened
    // before the redaction cannot write the name back.
    changed += c.execute(
        "UPDATE business_opportunities
         SET notes = REPLACE(notes, ?1, '[removed]'),
             next_step = REPLACE(next_step, ?1, '[removed]'),
             evidence = REPLACE(evidence, ?1, '[removed]'),
             updated_at = ?3,
             revision = revision + 1
         WHERE notes LIKE ?2 OR next_step LIKE ?2 OR evidence LIKE ?2",
        params![needle, pattern, now],
    )?;
    Ok(changed)
}
