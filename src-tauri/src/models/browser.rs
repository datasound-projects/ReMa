//! The built-in browser workspace and ReMa Auto Fill.

use serde::{Deserialize, Serialize};
use specta::Type;

/// What the browser workspace shows. Contains no page content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatus {
    pub open: bool,
    pub url: Option<String>,
    pub title: Option<String>,
    pub loading: bool,
}

/// Where the web page is drawn, in CSS pixels of ReMa's window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The browser's page or state changed.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct BrowserChanged(pub BrowserStatus);

/// A form field ReMa filled from the profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FilledField {
    /// The field's label on the page.
    pub label: String,
    /// Which profile value was used, e.g. "Email".
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FileFieldKind {
    Resume,
    CoverLetter,
    Other,
}

/// A file upload field on the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileField {
    /// Identifies the field for `attach_profile_document`.
    pub field: u32,
    pub label: String,
    pub kind: FileFieldKind,
}

/// What ReMa Auto Fill did. Nothing is ever submitted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AutofillResult {
    pub filled: Vec<FilledField>,
    /// Recognized fields that already had a value (left unchanged).
    pub kept: u32,
    /// Recognized fields the profile has no value for, e.g. "Phone".
    pub missing: Vec<String>,
    pub files: Vec<FileField>,
    /// Other questions left for the user.
    pub questions: Vec<String>,
    /// Forms embedded from another site, which Auto Fill cannot reach here.
    /// Opening one directly lets Auto Fill work on it.
    pub embedded_forms: Vec<String>,
}
