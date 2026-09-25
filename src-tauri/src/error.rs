use serde::{ser::SerializeStruct, Serialize, Serializer};

/// Result type used by services and commands.
pub type AppResult<T> = Result<T, AppError>;

/// Errors that can cross the IPC boundary.
///
/// Serialized to the frontend as `{ "code": "...", "message": "..." }` so the
/// UI can branch on a stable `code` instead of parsing messages.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    /// Stable, machine-readable identifier for the error kind.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Internal(_) => "internal",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut error = serializer.serialize_struct("AppError", 2)?;
        error.serialize_field("code", self.code())?;
        error.serialize_field("message", &self.to_string())?;
        error.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_code_and_message() {
        let error = AppError::Internal("something broke".into());
        let json = serde_json::to_value(&error).unwrap();

        assert_eq!(
            json,
            serde_json::json!({ "code": "internal", "message": "something broke" })
        );
    }
}
