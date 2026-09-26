//! The user's career profile: the reusable source of truth for Auto Fill and
//! (when the user turns it on) chat context.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::text_enum;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Experience {
    pub title: String,
    pub company: String,
    pub location: String,
    /// Free text as written by the user or the CV, e.g. "2021-03" or "Mar 2021".
    pub start: String,
    pub end: String,
    /// Still in this position (`end` is ignored).
    pub current: bool,
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Education {
    pub school: String,
    pub degree: String,
    pub field: String,
    pub start: String,
    pub end: String,
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Language {
    pub name: String,
    /// E.g. "Native", "C1", "Fluent".
    pub level: String,
}

/// A link beyond the standard ones (website, GitHub, LinkedIn).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileLink {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CustomFieldKind {
    Text,
    Url,
    /// A Profile document (certificate, portfolio, …).
    File,
}

text_enum!(CustomFieldKind { Text => "text", Url => "url", File => "file" });

/// A field the user defines, e.g. "Research profile" → URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomField {
    pub label: String,
    pub kind: CustomFieldKind,
    /// Text or URL (empty for files).
    pub value: String,
    /// For `File` fields.
    pub document_id: Option<i64>,
}

/// The structured profile. Every field is optional (empty) and editable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    // Personal information
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: String,
    pub location: String,
    // Professional information
    /// Professional title, e.g. "Data Engineer".
    pub title: String,
    pub summary: String,
    pub skills: Vec<String>,
    pub experience: Vec<Experience>,
    pub education: Vec<Education>,
    pub languages: Vec<Language>,
    // Links
    pub website: String,
    /// A resume or portfolio website, usable instead of a CV file.
    pub resume_website: String,
    pub github: String,
    pub linkedin: String,
    pub other_links: Vec<ProfileLink>,
    pub custom_fields: Vec<CustomField>,
}

impl Profile {
    pub fn is_empty(&self) -> bool {
        *self == Profile::default()
    }

    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Cv,
    Certificate,
    Portfolio,
    Other,
}

text_enum!(DocumentKind {
    Cv => "cv",
    Certificate => "certificate",
    Portfolio => "portfolio",
    Other => "other",
});

/// File formats ReMa accepts. Text is extracted from all but images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormat {
    Pdf,
    Docx,
    Text,
    Markdown,
    Png,
    Jpeg,
    Webp,
}

text_enum!(DocumentFormat {
    Pdf => "pdf",
    Docx => "docx",
    Text => "text",
    Markdown => "markdown",
    Png => "png",
    Jpeg => "jpeg",
    Webp => "webp",
});

impl DocumentFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Text => "txt",
            Self::Markdown => "md",
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }

    pub fn is_image(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp)
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Text => "text/plain",
            Self::Markdown => "text/markdown",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

/// A file stored in ReMa's application data folder. The file itself never
/// leaves Rust except when the user attaches it to an application form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDocument {
    pub id: i64,
    pub name: String,
    pub kind: DocumentKind,
    pub format: DocumentFormat,
    /// File name as uploaded.
    pub original_name: String,
    /// Bytes.
    pub size: i64,
    /// Readable text was extracted (it can be imported into the profile).
    pub has_text: bool,
    /// The main CV: preferred when Profile context has to be shortened.
    pub is_primary: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Degree,
    ProfessionalCertificate,
    CourseCertificate,
    Training,
    License,
    Badge,
    Other,
}

text_enum!(CredentialKind {
    Degree => "degree",
    ProfessionalCertificate => "professional_certificate",
    CourseCertificate => "course_certificate",
    Training => "training",
    License => "license",
    Badge => "badge",
    Other => "other",
});

impl CredentialKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Degree => "Degree",
            Self::ProfessionalCertificate => "Professional certificate",
            Self::CourseCertificate => "Course certificate",
            Self::Training => "Training",
            Self::License => "License",
            Self::Badge => "Badge",
            Self::Other => "Credential",
        }
    }
}

/// A degree, certificate, license or other career evidence. Everything but
/// the title is optional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCredential {
    pub id: i64,
    pub kind: CredentialKind,
    pub title: String,
    pub issuer: String,
    /// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`; empty if unknown.
    pub issue_date: String,
    pub expiration_date: String,
    pub credential_id: String,
    pub credential_url: String,
    pub note: String,
    /// The attached file, if any.
    pub document: Option<ProfileDocument>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// What the user enters for a credential.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInput {
    pub kind: Option<CredentialKind>,
    pub title: String,
    pub issuer: String,
    pub issue_date: String,
    pub expiration_date: String,
    pub credential_id: String,
    pub credential_url: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    /// The optional Custom Profile (structured fields).
    pub profile: Profile,
    /// Every stored file: CVs, credential files and other documents.
    pub documents: Vec<ProfileDocument>,
    pub credentials: Vec<ProfileCredential>,
    /// When the Custom Profile was last saved; `None` if never.
    pub updated_at: Option<i64>,
}

/// A file that could not be added, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RejectedFile {
    pub name: String,
    pub reason: String,
}

/// The outcome of adding several files at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddDocumentsResult {
    pub added: Vec<ProfileDocument>,
    pub failed: Vec<RejectedFile>,
}

/// A safe, structured rendering of a Word document for the in-app viewer:
/// plain text blocks, never the document's own markup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DocumentBlock {
    Heading { level: u8, text: String },
    Paragraph { text: String },
    ListItem { level: u8, text: String },
    Table { rows: Vec<Vec<String>> },
}

/// Profile details read from a document, for the user to review. Nothing is
/// saved until the user accepts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileImport {
    pub document: ProfileDocument,
    pub extracted: Profile,
    /// The model that read the document, if one was used.
    pub model: Option<String>,
    /// What could not be read, and why.
    pub notes: Vec<String>,
}

/// The profile or its documents changed.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ProfileChanged;
