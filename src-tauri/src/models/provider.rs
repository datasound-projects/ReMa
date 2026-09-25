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

    /// How ReMa can connect to this provider, preferred method first.
    ///
    /// Account sign-in goes through the provider's official local runtime
    /// (see `accounts`); it is offered only where the provider permits a
    /// third-party app to use it. Anthropic does not allow third-party apps
    /// to offer Claude.ai (Free/Pro/Max) sign-in, so its account option is
    /// the Claude Console (API) account.
    pub fn connection_methods(self) -> &'static [ConnectionMethod] {
        match self {
            Self::Openai => &[ConnectionMethod::ChatgptAccount, ConnectionMethod::ApiKey],
            Self::Anthropic => &[ConnectionMethod::ClaudeConsole, ConnectionMethod::ApiKey],
            Self::Gemini | Self::OpenaiCompatible => &[ConnectionMethod::ApiKey],
        }
    }

    /// The account sign-in this provider offers, if any.
    pub fn account_method(self) -> Option<ConnectionMethod> {
        self.connection_methods()
            .iter()
            .copied()
            .find(|m| m.is_account())
    }
}

/// How ReMa reaches a provider: which transport carries the requests and
/// how they are authenticated. One provider has one active method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionMethod {
    /// The provider's HTTPS API with a key kept in the OS credential store
    /// (or no key, for local endpoints).
    ApiKey,
    /// OpenAI through the official Codex runtime, signed in with a ChatGPT
    /// account. Codex stores the credentials and sends the requests.
    ChatgptAccount,
    /// The Anthropic API with a Claude Console account sign-in managed by
    /// the official Anthropic CLI, which stores and refreshes the token.
    ClaudeConsole,
}

text_enum!(ConnectionMethod {
    ApiKey => "api_key",
    ChatgptAccount => "chatgpt_account",
    ClaudeConsole => "claude_console",
});

impl ConnectionMethod {
    /// Signed in through a browser rather than with a pasted key.
    pub fn is_account(self) -> bool {
        !matches!(self, Self::ApiKey)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::ApiKey => "API key",
            Self::ChatgptAccount => "ChatGPT account",
            Self::ClaudeConsole => "Claude Console account",
        }
    }
}

/// What ReMa itself stores for a provider (in the OS credential store).
/// Account connections store nothing: their runtime keeps the credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    None,
    ApiKey,
    #[serde(rename = "oauth")]
    OAuth,
}

text_enum!(AuthMethod { None => "none", ApiKey => "api_key", OAuth => "oauth" });

/// Whether a saved connection works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Disconnected,
    Connected,
    /// The sign-in expired and could not be renewed.
    Expired,
    /// The runtime is signed out (e.g. signed out elsewhere).
    ReauthRequired,
    /// The runtime is missing or failed; `status_message` says why.
    Unavailable,
}

/// A browser sign-in that is running or just ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SignInStatus {
    Connecting,
    OpeningBrowser,
    WaitingForAuthorization,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SignInView {
    pub method: ConnectionMethod,
    pub status: SignInStatus,
    pub message: Option<String>,
    /// Device-code sign-in: the one-time code to enter on the provider's page.
    pub user_code: Option<String>,
    /// Device-code sign-in: where to enter it.
    pub verification_url: Option<String>,
}

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

/// A provider as shown in Settings. Never contains secrets or tokens.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub kind: ProviderKind,
    pub name: String,
    pub base_url: Option<String>,
    /// Configured model of an OpenAI-compatible endpoint.
    pub configured_model: Option<String>,
    /// How this provider can be connected, preferred first.
    pub connection_methods: Vec<ConnectionMethod>,
    /// The active connection, if connected.
    pub connection: Option<ConnectionMethod>,
    /// Who is signed in (account connections), e.g. "ana@example.com · Plus".
    pub account_label: Option<String>,
    pub status: ConnectionStatus,
    pub status_message: Option<String>,
    /// A browser sign-in running or just ended for this provider.
    pub sign_in: Option<SignInView>,
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
