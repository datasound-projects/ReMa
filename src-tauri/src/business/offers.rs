//! Offers (B3–B5): a manual description or an ingested website becomes a
//! draft; the user reviews it into an immutable version; research and
//! GTM use a selected reviewed version, never the draft. A website refresh
//! proposes changes to the draft — it never overwrites a claim the user
//! confirmed or rejected.

use super::{
    model::{Claim, FieldStatus, Offer, OfferContent, OfferDiff, OfferKind, OfferRef, PriceUnit},
    store,
    text::norm,
};
use crate::{
    analytics::normalize,
    error::{AppError, AppResult},
    state::AppState,
    time::now_ms,
};

fn tidy_claim(claim: &mut Claim) {
    claim.text = normalize::clip(claim.text.trim(), 300);
    if claim.text.is_empty() {
        claim.status = FieldStatus::Unknown;
    }
    if let Some(url) = &claim.source_url {
        claim.source_url = normalize::web_url(url);
    }
    // A claim the user typed has no source page: it is theirs.
    if claim.status == FieldStatus::Observed
        && claim.source_url.is_none()
        && claim.excerpt.is_none()
    {
        claim.status = FieldStatus::UserConfirmed;
    }
}

fn tidy_list(list: &mut Vec<Claim>) {
    for claim in list.iter_mut() {
        tidy_claim(claim);
    }
    let mut seen: Vec<String> = Vec::new();
    list.retain(|c| {
        let key = norm(&c.text);
        if key.is_empty() || seen.contains(&key) {
            return false;
        }
        seen.push(key);
        true
    });
    list.truncate(40);
}

/// Normalizes a draft the user saves.
pub fn validate(content: &mut OfferContent) -> AppResult<()> {
    content.name = normalize::clip(content.name.trim(), 120);
    if content.name.is_empty() {
        return Err(AppError::validation("Give the offer a name."));
    }
    for claim in [
        &mut content.summary,
        &mut content.problem,
        &mut content.delivery_model,
    ] {
        tidy_claim(claim);
    }
    for list in [
        &mut content.outcomes,
        &mut content.features,
        &mut content.use_cases,
        &mut content.customer_types,
        &mut content.buyer_roles,
        &mut content.geography,
        &mut content.languages,
        &mut content.requirements,
        &mut content.integrations,
        &mut content.deployment_constraints,
        &mut content.exclusions,
        &mut content.unsupported_claims,
        &mut content.limitations,
    ] {
        tidy_list(list);
    }
    for price in content.pricing.iter_mut() {
        price.amount = normalize::clip(price.amount.trim(), 40);
        price.currency = price
            .currency
            .as_ref()
            .map(|c| c.trim().to_uppercase())
            .filter(|c| c.len() == 3 && c.chars().all(|x| x.is_ascii_alphabetic()));
        tidy_claim(&mut price.claim);
    }
    content
        .pricing
        .retain(|p| !p.amount.is_empty() || p.unit == PriceUnit::CustomQuote);
    content.website_urls = content
        .website_urls
        .iter()
        .filter_map(|u| normalize::web_url(u))
        .take(5)
        .collect();
    Ok(())
}

/// Why a draft cannot be reviewed yet (B3: name, what it does, the
/// problem; conflicts must be resolved).
pub fn review_blockers(content: &OfferContent) -> Vec<String> {
    let mut out = Vec::new();
    if content.name.trim().is_empty() {
        out.push("Give the offer a name.".to_string());
    }
    if !content.summary.is_known() {
        out.push("Describe what it does.".to_string());
    }
    if !content.problem.is_known() {
        out.push("Describe the problem it addresses.".to_string());
    }
    let conflicting = content
        .claims()
        .into_iter()
        .filter(|(_, c)| c.status == FieldStatus::Conflicting)
        .count();
    if conflicting > 0 {
        out.push(format!(
            "Resolve {conflicting} conflicting claim{} (keep, correct or reject).",
            if conflicting == 1 { "" } else { "s" }
        ));
    }
    out
}

