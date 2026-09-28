//! Portfolio Studio's AI assistant.
//!
//! Every request goes to the model the user chose in Settings (the same
//! provider integration as chat) and comes back as a proposal the user
//! reviews: nothing is applied by ReMa. The model is told not to invent
//! employers, qualifications, dates, responsibilities or figures, and the
//! proposal is checked afterwards: figures and years that the source
//! material does not contain, and entries that were not in the document,
//! are listed as warnings next to the proposal.

use std::collections::HashSet;

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::{
    db::profile as profile_repo,
    error::{AppError, AppResult},
    jobs::extract::json_object,
    llm::{ChatRequest, Finish, Turn},
    models::{
        chat::MessageRole,
        portfolio::{
            AiAction, AiProposal, AiRequest, AiScope, AiSource, CoverLetter, PortfolioContent,
            PortfolioEntry, PortfolioHeader, PortfolioKind, PortfolioSection, SectionKind,
        },
        provider::ModelRef,
    },
    services::{portfolio, providers},
    state::AppState,
};

/// Characters of text sent to the model per part.
const MAX_INPUT: usize = 30_000;

pub const RULES: &str = "You are the writing assistant of ReMa's Portfolio Studio, helping with a CV or a \
cover letter. Rules:\n\
- Never invent employers, job titles, qualifications, dates, responsibilities, figures, \
percentages or achievements. Use only facts in the material you are given.\n\
- If a stronger version needs information that is missing (a figure, a date, a team \
size), keep the text without it and list what is missing in \"notes\".\n\
- Keep the person's meaning and the language of the text unless asked to translate.\n\
- Keep placeholders and formatting: lines starting with \"- \" are bullets; **bold**, \
_italic_ and [text](url) are inline formatting.\n\
- Answer with one JSON object only, no prose before or after it.";

