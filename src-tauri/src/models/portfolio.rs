//! Portfolio Studio: CVs the user builds in ReMa, separate from uploaded
//! files. Templates are part of the interface; a document stores its
//! content once, so switching templates never loses anything.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::text_enum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PageSize {
    A4,
    Letter,
}

text_enum!(PageSize { A4 => "a4", Letter => "letter" });

/// What a Portfolio Studio document is: a CV, or a cover letter (the same
/// header, a letter body instead of sections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum PortfolioKind {
    #[default]
    Cv,
    CoverLetter,
}

text_enum!(PortfolioKind { Cv => "cv", CoverLetter => "cover_letter" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Summary,
    Experience,
    Projects,
    Education,
    Skills,
    Languages,
    Certifications,
    Publications,
    Links,
    Custom,
}

impl SectionKind {
    pub fn default_title(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Experience => "Experience",
            Self::Projects => "Projects",
            Self::Education => "Education",
            Self::Skills => "Skills",
            Self::Languages => "Languages",
            Self::Certifications => "Certifications",
            Self::Publications => "Publications",
            Self::Links => "Links",
            Self::Custom => "Section",
        }
    }
}

/// Name and contact details at the top of the document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioHeader {
    pub full_name: String,
    /// E.g. "Senior Data Engineer".
    pub headline: String,
    pub email: String,
    pub phone: String,
    pub location: String,
    pub website: String,
    pub linkedin: String,
    pub github: String,
    /// An optional photo as a `data:image/…;base64,…` URL (templates that
    /// place one show it; the others leave it out).
    #[serde(default)]
    pub photo: String,
}

/// One item of a section. Fields are used by kind, for example
/// experience: title = role, subtitle = company; skills: title = group,
/// tags = skills; languages: title = language, subtitle = level.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioEntry {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub location: String,
    pub start: String,
    pub end: String,
    pub url: String,
    /// Multi-line; lines starting with "- " are shown as bullets.
    pub description: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioSection {
    pub id: String,
    pub kind: SectionKind,
    pub title: String,
    /// Hidden sections keep their content but are not shown or exported.
    pub visible: bool,
    /// Free text (summary and custom sections).
    pub text: String,
    pub entries: Vec<PortfolioEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioContent {
    pub header: PortfolioHeader,
    /// In display order.
    pub sections: Vec<PortfolioSection>,
}

/// The user's design choices on top of a template. Every field is empty
/// (or 0) for "the template's own"; the interface knows the allowed values
/// and the layout engine applies them. Kept as JSON so older documents
/// read as "template defaults".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct PortfolioStyle {
    /// A curated palette id, or empty.
    pub palette: String,
    /// `#rrggbb` overrides, or empty.
    pub heading_color: String,
    pub text_color: String,
    pub background_color: String,
    pub panel_color: String,
    /// A font pairing id, or empty.
    pub font_pairing: String,
    /// Type scale multiplier (0 = the template's).
    pub scale: f64,
    /// `narrow`, `normal`, `wide`, or empty.
    pub margins: String,
    /// `tight`, `normal`, `relaxed`, or empty.
    pub line_spacing: String,
    /// `tight`, `normal`, `relaxed`, or empty.
    pub section_spacing: String,
    /// `single`, `sidebar_left`, `sidebar_right`, `columns_right`, or empty.
    pub columns: String,
    /// `none`, `dots`, `grid`, `diagonal`, or empty.
    pub pattern: String,
    /// `none`, `hairline`, `dotted`, `thick`, or empty.
    pub dividers: String,
    /// `left`, `center`, `band`, `split`, `stacked`, or empty.
    pub header: String,
    /// `tinted`, `plain`, `outlined`, or empty.
    pub sidebar: String,
    /// `left`, `justify`, or empty.
    pub text_align: String,
    /// Show the header photo when there is one.
    pub show_photo: bool,
}