pub fn create(
    state: &AppState,
    mut content: OfferContent,
    idempotency_key: Option<&str>,
) -> AppResult<Offer> {
    validate(&mut content)?;
    let now = now_ms();
    let id = state
        .db
        .call(|c| store::insert_offer(c, &content, idempotency_key, now))?;
    state.events.business_changed();
    get(state, &id)
}

pub fn get(state: &AppState, id: &str) -> AppResult<Offer> {
    state
        .db
        .call(|c| store::offer(c, id))?
        .ok_or_else(|| AppError::not_found("The offer no longer exists."))
}

pub fn save_draft(
    state: &AppState,
    id: &str,
    mut content: OfferContent,
    expected_revision: u32,
) -> AppResult<Offer> {
    validate(&mut content)?;
    state
        .db
        .call(|c| store::save_offer_draft(c, id, &content, expected_revision, now_ms()))?;
    state.events.business_changed();
    get(state, id)
}

/// Saves the draft as the next reviewed version (the user's explicit
/// action: never called by research or a model).
pub fn review(state: &AppState, id: &str, expected_revision: u32) -> AppResult<Offer> {
    let offer = get(state, id)?;
    let mut content = offer
        .draft
        .clone()
        .ok_or_else(|| AppError::validation("There is no draft to review."))?;
    validate(&mut content)?;
    let blockers = review_blockers(&content);
    if !blockers.is_empty() {
        return Err(AppError::validation(blockers.join(" ")));
    }
    state.db.call(|c| {
        store::add_offer_version(c, id, &content, expected_revision, now_ms()).map(|_| ())
    })?;
    state.events.business_changed();
    get(state, id)
}

/// The reviewed version research uses: the one asked for, or the newest.
pub fn reviewed(
    state: &AppState,
    id: &str,
    version: Option<u32>,
) -> AppResult<(OfferRef, OfferContent)> {
    let offer = get(state, id)?;
    let version = version.or(offer.current_version).ok_or_else(|| {
        AppError::validation(format!(
            "\"{}\" has not been reviewed yet. Review the offer before using it for research.",
            offer.name
        ))
    })?;
    let (content, _) = state
        .db
        .call(|c| store::offer_version(c, id, version))?
        .ok_or_else(|| AppError::not_found("That offer version does not exist."))?;
    Ok((
        OfferRef {
            offer_id: offer.id,
            version,
            name: content.name.clone(),
        },
        content,
    ))
}

/// Where a lowercased text names an offer, as whole words ("support
/// workspace" in "… for my support workspace v2", not in "support
/// workspaces"): the end of the first mention. Names under three
/// characters never count.
fn mention_end(lower: &str, name: &str) -> Option<usize> {
    let needle = name.trim().to_lowercase();
    if needle.chars().count() < 3 {
        return None;
    }
    let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
    lower.match_indices(&needle).find_map(|(i, _)| {
        let end = i + needle.len();
        (boundary(lower[..i].chars().next_back()) && boundary(lower[end..].chars().next()))
            .then_some(end)
    })
}

/// Whether a text names an offer (whole words, ignoring case).
pub fn mentions(text: &str, name: &str) -> bool {
    mention_end(&text.to_lowercase(), name).is_some()
}

/// The offer a lowercased text names: the longest name wins ("Support
/// Workspace Pro" over "Support Workspace"), then a usable offer over an
/// archived or unreviewed one with the same name, then the first listed.
fn named<'a>(offers: &'a [Offer], lower: &str) -> Option<&'a Offer> {
    let key = |o: &Offer| {
        (
            o.name.trim().len(),
            !o.archived && o.current_version.is_some(),
        )
    };
    offers
        .iter()
        .filter(|o| mention_end(lower, &o.name).is_some())
        .fold(None, |best: Option<&Offer>, o| match best {
            Some(b) if key(b) >= key(o) => Some(b),
            _ => Some(o),
        })
}

