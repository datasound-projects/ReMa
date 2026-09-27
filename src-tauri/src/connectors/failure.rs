//! Why a sign-in or a connection check failed, in the categories of Spec B
//! §60, each with a message that says what to do next. Provider error texts
//! appear only in `detail` (for "Show details"), scrubbed of every value
//! ReMa sent and cut short; codes, tokens and verifiers never appear.

use crate::{error::AppError, models::connectors::ConnectorErrorCode};

/// A failed sign-in or connection check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: ConnectorErrorCode,
    pub message: String,
    /// The provider's own error code and text (no secrets).
    pub detail: Option<String>,
}

impl Failure {
    pub fn new(code: ConnectorErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail: String = detail.into();
        self.detail = Some(detail.chars().take(300).collect());
        self
    }

    pub fn into_error(self) -> AppError {
        use ConnectorErrorCode::*;
        match self.code {
            UserCancelled | SignInTimedOut | BrowserUnavailable => {
                AppError::validation(self.message)
            }
            RedirectMismatch | ApiNotEnabled | ProviderConfigurationError => {
                AppError::configuration(self.message)
            }
            ScopeNotGranted | ProviderAdminPolicy | AccountNotSupported | OauthAppNotVerified => {
                AppError::permission(self.message)
            }
            NetworkError => AppError::network(self.message),
            CredentialStoreUnavailable => AppError::configuration(self.message),
            InvalidState | TokenExchangeFailed | ReauthRequired => {
                AppError::authentication(self.message)
            }
        }
    }
}

pub fn cancelled(provider: &str) -> Failure {
    Failure::new(
        ConnectorErrorCode::UserCancelled,
        format!("The {provider} sign-in was cancelled. Nothing was connected."),
    )
}

pub fn timed_out(provider: &str) -> Failure {
    Failure::new(
        ConnectorErrorCode::SignInTimedOut,
        format!(
            "The {provider} sign-in was not finished in the browser within 5 minutes. Click Retry \
             to open it again."
        ),
    )
}

pub fn invalid_state(provider: &str) -> Failure {
    Failure::new(
        ConnectorErrorCode::InvalidState,
        format!(
            "The browser returned a {provider} sign-in that ReMa did not start (perhaps from an \
             old tab). Close old sign-in tabs and click Retry."
        ),
    )
}

pub fn network(provider: &str) -> Failure {
    Failure::new(
        ConnectorErrorCode::NetworkError,
        format!("{provider} could not be reached. Check your internet connection and try again."),
    )
}

pub fn browser_unavailable(reason: &str) -> Failure {
    Failure::new(
        ConnectorErrorCode::BrowserUnavailable,
        "ReMa could not open your default browser for the sign-in.",
    )
    .with_detail(reason)
}

/// Microsoft's organization-policy refusals: consent needs an administrator
/// (AADSTS90094, AADSTS90095, or AADSTS65001 naming an administrator).
fn admin_policy(description: &str) -> bool {
    let d = description.to_ascii_lowercase();
    d.contains("aadsts90094")
        || d.contains("aadsts90095")
        || (d.contains("aadsts65001") && d.contains("admin"))
        || d.contains("need admin approval")
}

fn admin_policy_failure(provider: &str) -> Failure {
    let message = if provider == "Microsoft" {
        "Your organization requires administrator approval before ReMa can access this \
         Microsoft account. Ask your IT administrator to approve ReMa, or connect a personal \
         Microsoft account."
            .to_string()
    } else {
        format!(
            "Your organization's {provider} administrator does not allow ReMa to access this \
             account. Ask your administrator to allow ReMa, or use another account."
        )
    };
    Failure::new(ConnectorErrorCode::ProviderAdminPolicy, message)
}

fn configuration(provider: &str) -> Failure {
    Failure::new(
        ConnectorErrorCode::ProviderConfigurationError,
        format!(
            "{provider} rejected this copy of ReMa's app registration. This is not a problem \
             with your account; updating ReMa fixes it."
        ),
    )
}

