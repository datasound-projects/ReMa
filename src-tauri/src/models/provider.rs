use serde::{Deserialize, Serialize};
use specta::Type;

use super::text_enum;

/// The API family a provider speaks. Each kind has one adapter in `llm/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Openai,
    Anthropic,
    Gemini,
    /// Any server implementing the OpenAI Chat Completions API.
    OpenaiCompatible,
}

text_enum!(ProviderKind {
    Openai => "openai",
    Anthropic => "anthropic",
    Gemini => "gemini",
    OpenaiCompatible => "openai_compatible",
});

impl ProviderKind {
    /// Cloud providers with a fixed id and endpoint.
    pub const BUILT_IN: [ProviderKind; 3] = [Self::Openai, Self::Anthropic, Self::Gemini];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Openai => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Gemini",
            Self::OpenaiCompatible => "OpenAI-Compatible",
        }
    }

    /// Authentication methods ReMa supports for this provider.
    ///
    /// OpenAI, Anthropic and Google do not offer an official OAuth flow that a
    /// third-party desktop app can use for their model APIs without its own
    /// registered client, so API keys are the supported method for now. The
    /// credential model (`secrets::Credential`) already carries OAuth tokens.
    pub fn auth_methods(self) -> &'static [AuthMethod] {
        match self {
            Self::OpenaiCompatible => &[AuthMethod::None, AuthMethod::ApiKey],
            _ => &[AuthMethod::ApiKey],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    None,
    ApiKey,
    #[serde(rename = "oauth")]
    OAuth,
}

text_enum!(AuthMethod { None => "none", ApiKey => "api_key", OAuth => "oauth" });

/// Identifies a model: which provider serves it and its provider-side id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    pub model_id: String,
    pub display_name: String,
    pub enabled: bool,
}

/// A provider as shown in Settings. Never contains secrets.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub kind: ProviderKind,
    pub name: String,
    pub base_url: Option<String>,
    /// Configured model of an OpenAI-compatible endpoint.
    pub configured_model: Option<String>,
    pub auth_methods: Vec<AuthMethod>,
    /// Saved in ReMa (built-in providers are listed even when not saved).
    pub configured: bool,
    /// A credential is stored in the OS credential store.
    pub has_credential: bool,
    pub models: Vec<ProviderModel>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    pub providers: Vec<ProviderView>,
    pub default_model: Option<ModelRef>,
}

/// A model the user can pick in the chat.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub model: ModelRef,
    pub provider_name: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub models: Vec<ModelOption>,
    pub default_model: Option<ModelRef>,
}

/// Create or update an OpenAI-compatible endpoint.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderInput {
    /// `None` creates a new endpoint.
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub model: String,
    /// `None` keeps the stored key; an empty string removes it.
    pub api_key: Option<String>,
}

/// Provider configuration or models changed.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ProvidersChanged;