/// Which offer "my product" means (B5): the one the text names, the only
/// reviewed one, or a question. A named offer that is archived or not
/// reviewed is never swapped for another one.
pub fn pick<'a>(offers: &'a [Offer], text: &str) -> Result<&'a Offer, String> {
    match named(offers, &text.to_lowercase()) {
        Some(o) if o.archived => {
            return Err(format!(
                "The offer \"{}\" is archived. Unarchive it in Business → Business Profile to \
                 use it.",
                o.name
            ))
        }
        Some(o) if o.current_version.is_none() => {
            return Err(format!(
                "The offer \"{}\" has not been reviewed yet. Review it in Business → Business \
                 Profile first.",
                o.name
            ))
        }
        Some(o) => return Ok(o),
        None => {}
    }
    let usable: Vec<&Offer> = offers
        .iter()
        .filter(|o| !o.archived && o.current_version.is_some())
        .collect();
    match usable.len() {
        0 => Err(
            "You have no reviewed offer yet. Describe your product or service in Business → \
             Business Profile and review it first."
                .into(),
        ),
        1 => Ok(usable[0]),
        _ => Err(format!(
            "Which offer do you mean: {}?",
            usable
                .iter()
                .map(|o| format!("\"{}\"", o.name))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The offer (and version) a scheduled task's prompt names (B26): a named
/// offer that is archived or not reviewed stops the task instead of
/// silently using another one; "v2" after the name pins that version.
pub fn for_task(offers: &[Offer], prompt: &str) -> Result<(String, Option<u32>), String> {
    let lower = prompt.to_lowercase();
    let offer = match named(offers, &lower) {
        Some(o) if o.archived => {
            return Err(format!(
                "The offer \"{}\" is archived, so this task does not run. Unarchive it or \
                 change the task.",
                o.name
            ))
        }
        Some(o) if o.current_version.is_none() => {
            return Err(format!(
                "The offer \"{}\" has not been reviewed yet; review it before this task can \
                 run.",
                o.name
            ))
        }
        Some(o) => o,
        None => pick(offers, prompt)?,
    };
    let after = mention_end(&lower, &offer.name)
        .map(|end| &lower[end..])
        .unwrap_or("");
    let version = after
        .trim_start()
        .strip_prefix('v')
        .map(|rest| {
            rest.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .and_then(|digits| digits.parse::<u32>().ok());
    if let Some(v) = version {
        if offer.current_version.is_none_or(|current| v > current) {
            return Err(format!("\"{}\" has no reviewed version {v}.", offer.name));
        }
    }
    Ok((offer.id.clone(), version))
}

fn status_label(status: FieldStatus) -> &'static str {
    match status {
        FieldStatus::UserConfirmed => "confirmed by the user",
        FieldStatus::Observed => "stated on the website",
        FieldStatus::Hypothesis => "hypothesis, untested",
        FieldStatus::Unknown => "unknown",
        FieldStatus::Conflicting => "conflicting sources",
    }
}

/// The selected reviewed version as a model may see it (B5): this offer
/// only — no other offer, no CV, no private notes. Rejected claims are
/// listed as claims not to make.
pub fn context(offer: &OfferRef, content: &OfferContent) -> String {
    let kind = match content.kind {
        OfferKind::Service => "service",
        OfferKind::DigitalProduct => "digital product",
        OfferKind::Hybrid => "service and digital product",
    };
    let maturity = match content.maturity {
        super::model::Maturity::NotStated => "not stated",
        super::model::Maturity::Prototype => "prototype",
        super::model::Maturity::PilotReady => "pilot-ready",
        super::model::Maturity::GenerallyAvailable => "generally available",
    };
    let mut lines = vec![format!(
        "Offer: {} v{} ({kind}; maturity: {maturity})",
        content.name, offer.version
    )];
    let one = |label: &str, c: &Claim, lines: &mut Vec<String>| {
        if c.is_known() {
            lines.push(format!("{label}: {} [{}]", c.text, status_label(c.status)));
        }
    };
    one("What it does", &content.summary, &mut lines);
    one("Problem addressed", &content.problem, &mut lines);
    one("Delivery", &content.delivery_model, &mut lines);
    for (label, list) in [
        ("Outcomes", &content.outcomes),
        ("Features or deliverables", &content.features),
        ("Use cases", &content.use_cases),
        ("Intended customer types", &content.customer_types),
        ("Proposed buyer roles", &content.buyer_roles),
        ("Available in", &content.geography),
        ("Languages", &content.languages),
        ("Requirements", &content.requirements),
        ("Integrations", &content.integrations),
        ("Deployment", &content.deployment_constraints),
        ("Exclusions", &content.exclusions),
        ("Known limitations", &content.limitations),
    ] {
        let known: Vec<&Claim> = list.iter().filter(|c| c.is_known()).collect();
        if known.is_empty() {
            continue;
        }
        lines.push(format!("{label}:"));
        for c in known.iter().take(12) {
            let note = c
                .note
                .as_deref()
                .filter(|n| n.contains("marketing"))
                .map(|_| "; marketing claim")
                .unwrap_or("");
            lines.push(format!("- {} [{}{note}]", c.text, status_label(c.status)));
        }
    }
    if !content.pricing.is_empty() {
        lines.push("Pricing (as stated; units are not interchangeable):".into());
        for p in &content.pricing {
            lines.push(format!(
                "- {} {} {} [{}]",
                p.currency.as_deref().unwrap_or(""),
                p.amount,
                p.unit.label(),
                status_label(p.claim.status)
            ));
        }
    }
    if !content.unsupported_claims.is_empty() {
        lines.push("Never claim these (the user rejected them):".into());
        for c in content.unsupported_claims.iter().take(12) {
            lines.push(format!("- {}", c.text));
        }
    }
    lines.join("\n")
}

fn claim_lists(content: &mut OfferContent) -> Vec<(&'static str, &mut Vec<Claim>)> {
    vec![
        ("outcomes", &mut content.outcomes),
        ("features", &mut content.features),
        ("use cases", &mut content.use_cases),
        ("customer types", &mut content.customer_types),
        ("buyer roles", &mut content.buyer_roles),
        ("geography", &mut content.geography),
        ("languages", &mut content.languages),
        ("requirements", &mut content.requirements),
        ("integrations", &mut content.integrations),
        ("deployment", &mut content.deployment_constraints),
        ("exclusions", &mut content.exclusions),
        ("limitations", &mut content.limitations),
    ]
}

/// Merges a fresh extraction into the draft as a proposal (B5): new
/// claims are added as `observed`; what the user confirmed or rejected
/// stays; a website that now disagrees with a confirmed price is marked
/// conflicting; claims the pages no longer show are noted, not deleted.
pub fn merge_refresh(
    draft: &OfferContent,
    fresh: &OfferContent,
    now: i64,
) -> (OfferContent, OfferDiff) {
    let mut merged = draft.clone();
    let mut diff = OfferDiff::default();
    let rejected: Vec<String> = draft
        .unsupported_claims
        .iter()
        .map(|c| norm(&c.text))
        .collect();
    let mut fresh_copy = fresh.clone();
    let fresh_lists = claim_lists(&mut fresh_copy);
    let mut merged_lists = claim_lists(&mut merged);
    for ((label, target), (_, source)) in merged_lists.iter_mut().zip(fresh_lists) {
        let fresh_keys: Vec<String> = source.iter().map(|c| norm(&c.text)).collect();
        for claim in source.iter() {
            let key = norm(&claim.text);
            if rejected.contains(&key) {
                diff.still_rejected.push(format!("{label}: {}", claim.text));
                continue;
            }
            match target.iter_mut().find(|c| norm(&c.text) == key) {
                Some(existing) => {
                    if existing.status == FieldStatus::Observed {
                        existing.retrieved_at = claim.retrieved_at;
                        existing.note = claim.note.clone();
                    }
                }
                None => {
                    let mut new = claim.clone();
                    new.note = Some(match &claim.note {
                        Some(n) => format!("{n} New on the website at refresh."),
                        None => "New on the website at refresh.".to_string(),
                    });
                    diff.added.push(format!("{label}: {}", claim.text));
                    target.push(new);
                }
            }
        }
        for existing in target.iter_mut() {
            let from_site =
                existing.status == FieldStatus::Observed && existing.source_url.is_some();
            if from_site && !fresh_keys.contains(&norm(&existing.text)) {
                existing.note = Some(format!(
                    "Not found on the website at refresh ({}).",
                    normalize::date_of(now)
                ));
                diff.not_found.push(format!("{label}: {}", existing.text));
            }
        }
    }
    // Singular fields: fill what is unknown; never replace what the user
    // confirmed.
    for (label, target, source) in [
        ("what it does", &mut merged.summary, &fresh.summary),
        ("problem", &mut merged.problem, &fresh.problem),
        (
            "delivery",
            &mut merged.delivery_model,
            &fresh.delivery_model,
        ),
    ] {
        if !source.is_known() || norm(&source.text) == norm(&target.text) {
            continue;
        }
        match target.status {
            FieldStatus::Unknown | FieldStatus::Observed => {
                if target.is_known() {
                    diff.added
                        .push(format!("{label} (updated): {}", source.text));
                } else {
                    diff.added.push(format!("{label}: {}", source.text));
                }
                *target = source.clone();
            }
            _ => {}
        }
    }
    // Prices: a confirmed price the website now contradicts is a conflict.
    for price in &fresh.pricing {
        let same_unit: Vec<usize> = merged
            .pricing
            .iter()
            .enumerate()
            .filter(|(_, p)| p.unit == price.unit)
            .map(|(i, _)| i)
            .collect();
        if same_unit
            .iter()
            .any(|&i| norm(&merged.pricing[i].amount) == norm(&price.amount))
        {
            continue;
        }
        let confirmed = same_unit
            .iter()
            .find(|&&i| merged.pricing[i].claim.status == FieldStatus::UserConfirmed)
            .copied();
        let mut new = price.clone();
        match confirmed {
            Some(i) => {
                new.claim.status = FieldStatus::Conflicting;
                new.claim.note = Some(format!(
                    "The website now states {} {} {}; you confirmed {} {}.",
                    price.currency.as_deref().unwrap_or(""),
                    price.amount,
                    price.unit.label(),
                    merged.pricing[i].amount,
                    merged.pricing[i].unit.label()
                ));
                diff.conflicts.push(format!(
                    "pricing: website {} vs. confirmed {} ({})",
                    price.amount,
                    merged.pricing[i].amount,
                    price.unit.label()
                ));
            }
            None => diff
                .added
                .push(format!("pricing: {} {}", price.amount, price.unit.label())),
        }
        merged.pricing.push(new);
    }
    for url in &fresh.website_urls {
        if !merged.website_urls.contains(url) {
            merged.website_urls.push(url.clone());
        }
    }
    (merged, diff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::model::{Maturity, PricePoint};

    fn observed(text: &str) -> Claim {
        Claim {
            text: text.into(),
            status: FieldStatus::Observed,
            source_url: Some("https://acme.example/".into()),
            retrieved_at: Some(1),
            excerpt: Some(text.into()),
            note: None,
        }
    }

    fn offer() -> OfferContent {
        let mut c = OfferContent::empty("Support Workspace", OfferKind::DigitalProduct);
        c.summary = Claim::user("Answers support questions from a knowledge base.");
        c.problem = Claim::user("Support teams answer the same questions by hand.");
        c.maturity = Maturity::PilotReady;
        c
    }

    #[test]
    fn a_manual_offer_needs_only_a_name_what_it_does_and_the_problem() {
        let mut empty = OfferContent::empty("  ", OfferKind::Service);
        assert!(validate(&mut empty).is_err());
        let mut c = offer();
        validate(&mut c).unwrap();
        assert!(review_blockers(&c).is_empty());
        c.problem = Claim::unknown();
        assert_eq!(review_blockers(&c), ["Describe the problem it addresses."]);
        // A typed claim without a source is the user's own.
        let mut typed = offer();
        typed.features.push(Claim {
            status: FieldStatus::Observed,
            source_url: None,
            excerpt: None,
            ..Claim::user("Answer drafts")
        });
        validate(&mut typed).unwrap();
        assert_eq!(typed.features[0].status, FieldStatus::UserConfirmed);
    }

    #[test]
    fn a_refresh_proposes_and_never_overwrites_the_users_corrections() {
        let mut draft = offer();
        draft.features.push(observed("Knowledge base search"));
        draft.features.push(Claim::user("Answer drafts"));
        draft
            .unsupported_claims
            .push(observed("Salesforce integration"));
        draft.pricing.push(PricePoint {
            amount: "49".into(),
            currency: Some("EUR".into()),
            unit: PriceUnit::SeatMonth,
            claim: Claim::user("EUR 49 per seat per month"),
        });
        let mut fresh = OfferContent::empty("Support Workspace", OfferKind::DigitalProduct);
        fresh.summary = observed("An AI support desk.");
        fresh.features.push(observed("Analytics dashboard"));
        fresh.features.push(observed("Salesforce integration"));
        fresh.pricing.push(PricePoint {
            amount: "59".into(),
            currency: Some("EUR".into()),
            unit: PriceUnit::SeatMonth,
            claim: observed("EUR 59 per seat per month"),
        });
        let (merged, diff) = merge_refresh(&draft, &fresh, 1_790_000_000_000);
        // Confirmed summary stays.
        assert_eq!(merged.summary.status, FieldStatus::UserConfirmed);
        assert_eq!(diff.added, ["features: Analytics dashboard"]);
        assert_eq!(diff.still_rejected, ["features: Salesforce integration"]);
        assert!(!merged
            .features
            .iter()
            .any(|f| f.text.contains("Salesforce")));
        // The user's own claim is kept; the site claim no longer shown is
        // noted, not deleted.
        assert!(merged.features.iter().any(|f| f.text == "Answer drafts"));
        let old = merged
            .features
            .iter()
            .find(|f| f.text == "Knowledge base search")
            .unwrap();
        assert!(old.note.as_deref().unwrap().contains("Not found"));
        assert_eq!(diff.not_found, ["features: Knowledge base search"]);
        // The confirmed price stays; the new one is a visible conflict.
        assert_eq!(merged.pricing.len(), 2);
        assert_eq!(merged.pricing[0].claim.status, FieldStatus::UserConfirmed);
        assert_eq!(merged.pricing[1].claim.status, FieldStatus::Conflicting);
        assert_eq!(diff.conflicts.len(), 1);
        assert!(
            !review_blockers(&merged).is_empty(),
            "conflicts block review"
        );
    }

    #[test]
    fn context_carries_only_the_selected_offer_with_its_statuses() {
        let mut c = offer();
        c.use_cases.push(Claim {
            status: FieldStatus::Hypothesis,
            ..Claim::user("Internal IT helpdesk")
        });
        c.outcomes.push(Claim {
            note: Some("A marketing claim on the website, not a proven result.".into()),
            ..observed("Save 80% of your time")
        });
        c.unsupported_claims.push(observed("SOC 2 certified"));
        let text = context(
            &OfferRef {
                offer_id: "off_1".into(),
                version: 2,
                name: "Support Workspace".into(),
            },
            &c,
        );
        assert!(text.starts_with("Offer: Support Workspace v2"));
        assert!(text.contains("Internal IT helpdesk [hypothesis, untested]"));
        assert!(text.contains("[stated on the website; marketing claim]"));
        assert!(text.contains("Never claim these"));
        assert!(text.contains("- SOC 2 certified"));
    }

    #[test]
    fn my_product_is_asked_about_when_it_is_ambiguous() {
        let make = |id: &str, name: &str, reviewed: bool| Offer {
            id: id.into(),
            name: name.into(),
            kind: OfferKind::Service,
            current_version: reviewed.then_some(1),
            draft: None,
            reviewed: None,
            reviewed_at: None,
            archived: false,
            created_at: 0,
            updated_at: 0,
            revision: 0,
        };
        let offers = vec![
            make("a", "Support Workspace", true),
            make("b", "AI Automation Consulting", true),
            make("c", "Draft only", false),
        ];
        let question = pick(&offers, "Find buyers for my product").unwrap_err();
        assert!(question.contains("Which offer"), "{question}");
        assert!(!question.contains("Draft only"));
        assert_eq!(
            pick(&offers, "Find buyers for Support Workspace")
                .unwrap()
                .id,
            "a"
        );
        assert_eq!(pick(&offers[..1], "my product").unwrap().id, "a");
        assert!(pick(&offers[2..], "my product").is_err());
    }

    #[test]
    fn a_named_offer_is_the_one_used() {
        let make = |id: &str, name: &str, reviewed: bool, archived: bool| Offer {
            id: id.into(),
            name: name.into(),
            kind: OfferKind::Service,
            current_version: reviewed.then_some(2),
            draft: None,
            reviewed: None,
            reviewed_at: None,
            archived,
            created_at: 0,
            updated_at: 0,
            revision: 0,
        };
        let offers = vec![
            make("a", "Support Workspace", true, false),
            make("b", "Support Workspace Pro", true, false),
            make("c", "Legacy Helpdesk", true, true),
            make("d", "Draft only", false, false),
        ];
        // Whole words, any case; the longest name wins.
        assert!(mentions(
            "Find buyers for my support workspace.",
            "Support Workspace"
        ));
        assert!(!mentions(
            "Find buyers for Support Workspaces",
            "Support Workspace"
        ));
        assert!(!mentions("Find buyers", "AI"));
        assert_eq!(
            pick(&offers, "Clients for Support Workspace Pro")
                .unwrap()
                .id,
            "b"
        );
        assert_eq!(
            pick(&offers, "Clients for support workspace").unwrap().id,
            "a"
        );
        // A named offer that cannot be used is never swapped for another.
        let archived = pick(&offers, "Clients for Legacy Helpdesk").unwrap_err();
        assert!(
            archived.contains("\"Legacy Helpdesk\" is archived"),
            "{archived}"
        );
        let draft = pick(&offers, "Clients for Draft only").unwrap_err();
        assert!(draft.contains("not been reviewed"), "{draft}");
        assert!(
            for_task(&offers, "Clients for Legacy Helpdesk every Monday")
                .unwrap_err()
                .contains("this task does not run")
        );
        // "v2" after the name pins that reviewed version; later ones do not exist.
        assert_eq!(
            for_task(&offers, "Find clients for SUPPORT WORKSPACE v2 weekly").unwrap(),
            ("a".to_string(), Some(2))
        );
        assert!(for_task(&offers, "Find clients for Support Workspace v3")
            .unwrap_err()
            .contains("no reviewed version 3"));
        // The same name archived and in use: the usable one.
        let renamed = vec![
            make("old", "Support Workspace", true, true),
            make("new", "Support Workspace", true, false),
        ];
        assert_eq!(
            pick(&renamed, "Clients for Support Workspace").unwrap().id,
            "new"
        );
    }
}