fn redirect_mismatch(provider: &str) -> Failure {
    Failure::new(
        ConnectorErrorCode::RedirectMismatch,
        format!(
            "{provider} rejected the address ReMa listens on for the sign-in. This copy of \
             ReMa's app registration needs an update."
        ),
    )
}

fn detail(error: &str, description: Option<&str>) -> String {
    match description {
        Some(d) if !d.is_empty() => format!("{error}: {d}"),
        _ => error.to_string(),
    }
}

/// An `error` the provider sent back to the loopback redirect instead of a
/// code: the user declined, a policy stopped the sign-in, or ReMa's
/// registration is wrong.
pub fn from_redirect(provider: &str, error: &str, description: Option<&str>) -> Failure {
    let text = description.unwrap_or_default();
    let lower = text.to_ascii_lowercase();
    let failure = if admin_policy(text) || error == "admin_policy_enforced" {
        admin_policy_failure(provider)
    } else if lower.contains("aadsts50011") || error == "redirect_uri_mismatch" {
        redirect_mismatch(provider)
    } else if lower.contains("verification") || lower.contains("not verified") {
        not_verified(provider)
    } else {
        match error {
            "access_denied" | "consent_required" | "interaction_required" => Failure::new(
                ConnectorErrorCode::UserCancelled,
                format!(
                    "{provider} access was not granted, so nothing was connected. Click Retry \
                     and allow access on {provider}'s screen."
                ),
            ),
            "invalid_client"
            | "unauthorized_client"
            | "invalid_request"
            | "org_internal"
            | "invalid_scope"
            | "unsupported_response_type" => configuration(provider),
            "temporarily_unavailable" | "server_error" => Failure::new(
                ConnectorErrorCode::TokenExchangeFailed,
                format!("{provider} could not complete the sign-in right now. Try again shortly."),
            ),
            _ => Failure::new(
                ConnectorErrorCode::TokenExchangeFailed,
                format!("{provider} did not complete the sign-in. Click Retry."),
            ),
        }
    };
    failure.with_detail(detail(error, description))
}

fn not_verified(provider: &str) -> Failure {
    let message = if cfg!(debug_assertions) {
        format!(
            "Development: {provider} has not verified ReMa's app for this account. While the \
             Google project is in Testing, only its test users can sign in (Google Auth \
             Platform → Audience)."
        )
    } else {
        format!("{provider} has not approved ReMa for this account yet.")
    };
    Failure::new(ConnectorErrorCode::OauthAppNotVerified, message)
}

/// Whether ReMa is exchanging a code (sign-in) or refreshing access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenPhase {
    Exchange,
    Refresh,
}

/// A token endpoint error (`status` and the OAuth `error` fields).
pub fn from_token_error(
    provider: &str,
    phase: TokenPhase,
    status: u16,
    error: Option<&str>,
    description: Option<&str>,
) -> Failure {
    let text = description.unwrap_or_default();
    let lower = text.to_ascii_lowercase();
    let code = error.unwrap_or_default();
    let failure = if admin_policy(text) {
        admin_policy_failure(provider)
    } else if code == "redirect_uri_mismatch" || lower.contains("aadsts50011") {
        redirect_mismatch(provider)
    } else if matches!(code, "invalid_client" | "unauthorized_client")
        || lower.contains("client_secret is missing")
        || lower.contains("aadsts7000218")
        || lower.contains("aadsts700016")
    {
        configuration(provider)
    } else if phase == TokenPhase::Refresh
        && matches!(
            code,
            "invalid_grant" | "interaction_required" | "consent_required" | "login_required"
        )
    {
        Failure::new(
            ConnectorErrorCode::ReauthRequired,
            format!(
                "{provider} access was revoked or has expired. Reconnect in Settings → Connectors."
            ),
        )
    } else if code == "invalid_grant" {
        Failure::new(
            ConnectorErrorCode::TokenExchangeFailed,
            format!(
                "{provider} did not accept the finished sign-in (it may have expired or been used \
                 already). Click Retry."
            ),
        )
    } else if status == 429 || status >= 500 {
        Failure::new(
            ConnectorErrorCode::NetworkError,
            format!("{provider} is temporarily unavailable. Try again in a minute."),
        )
    } else {
        Failure::new(
            ConnectorErrorCode::TokenExchangeFailed,
            format!("{provider} did not issue access to ReMa. Click Retry."),
        )
    };
    let reported = match error {
        Some(e) => detail(e, description),
        None => format!("HTTP {status}"),
    };
    failure.with_detail(reported)
}

