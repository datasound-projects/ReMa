use serde::{Serialize, Serializer};
use specta::Type;

/// Result type used by services and commands.
pub type AppResult<T> = Result<T, AppError>;

/// Errors that can cross the IPC boundary.
///
/// Serialized to the frontend as `{ "code": "...", "message": "..." }` (see
/// [`ErrorPayload`]) so the UI can branch on a stable `code` instead of
/// parsing messages.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The caller sent input that breaks a rule (bad value, missing field).
    #[error("{0}")]
    Validation(String),

    /// The requested item does not exist.
    #[error("{0}")]
    NotFound(String),

    /// A file-system or other I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Reading from or writing to the application's storage failed.
    #[error("Database error: {0}")]
    Database(String),

    /// Settings or environment are missing or invalid.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Credentials are missing, invalid, expired or were rejected.
    #[error("{0}")]
    Authentication(String),

    /// An LLM provider returned an error (rate limit, bad request, outage).
    #[error("{0}")]
    Provider(String),

    /// The provider account has no credits or quota left; requests fail
    /// until the user adds some in the provider's billing settings.
    #[error("{0}")]
    Billing(String),

    /// The network request failed (offline, DNS, TLS, timeout).
    #[error("Network error: {0}")]
    Network(String),

    /// An unexpected failure inside the application.
    #[error("{0}")]
    Internal(String),
}

/// Stable, machine-readable error kind sent to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Validation,
    NotFound,
    Io,
    Database,
    Configuration,
    Authentication,
    Provider,
    Billing,
    Network,
    Internal,
}

/// Wire format of every backend error.
#[derive(Debug, Serialize, Type)]
pub struct ErrorPayload {
    pub code: ErrorCode,
    pub message: String,
}

impl AppError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn database(message: impl Into<String>) -> Self {
        Self::Database(message.into())
    }

    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration(message.into())
    }

    pub fn authentication(message: impl Into<String>) -> Self {
        Self::Authentication(message.into())
    }

    pub fn provider(message: impl Into<String>) -> Self {
        Self::Provider(message.into())
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::Network(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Validation(_) => ErrorCode::Validation,
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::Io(_) => ErrorCode::Io,
            Self::Database(_) => ErrorCode::Database,
            Self::Configuration(_) => ErrorCode::Configuration,
            Self::Authentication(_) => ErrorCode::Authentication,
            Self::Provider(_) => ErrorCode::Provider,
            Self::Billing(_) => ErrorCode::Billing,
            Self::Network(_) => ErrorCode::Network,
            Self::Internal(_) => ErrorCode::Internal,
        }
    }

    pub fn to_payload(&self) -> ErrorPayload {
        ErrorPayload {
            code: self.code(),
            message: self.to_string(),
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(error: reqwest::Error) -> Self {
        // Never leak URLs (they may carry credentials for custom endpoints).
        let error = error.without_url();
        if error.is_timeout() {
            Self::Network("the request timed out".into())
        } else if error.is_connect() {
            Self::Network("could not connect to the server".into())
        } else if error.is_decode() {
            Self::Provider(format!("unexpected response from provider: {error}"))
        } else {
            Self::Network(error.to_string())
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_payload().serialize(serializer)
    }
}

/// Commands return `AppError`; the generated TypeScript sees its wire format.
impl Type for AppError {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        ErrorPayload::definition(types)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn to_json(error: AppError) -> serde_json::Value {
        serde_json::to_value(&error).unwrap()
    }

    #[test]
    fn serializes_validation() {
        assert_eq!(
            to_json(AppError::validation("Title is required")),
            json!({ "code": "validation", "message": "Title is required" })
        );
    }

    #[test]
    fn serializes_not_found() {
        assert_eq!(
            to_json(AppError::not_found("Item 42 not found")),
            json!({ "code": "not_found", "message": "Item 42 not found" })
        );
    }

    #[test]
    fn converts_and_serializes_io_errors() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let error: AppError = io.into();

        assert_eq!(
            to_json(error),
            json!({ "code": "io", "message": "I/O error: access denied" })
        );
    }

    #[test]
    fn serializes_database() {
        assert_eq!(
            to_json(AppError::database("table is locked")),
            json!({ "code": "database", "message": "Database error: table is locked" })
        );
    }

    #[test]
    fn serializes_configuration() {
        assert_eq!(
            to_json(AppError::configuration("missing data directory")),
            json!({ "code": "configuration", "message": "Configuration error: missing data directory" })
        );
    }

    #[test]
    fn serializes_authentication_provider_and_network() {
        assert_eq!(
            to_json(AppError::authentication("OpenAI rejected the API key")),
            json!({ "code": "authentication", "message": "OpenAI rejected the API key" })
        );
        assert_eq!(
            to_json(AppError::provider("rate limited")),
            json!({ "code": "provider", "message": "rate limited" })
        );
        assert_eq!(
            to_json(AppError::network("offline")),
            json!({ "code": "network", "message": "Network error: offline" })
        );
    }

    #[test]
    fn converts_database_errors() {
        let error: AppError = rusqlite::Error::InvalidQuery.into();
        assert_eq!(error.code(), ErrorCode::Database);
    }

    #[test]
    fn serializes_internal() {
        assert_eq!(
            to_json(AppError::internal("something broke")),
            json!({ "code": "internal", "message": "something broke" })
        );
    }
}
