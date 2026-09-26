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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioDocument {
    pub id: i64,
    pub name: String,
    pub template_id: String,
    pub page_size: PageSize,
    /// `#rrggbb`, or empty for the template's own color.
    pub accent: String,
    pub content: PortfolioContent,
    pub created_at: i64,
    pub updated_at: i64,
}

/// What the editor saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioInput {
    pub name: String,
    pub template_id: String,
    pub page_size: PageSize,
    pub accent: String,
    pub content: PortfolioContent,
}

/// How a new document starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PortfolioStart {
    /// Empty standard sections.
    Blank,
    /// A one-time copy of the Custom Profile and credentials.
    CustomProfile,
}

/// Portfolio documents were created, changed or deleted.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct PortfolioChanged;