/// What a provider API said to a connection check (`connector` is the
/// card's name, e.g. "Gmail").
pub fn from_api(provider: &str, connector: &str, status: u16, body: &str) -> Failure {
    let lower = body.to_ascii_lowercase();
    let failure = if status == 401 {
        Failure::new(
            ConnectorErrorCode::ReauthRequired,
            format!("{provider} did not accept ReMa's access. Reconnect {connector}."),
        )
    } else if lower.contains("accessnotconfigured")
        || lower.contains("service_disabled")
        || lower.contains("has not been used in project")
    {
        Failure::new(
            ConnectorErrorCode::ApiNotEnabled,
            format!(
                "The {connector} API is not enabled for this copy of ReMa, so {connector} cannot \
                 be used yet. This is not a problem with your account; updating ReMa fixes it."
            ),
        )
    } else if lower.contains("mailboxnotenabledforrestapi")
        || lower.contains("mail service not enabled")
        || lower.contains("mailboxnotfound")
        || lower.contains("the mailbox is either inactive")
    {
        Failure::new(
            ConnectorErrorCode::AccountNotSupported,
            format!("This {provider} account has no {connector} mailbox or calendar ReMa can use."),
        )
    } else if status == 403
        && (lower.contains("insufficient")
            || lower.contains("scope")
            || lower.contains("erroraccessdenied")
            || lower.contains("access is denied"))
    {
        Failure::new(
            ConnectorErrorCode::ScopeNotGranted,
            format!(
                "{provider} did not grant ReMa permission to use {connector}. Reconnect and allow \
                 access on {provider}'s screen."
            ),
        )
    } else if status == 403 && admin_policy(body) {
        admin_policy_failure(provider)
    } else if status == 403 {
        Failure::new(
            ConnectorErrorCode::ProviderAdminPolicy,
            format!(
                "{provider} refused access to {connector} for this account; an administrator's \
                 policy may block it."
            ),
        )
    } else if status == 429 || status >= 500 {
        network(provider)
    } else {
        Failure::new(
            ConnectorErrorCode::TokenExchangeFailed,
            format!("{connector} did not answer ReMa's check ({status}). Try reconnecting."),
        )
    };
    let reason = body.chars().take(200).collect::<String>();
    failure.with_detail(format!("HTTP {status} {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ConnectorErrorCode::*;

    #[test]
    fn redirect_errors_become_actionable_categories() {
        let cases = [
            ("access_denied", None, UserCancelled),
            (
                "access_denied",
                Some("AADSTS65004: User declined to consent to access the app."),
                UserCancelled,
            ),
            (
                "access_denied",
                Some("AADSTS90094: An administrator of Contoso has set a policy that prevents you from granting ReMa the permissions it is requesting."),
                ProviderAdminPolicy,
            ),
            ("admin_policy_enforced", None, ProviderAdminPolicy),
            ("invalid_request", Some("AADSTS50011: The redirect URI does not match."), RedirectMismatch),
            ("redirect_uri_mismatch", None, RedirectMismatch),
            ("invalid_client", None, ProviderConfigurationError),
            ("org_internal", None, ProviderConfigurationError),
            (
                "access_denied",
                Some("ReMa has not completed the Google verification process"),
                OauthAppNotVerified,
            ),
            ("server_error", None, TokenExchangeFailed),
        ];
        for (error, description, code) in cases {
            let failure = from_redirect("Microsoft", error, description);
            assert_eq!(failure.code, code, "{error} {description:?}");
            assert!(failure.detail.unwrap().starts_with(error));
        }
        let admin = from_redirect("Microsoft", "access_denied", Some("AADSTS90094: x"));
        assert!(admin
            .message
            .starts_with("Your organization requires administrator approval before ReMa can access this Microsoft account."));
    }

    #[test]
    fn token_errors_depend_on_the_phase() {
        let refresh = from_token_error(
            "Google",
            TokenPhase::Refresh,
            400,
            Some("invalid_grant"),
            Some("Token has been expired or revoked."),
        );
        assert_eq!(refresh.code, ReauthRequired);
        let exchange = from_token_error(
            "Google",
            TokenPhase::Exchange,
            400,
            Some("invalid_grant"),
            None,
        );
        assert_eq!(exchange.code, TokenExchangeFailed);
        for (status, error, description, code) in [
            (400, Some("invalid_request"), Some("client_secret is missing."), ProviderConfigurationError),
            (401, Some("invalid_client"), Some("Unauthorized"), ProviderConfigurationError),
            (400, Some("invalid_request"), Some("AADSTS7000218: The request body must contain client_assertion or client_secret."), ProviderConfigurationError),
            (400, Some("invalid_grant"), Some("AADSTS65001: The user or administrator has not consented; an admin must grant consent."), ProviderAdminPolicy),
            (503, None, None, NetworkError),
            (429, None, None, NetworkError),
        ] {
            let failure = from_token_error("Microsoft", TokenPhase::Exchange, status, error, description);
            assert_eq!(failure.code, code, "{status} {error:?} {description:?}");
        }
        // Temporary trouble during a refresh never asks the user to reconnect.
        assert_eq!(
            from_token_error("Google", TokenPhase::Refresh, 503, None, None).code,
            NetworkError
        );
    }

    #[test]
    fn api_answers_tell_permission_from_configuration() {
        let google_scope = r#"{"error":{"code":403,"message":"Request had insufficient authentication scopes.","status":"PERMISSION_DENIED","details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#;
        assert_eq!(
            from_api("Google", "Gmail", 403, google_scope).code,
            ScopeNotGranted
        );
        let disabled = r#"{"error":{"code":403,"message":"Gmail API has not been used in project 123 before or it is disabled.","status":"PERMISSION_DENIED","details":[{"reason":"SERVICE_DISABLED"}]}}"#;
        assert_eq!(
            from_api("Google", "Gmail", 403, disabled).code,
            ApiNotEnabled
        );
        let no_mailbox = r#"{"error":{"code":"MailboxNotEnabledForRESTAPI","message":"The mailbox is either inactive, soft-deleted, or is hosted on-premise."}}"#;
        assert_eq!(
            from_api("Microsoft", "Outlook Mail", 404, no_mailbox).code,
            AccountNotSupported
        );
        let denied = r#"{"error":{"code":"ErrorAccessDenied","message":"Access is denied. Check credentials and try again."}}"#;
        assert_eq!(
            from_api("Microsoft", "Outlook Mail", 403, denied).code,
            ScopeNotGranted
        );
        assert_eq!(from_api("Google", "Gmail", 401, "{}").code, ReauthRequired);
        assert_eq!(from_api("Google", "Gmail", 503, "").code, NetworkError);
    }

    #[test]
    fn messages_never_carry_what_was_sent() {
        let failure = from_token_error(
            "Google",
            TokenPhase::Exchange,
            400,
            Some("invalid_grant"),
            Some("Bad Request"),
        );
        assert!(
            !failure.message.contains("Bad Request"),
            "provider text stays in detail"
        );
        assert_eq!(
            failure.detail.as_deref(),
            Some("invalid_grant: Bad Request")
        );
        assert!(matches!(
            cancelled("Google").into_error(),
            AppError::Validation(_)
        ));
        assert!(matches!(
            network("Google").into_error(),
            AppError::Network(_)
        ));
    }
}