/// A cover letter's own parts (the sender comes from the header).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct CoverLetter {
    pub recipient_name: String,
    pub recipient_title: String,
    pub company: String,
    /// Multi-line address.
    pub address: String,
    pub position: String,
    pub date: String,
    pub subject: String,
    pub greeting: String,
    /// Paragraphs separated by blank lines.
    pub body: String,
    pub closing: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioDocument {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub kind: PortfolioKind,
    pub template_id: String,
    pub page_size: PageSize,
    /// `#rrggbb`, or empty for the template's own color.
    pub accent: String,
    #[serde(default)]
    pub style: PortfolioStyle,
    pub content: PortfolioContent,
    #[serde(default)]
    pub letter: CoverLetter,
    /// The Profile document this was rebuilt from, if any (kept as is).
    #[serde(default)]
    pub source_document_id: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// What the editor saves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioInput {
    pub name: String,
    #[serde(default)]
    pub kind: PortfolioKind,
    pub template_id: String,
    pub page_size: PageSize,
    pub accent: String,
    #[serde(default)]
    pub style: PortfolioStyle,
    pub content: PortfolioContent,
    #[serde(default)]
    pub letter: CoverLetter,
    #[serde(default)]
    pub source_document_id: Option<i64>,
}

/// What an import read from a file, for review before a document is made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioImport {
    /// The stored Profile document (the original file, kept as is).
    pub document_id: i64,
    pub document_name: String,
    pub kind: PortfolioKind,
    pub content: PortfolioContent,
    pub letter: CoverLetter,
    /// The model that read the text, if one was used.
    pub model: Option<String>,
    /// What could not be read, and why.
    pub notes: Vec<String>,
    /// Entry or field ids the model was unsure about.
    pub uncertain: Vec<String>,
    /// The original file has readable text (a scanned file has none).
    pub has_text: bool,
}

/// What the AI assistant is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AiAction {
    Improve,
    Shorten,
    Expand,
    Achievements,
    Tailor,
    Translate,
    Tone,
    SuggestMissing,
    Populate,
    Custom,
}

text_enum!(AiAction {
    Improve => "improve",
    Shorten => "shorten",
    Expand => "expand",
    Achievements => "achievements",
    Tailor => "tailor",
    Translate => "translate",
    Tone => "tone",
    SuggestMissing => "suggest_missing",
    Populate => "populate",
    Custom => "custom",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AiScope {
    Selection,
    Section,
    Document,
}

/// Where a `Populate` request takes its facts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AiSource {
    Profile,
    /// A Profile document (an older CV) by id.
    Document,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AiRequest {
    pub kind: PortfolioKind,
    pub content: PortfolioContent,
    pub letter: CoverLetter,
    pub action: AiAction,
    pub scope: AiScope,
    /// A free-text instruction (required for `Custom`, optional otherwise).
    #[serde(default)]
    pub instruction: String,
    /// The selected text (scope `Selection`).
    #[serde(default)]
    pub selection: String,
    /// The section (scope `Section`), or the section the selection is in.
    #[serde(default)]
    pub section_id: String,
    /// The job posting to tailor to (`Tailor`).
    #[serde(default)]
    pub job_text: String,
    /// Target language (`Translate`) or tone (`Tone`).
    #[serde(default)]
    pub target: String,
    /// `Populate`: where the facts come from.
    #[serde(default)]
    pub source: Option<AiSource>,
    #[serde(default)]
    pub source_document_id: Option<i64>,
}

/// A proposed change, shown for review; nothing is applied by ReMa.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AiProposal {
    pub scope: AiScope,
    /// Scope `Selection`: the replacement text.
    pub text: Option<String>,
    /// Scope `Section`: the replacement section (same id).
    pub section: Option<PortfolioSection>,
    /// Scope `Document`: the replacement content (CV) …
    pub content: Option<PortfolioContent>,
    /// … or letter (cover letter).
    pub letter: Option<CoverLetter>,
    /// What the model suggests adding or says is missing.
    pub notes: Vec<String>,
    /// Facts in the proposal that the source material does not contain.
    pub warnings: Vec<String>,
    pub model: String,
}

/// How a new document starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PortfolioStart {
    /// Empty standard sections.
    Blank,
    /// A one-time copy of the Custom Profile and credentials.
    CustomProfile,
    /// Sample content, to see a template with text in it.
    Sample,
}

/// Portfolio documents were created, changed or deleted.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct PortfolioChanged;