/// What the model answers with, for any scope. Missing fields are fine.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawAnswer {
    text: String,
    section: Option<RawSection>,
    content: Option<RawContent>,
    letter: Option<RawLetter>,
    notes: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawContent {
    pub header: RawHeader,
    pub sections: Vec<RawSection>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawHeader {
    pub full_name: String,
    pub headline: String,
    pub email: String,
    pub phone: String,
    pub location: String,
    pub website: String,
    pub linkedin: String,
    pub github: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawSection {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub visible: Option<bool>,
    pub text: String,
    pub entries: Vec<RawEntry>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawEntry {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub location: String,
    pub start: String,
    pub end: String,
    pub url: String,
    pub description: String,
    pub tags: Vec<String>,
    /// The model was not sure it read this right.
    pub uncertain: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawLetter {
    pub recipient_name: String,
    pub recipient_title: String,
    pub company: String,
    pub address: String,
    pub position: String,
    pub date: String,
    pub subject: String,
    pub greeting: String,
    pub body: String,
    pub closing: String,
    pub signature: String,
}

pub fn section_kind(name: &str) -> SectionKind {
    match name.trim().to_lowercase().replace([' ', '-'], "_").as_str() {
        "summary" | "profile" | "about" | "objective" => SectionKind::Summary,
        "experience" | "work" | "work_experience" | "employment" => SectionKind::Experience,
        "projects" => SectionKind::Projects,
        "education" => SectionKind::Education,
        "skills" => SectionKind::Skills,
        "languages" => SectionKind::Languages,
        "certifications" | "certificates" | "credentials" => SectionKind::Certifications,
        "publications" => SectionKind::Publications,
        "links" => SectionKind::Links,
        _ => SectionKind::Custom,
    }
}

pub fn from_raw_entry(e: RawEntry, keep_id: bool) -> PortfolioEntry {
    PortfolioEntry {
        id: if keep_id && !e.id.is_empty() { e.id } else { portfolio::new_id() },
        title: e.title,
        subtitle: e.subtitle,
        location: e.location,
        start: e.start,
        end: e.end,
        url: e.url,
        description: e.description,
        tags: e.tags,
    }
}

pub fn from_raw_section(s: RawSection, keep_ids: bool) -> PortfolioSection {
    let kind = section_kind(&s.kind);
    PortfolioSection {
        id: if keep_ids && !s.id.is_empty() { s.id } else { portfolio::new_id() },
        kind,
        title: if s.title.trim().is_empty() {
            kind.default_title().into()
        } else {
            s.title
        },
        visible: s.visible.unwrap_or(true),
        text: s.text,
        entries: s
            .entries
            .into_iter()
            .map(|e| from_raw_entry(e, keep_ids))
            .collect(),
    }
}

pub fn from_raw_content(c: RawContent, keep_ids: bool) -> PortfolioContent {
    PortfolioContent {
        header: PortfolioHeader {
            full_name: c.header.full_name,
            headline: c.header.headline,
            email: c.header.email,
            phone: c.header.phone,
            location: c.header.location,
            website: c.header.website,
            linkedin: c.header.linkedin,
            github: c.header.github,
            photo: String::new(),
        },
        sections: c
            .sections
            .into_iter()
            .map(|s| from_raw_section(s, keep_ids))
            .collect(),
    }
}

pub fn from_raw_letter(l: RawLetter) -> CoverLetter {
    CoverLetter {
        recipient_name: l.recipient_name,
        recipient_title: l.recipient_title,
        company: l.company,
        address: l.address,
        position: l.position,
        date: l.date,
        subject: l.subject,
        greeting: l.greeting,
        body: l.body,
        closing: l.closing,
        signature: l.signature,
    }
}

// ── The model ───────────────────────────────────────────────────────

/// The model chosen in Settings, or a clear error.
pub fn default_model(state: &AppState) -> AppResult<ModelRef> {
    providers::catalog(state)?.default_model.ok_or_else(|| {
        AppError::validation(
            "No model is set up. Connect a provider and choose a default model in Settings \
             to use the AI assistant.",
        )
    })
}

/// Sends one request to the chosen model and returns the whole answer.
pub async fn ask(
    state: &AppState,
    model: &ModelRef,
    system: &str,
    prompt: String,
    max_output_tokens: u32,
) -> AppResult<String> {
    let endpoint = providers::resolve_endpoint(state, &model.provider_id).await?;
    let request = ChatRequest {
        system: Some(system.into()),
        turns: vec![Turn {
            role: MessageRole::User,
            content: prompt,
        }],
        max_output_tokens: Some(max_output_tokens),
        ..ChatRequest::default()
    };
    let mut answer = String::new();
    let mut sink = |delta: &str| answer.push_str(delta);
    let finish = state
        .llm
        .stream_chat(
            &endpoint,
            &model.model_id,
            &request,
            CancellationToken::new(),
            &mut sink,
        )
        .await?;
    if finish == Finish::Cancelled {
        return Err(AppError::validation("The request was cancelled."));
    }
    Ok(answer)
}

fn cut(text: &str) -> String {
    text.chars().take(MAX_INPUT).collect()
}

// ── Requests ────────────────────────────────────────────────────────

fn task(request: &AiRequest) -> String {
    let what = match request.scope {
        AiScope::Selection => "the selected text",
        AiScope::Section => "the section",
        AiScope::Document => match request.kind {
            PortfolioKind::Cv => "the whole CV",
            PortfolioKind::CoverLetter => "the whole cover letter",
        },
    };
    let mut task = match request.action {
        AiAction::Improve => format!(
            "Improve the wording of {what}: clearer, more specific and more professional, \
             without adding facts."
        ),
        AiAction::Shorten => format!("Shorten {what}, keeping every fact that matters."),
        AiAction::Expand => format!(
            "Expand {what} a little, using only the facts already present in the material. \
             Where a detail would be needed, say so in notes instead of inventing it."
        ),
        AiAction::Achievements => format!(
            "Rewrite {what} as achievement-oriented bullet points (lines starting with \"- \"), \
             each starting with a strong verb. Use only figures that are in the material; \
             list in notes where a figure would strengthen a bullet."
        ),
        AiAction::Tailor => format!(
            "Tailor {what} to the job posting below: emphasise the matching experience and \
             skills, mirror its terminology where it is honest, and reorder bullets. \
             Do not claim skills or experience the material does not contain; list the \
             posting's requirements that are not covered in notes."
        ),
        AiAction::Translate => format!(
            "Translate {what} into {}. Keep names, employers and formatting.",
            if request.target.trim().is_empty() {
                "English"
            } else {
                request.target.trim()
            }
        ),
        AiAction::Tone => format!(
            "Rewrite {what} in a {} tone, keeping every fact.",
            if request.target.trim().is_empty() {
                "confident and professional"
            } else {
                request.target.trim()
            }
        ),
        AiAction::SuggestMissing => format!(
            "Review {what} and list in notes what is missing or weak for a strong application \
             (missing dates, figures, responsibilities, sections, contact details). Return the \
             text unchanged, apart from fixing obvious typos."
        ),
        AiAction::Populate => format!(
            "Fill {what} from the source material below. Add sections and entries for the \
             facts it contains, keep what is already filled in, and never invent anything \
             that is not in the material. Put the entries where they belong (experience, \
             education, skills, languages, certifications, projects, publications, links)."
        ),
        AiAction::Custom => format!("Apply this instruction to {what}."),
    };
    if !request.instruction.trim().is_empty() {
        task.push_str("\nInstruction from the user: ");
        task.push_str(request.instruction.trim());
    }
    task
}

fn answer_shape(request: &AiRequest) -> &'static str {
    match (request.scope, request.kind) {
        (AiScope::Selection, _) => {
            "Answer format: {\"text\": \"the replacement text\", \"notes\": [\"…\"]}"
        }
        (AiScope::Section, _) => {
            "Answer format: {\"section\": {\"id\": same id, \"kind\": same kind, \"title\": \"…\", \
             \"text\": \"…\", \"entries\": [{\"id\": keep ids of kept entries, \"title\": \"…\", \
             \"subtitle\": \"…\", \"location\": \"…\", \"start\": \"…\", \"end\": \"…\", \"url\": \"…\", \
             \"description\": \"…\", \"tags\": [\"…\"]}]}, \"notes\": [\"…\"]}"
        }
        (AiScope::Document, PortfolioKind::Cv) => {
            "Answer format: {\"content\": {\"header\": {\"fullName\", \"headline\", \"email\", \
             \"phone\", \"location\", \"website\", \"linkedin\", \"github\"}, \"sections\": [{\"id\": \
             keep ids of kept sections, \"kind\": one of summary, experience, projects, \
             education, skills, languages, certifications, publications, links, custom, \
             \"title\", \"visible\", \"text\", \"entries\": [{\"id\", \"title\", \"subtitle\", \
             \"location\", \"start\", \"end\", \"url\", \"description\", \"tags\"}]}]}, \
             \"notes\": [\"…\"]}"
        }
        (AiScope::Document, PortfolioKind::CoverLetter) => {
            "Answer format: {\"letter\": {\"recipientName\", \"recipientTitle\", \"company\", \
             \"address\", \"position\", \"date\", \"subject\", \"greeting\", \"body\", \"closing\", \
             \"signature\"}, \"notes\": [\"…\"]}"
        }
    }
}

fn section_json(content: &PortfolioContent, id: &str) -> AppResult<String> {
    let section = content
        .sections
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| AppError::validation("Choose a section first."))?;
    serde_json::to_string_pretty(section).map_err(|e| AppError::internal(e.to_string()))
}

/// The facts the model may use, as text, for the fact check afterwards.
async fn source_material(state: &AppState, request: &AiRequest) -> AppResult<(String, Vec<String>)> {
    let mut notes = Vec::new();
    let mut material = String::new();
    if request.action == AiAction::Populate {
        match request.source.unwrap_or(AiSource::Profile) {
            AiSource::Profile => {
                let content = state.db.call(|c| {
                    let (profile, _) = profile_repo::get(c)?;
                    Ok(portfolio::content_from_profile(
                        &profile,
                        &profile_repo::list_credentials(c)?,
                    ))
                })?;
                material = portfolio::plain_text(&content);
                if material.trim().is_empty() {
                    notes.push("Your Custom Profile is empty, so there is nothing to fill in from it.".into());
                }
            }
            AiSource::Document => {
                let id = request
                    .source_document_id
                    .ok_or_else(|| AppError::validation("Choose a document to fill in from."))?;
                let text = state.db.call(|c| profile_repo::document_text(c, id))?;
                match text {
                    Some(text) if !text.trim().is_empty() => material = text,
                    _ => {
                        return Err(AppError::validation(
                            "That document has no readable text (a scanned file or an image), \
                             so nothing can be filled in from it.",
                        ))
                    }
                }
            }
        }
    }
    Ok((material, notes))
}

/// Runs one assistant request and returns the proposal for review.
pub async fn assist(state: &AppState, request: AiRequest) -> AppResult<AiProposal> {
    if request.action == AiAction::Custom && request.instruction.trim().is_empty() {
        return Err(AppError::validation("Type an instruction first."));
    }
    if request.scope == AiScope::Selection && request.selection.trim().is_empty() {
        return Err(AppError::validation("Select some text first."));
    }
    if request.action == AiAction::Tailor && request.job_text.trim().is_empty() {
        return Err(AppError::validation("Paste or choose the job posting first."));
    }
    let model = default_model(state)?;
    let (material, mut notes) = source_material(state, &request).await?;

    let mut prompt = String::new();
    prompt.push_str(&task(&request));
    prompt.push_str("\n\n");
    prompt.push_str(answer_shape(&request));
    prompt.push_str("\n\n");
    let document_text = match request.kind {
        PortfolioKind::Cv => portfolio::plain_text(&request.content),
        PortfolioKind::CoverLetter => letter_text(&request.content, &request.letter),
    };
    match request.scope {
        AiScope::Selection => {
            prompt.push_str("Selected text:\n");
            prompt.push_str(&cut(&request.selection));
            prompt.push_str("\n\nThe whole document, for context only (do not return it):\n");
            prompt.push_str(&cut(&document_text));
        }
        AiScope::Section => {
            prompt.push_str("Section (JSON):\n");
            prompt.push_str(&section_json(&request.content, &request.section_id)?);
            prompt.push_str("\n\nThe whole document, for context only (do not return it):\n");
            prompt.push_str(&cut(&document_text));
        }
        AiScope::Document => match request.kind {
            PortfolioKind::Cv => {
                prompt.push_str("CV (JSON):\n");
                prompt.push_str(
                    &serde_json::to_string_pretty(&request.content)
                        .map_err(|e| AppError::internal(e.to_string()))?,
                );
            }
            PortfolioKind::CoverLetter => {
                prompt.push_str("Cover letter (JSON):\n");
                prompt.push_str(
                    &serde_json::to_string_pretty(&request.letter)
                        .map_err(|e| AppError::internal(e.to_string()))?,
                );
                prompt.push_str("\n\nThe sender's CV, for facts only:\n");
                prompt.push_str(&cut(&portfolio::plain_text(&request.content)));
            }
        },
    }
    if !request.job_text.trim().is_empty() {
        prompt.push_str("\n\nJob posting:\n");
        prompt.push_str(&cut(&request.job_text));
    }
    if !material.is_empty() {
        prompt.push_str("\n\nSource material:\n");
        prompt.push_str(&cut(&material));
    }

    let answer = ask(state, &model, RULES, prompt, 8_000).await?;
    let raw: RawAnswer = serde_json::from_str(json_object(&answer)?)
        .map_err(|_| AppError::provider("the answer was not in the expected format"))?;
    notes.extend(raw.notes.into_iter().filter(|n| !n.trim().is_empty()).take(20));

    // Everything the proposal may legitimately contain.
    let mut sources = vec![document_text, request.selection.clone(), material];
    if request.action == AiAction::Tailor {
        sources.push(request.job_text.clone());
    }
    if request.action == AiAction::Translate {
        // Translated numbers are the same numbers; dates may be reformatted.
        sources.push(String::new());
    }
    let known = sources.iter().flat_map(|s| figures(s)).collect::<HashSet<_>>();

    let mut proposal = AiProposal {
        scope: request.scope,
        text: None,
        section: None,
        content: None,
        letter: None,
        notes,
        warnings: Vec::new(),
        model: model.model_id.clone(),
    };
    match request.scope {
        AiScope::Selection => {
            if raw.text.trim().is_empty() {
                return Err(AppError::provider("the model returned no text"));
            }
            proposal.warnings = unknown_figures(&raw.text, &known);
            proposal.text = Some(raw.text);
        }
        AiScope::Section => {
            let raw = raw
                .section
                .ok_or_else(|| AppError::provider("the model returned no section"))?;
            let mut section = from_raw_section(raw, true);
            section.id = request.section_id.clone();
            let original = request
                .content
                .sections
                .iter()
                .find(|s| s.id == request.section_id);
            if let Some(original) = original {
                section.kind = original.kind;
                proposal
                    .warnings
                    .extend(new_entries(&original.entries, &section.entries));
            }
            proposal.warnings.extend(unknown_figures(
                &portfolio::section_text(&section),
                &known,
            ));
            proposal.section = Some(section);
        }
        AiScope::Document => match request.kind {
            PortfolioKind::Cv => {
                let raw = raw
                    .content
                    .ok_or_else(|| AppError::provider("the model returned no content"))?;
                let mut content = from_raw_content(raw, true);
                content.header.photo = request.content.header.photo.clone();
                let before: Vec<&PortfolioEntry> = request
                    .content
                    .sections
                    .iter()
                    .flat_map(|s| s.entries.iter())
                    .collect();
                let after: Vec<PortfolioEntry> = content
                    .sections
                    .iter()
                    .flat_map(|s| s.entries.iter().cloned())
                    .collect();
                proposal.warnings.extend(new_entries_ref(&before, &after));
                proposal.warnings.extend(unknown_figures(
                    &portfolio::plain_text(&content),
                    &known,
                ));
                proposal.content = Some(content);
            }
            PortfolioKind::CoverLetter => {
                let raw = raw
                    .letter
                    .ok_or_else(|| AppError::provider("the model returned no letter"))?;
                let letter = from_raw_letter(raw);
                proposal.warnings.extend(unknown_figures(&letter.body, &known));
                proposal.letter = Some(letter);
            }
        },
    }
    proposal.warnings.truncate(20);
    Ok(proposal)
}

fn letter_text(content: &PortfolioContent, letter: &CoverLetter) -> String {
    let h = &content.header;
    [
        h.full_name.as_str(),
        h.headline.as_str(),
        h.email.as_str(),
        h.phone.as_str(),
        h.location.as_str(),
        letter.recipient_name.as_str(),
        letter.recipient_title.as_str(),
        letter.company.as_str(),
        letter.address.as_str(),
        letter.position.as_str(),
        letter.date.as_str(),
        letter.subject.as_str(),
        letter.greeting.as_str(),
        letter.body.as_str(),
        letter.closing.as_str(),
        letter.signature.as_str(),
    ]
    .into_iter()
    .filter(|s| !s.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n")
}

// ── Fact check ──────────────────────────────────────────────────────

/// Figures in a text: runs of digits (with separators) and their
/// percentage/currency forms, normalised to digits only.
pub fn figures(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, out: &mut Vec<String>| {
        let digits: String = current.chars().filter(char::is_ascii_digit).collect();
        if digits.len() >= 2 {
            out.push(digits);
        }
        current.clear();
    };
    for c in text.chars() {
        if c.is_ascii_digit() {
            current.push(c);
        } else if (c == '.' || c == ',' || c == '\u{a0}' || c == ' ') && !current.is_empty() {
            // A separator inside a number ("1,200", "2 000"): keep going
            // only if a digit follows; the next branch handles the rest.
            current.push(c);
        } else {
            flush(&mut current, &mut out);
        }
    }
    flush(&mut current, &mut out);
    out
}

/// Figures in the proposal that none of the sources contain.
pub fn unknown_figures(text: &str, known: &HashSet<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    figures(text)
        .into_iter()
        .filter(|f| !known.contains(f) && seen.insert(f.clone()))
        .map(|f| format!("“{f}” is not in your material. Check this figure before accepting."))
        .collect()
}

fn entry_key(e: &PortfolioEntry) -> String {
    format!("{}|{}", e.title.trim().to_lowercase(), e.subtitle.trim().to_lowercase())
}

/// Entries in the proposal that were not in the original section.
fn new_entries(before: &[PortfolioEntry], after: &[PortfolioEntry]) -> Vec<String> {
    new_entries_ref(&before.iter().collect::<Vec<_>>(), after)
}

fn new_entries_ref(before: &[&PortfolioEntry], after: &[PortfolioEntry]) -> Vec<String> {
    let known: HashSet<String> = before.iter().map(|e| entry_key(e)).collect();
    let ids: HashSet<&str> = before.iter().map(|e| e.id.as_str()).collect();
    after
        .iter()
        .filter(|e| !ids.contains(e.id.as_str()) && !known.contains(&entry_key(e)))
        .filter(|e| !e.title.trim().is_empty() || !e.subtitle.trim().is_empty())
        .map(|e| {
            let name = [e.title.trim(), e.subtitle.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" at ");
            format!("“{name}” is a new entry that was not in your document. Check it before accepting.")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        llm::fake::FakeLanguageModel,
        models::{portfolio::PortfolioStart, provider::ProviderKind},
        services::portfolio::{create, PortfolioCreate},
        state::testing,
    };

    async fn state_with(answer: &str) -> AppState {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[answer])));
        providers::connect(&state, ProviderKind::Anthropic, "k")
            .await
            .unwrap();
        providers::set_default_model(
            &state,
            &ModelRef {
                provider_id: "anthropic".into(),
                model_id: "model-a".into(),
            },
        )
        .unwrap();
        state
    }

    fn request(state: &AppState, scope: AiScope, action: AiAction) -> AiRequest {
        let doc = create(
            state,
            PortfolioCreate {
                template_id: "modern".into(),
                start: Some(PortfolioStart::Sample),
                ..Default::default()
            },
        )
        .unwrap();
        AiRequest {
            kind: PortfolioKind::Cv,
            section_id: doc.content.sections[1].id.clone(),
            content: doc.content,
            letter: doc.letter,
            action,
            scope,
            instruction: String::new(),
            selection: "Cut warehouse costs by 35%".into(),
            job_text: String::new(),
            target: String::new(),
            source: None,
            source_document_id: None,
        }
    }

    #[test]
    fn figures_are_found_with_separators() {
        assert_eq!(figures("Saved 1,200 hours (35%) in 2021."), ["1200", "35", "2021"]);
        assert!(figures("one 5 x").is_empty(), "single digits are not figures");
    }

    #[tokio::test]
    async fn selection_proposals_flag_figures_that_are_not_in_the_material() {
        let state = state_with(
            r#"{"text": "Cut warehouse costs by 35%, saving $900k a year", "notes": ["Add the team size"]}"#,
        )
        .await;
        let proposal = assist(&state, request(&state, AiScope::Selection, AiAction::Improve))
            .await
            .unwrap();
        assert_eq!(proposal.model, "model-a");
        assert_eq!(proposal.notes, ["Add the team size"]);
        assert!(proposal.text.unwrap().contains("35%"));
        assert_eq!(proposal.warnings.len(), 1, "{:?}", proposal.warnings);
        assert!(proposal.warnings[0].contains("900"));
    }

    #[tokio::test]
    async fn section_proposals_keep_ids_and_flag_new_entries() {
        let state = state_with(
            r#"{"section": {"id": "x", "kind": "experience", "title": "Experience", "entries": [
                {"id": "e1", "title": "Senior Data Engineer", "subtitle": "Northwind Analytics", "description": "- Led the platform"},
                {"title": "CTO", "subtitle": "Initech", "description": "- Ran everything"}]}}"#,
        )
        .await;
        let request = request(&state, AiScope::Section, AiAction::Achievements);
        let id = request.section_id.clone();
        let proposal = assist(&state, request).await.unwrap();
        let section = proposal.section.unwrap();
        assert_eq!(section.id, id, "the proposal replaces the same section");
        assert_eq!(section.kind, SectionKind::Experience);
        assert_eq!(proposal.warnings.len(), 1);
        assert!(proposal.warnings[0].contains("CTO at Initech"));
    }

    #[tokio::test]
    async fn requests_need_a_model_and_an_instruction() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        let mut r = request(&state, AiScope::Selection, AiAction::Custom);
        assert!(assist(&state, r.clone()).await.is_err(), "no instruction");
        r.instruction = "Make it bolder".into();
        let error = assist(&state, r).await.unwrap_err().to_string();
        assert!(error.contains("No model is set up"), "{error}");
    }
}
