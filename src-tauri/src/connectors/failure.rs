//! Why a sign-in or a connection check failed, in the categories of Spec B
//! §60, each with a message that says what to do next. Provider error texts
//! appear only in `detail` (for "Show details"), scrubbed of every value
//! ReMa sent and cut short; codes, tokens and verifiers never appear.

use crate::{
    db::connectors::ReauthCause,
    error::AppError,
    models::connectors::{ConnectorErrorCode, ProviderId},
};

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
    let mut reported = match error {
        Some(e) => detail(e, description),
        None => format!("HTTP {status}"),
    };
    // Google's `invalid_grant` does not say why; the details say so too.
    if phase == TokenPhase::Refresh && code == "invalid_grant" && provider == "Google" {
        reported.push_str(GOOGLE_TESTING_HINT);
    }
    failure.with_detail(reported)
}

/// What Google's `invalid_grant` can mean (for the details).
pub const GOOGLE_TESTING_HINT: &str = " (Google does not say why: a revoked grant, a changed \
password, or an app in Testing, whose sign-ins Google ends about 7 days after they are made.)";

/// About how long a sign-in to a Google app in Testing lasts: Google says
/// "7 days", without the exact moment, so any date computed from this is
/// an estimate.
pub const GOOGLE_TESTING_GRANT_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Why a provider ended a connection, from what it said when a renewal was
/// refused (`reason`: its error code and text, or ReMa's own note). Only
/// the provider's words count: Google's `invalid_grant` is one answer for
/// several causes (a revoked grant, a changed password, an app in Testing,
/// too many refresh tokens, months unused) and never says which, so it is
/// recorded as [`ReauthCause::Revoked`] and explained as such.
pub fn reauth_cause(provider: ProviderId, reason: &str) -> Option<ReauthCause> {
    let r = reason.to_ascii_lowercase();
    let any = |codes: &[&str]| codes.iter().any(|c| r.contains(c));
    match provider {
        ProviderId::Microsoft => {
            if any(&["aadsts700082", "aadsts70008", "expired due to inactivity"]) {
                Some(ReauthCause::Inactive)
            } else if any(&["aadsts50057", "aadsts50053", "aadsts50034", "aadsts50064"]) {
                Some(ReauthCause::AccountBlocked)
            } else if any(&["aadsts65001"]) {
                // The user or an administrator has not consented (or the
                // consent was withdrawn): its own cause, not a security
                // policy and not a revoked session.
                Some(ReauthCause::ConsentRequired)
            } else if any(&[
                "aadsts50076",
                "aadsts50079",
                "aadsts50078",
                "aadsts50072",
                "aadsts50158",
                "aadsts53003",
                "aadsts50055",
                "aadsts530003",
                // Conditional Access sign-in frequency: the token expired
                // because a policy limits how long a sign-in lasts.
                "aadsts70043",
            ]) {
                Some(ReauthCause::SecurityPolicy)
            } else if any(&["aadsts50133"]) {
                // The session was invalidated (password change or reset, a
                // sign-out everywhere): sign in again.
                Some(ReauthCause::SessionEnded)
            } else if any(&["aadsts50173", "revoked"]) {
                Some(ReauthCause::Revoked)
            } else {
                None
            }
        }
        ProviderId::Google => {
            if any(&[
                "account has been deleted",
                "account disabled",
                "account_disabled",
            ]) {
                Some(ReauthCause::AccountBlocked)
            } else if r.contains("invalid_grant") {
                Some(ReauthCause::Revoked)
            } else {
                None
            }
        }
        ProviderId::Linkedin => Some(ReauthCause::TokenLifetime),
        ProviderId::Xing => None,
    }
}

/// What a connector card says when its connection must be renewed. It says
/// what the provider said and no more; when this build says its Google app
/// is in Testing (`google_testing`), it adds that Google ends such sign-ins
/// about 7 days after they are made, a setting of the Google app and not a
/// fault in ReMa or in the user's account.
pub fn reauth_message(
    provider: ProviderId,
    cause: Option<ReauthCause>,
    google_testing: Option<bool>,
) -> String {
    let name = provider.name();
    match cause {
        // `GoogleTesting` was recorded by earlier versions that inferred it
        // from the grant's age; it is read like any Google `invalid_grant`.
        Some(ReauthCause::Revoked | ReauthCause::GoogleTesting)
            if provider == ProviderId::Google =>
        {
            let mut message = "Google ended this sign-in; reconnect to continue. Google does \
                               not say why: it can be a grant you revoked (Google Account → \
                               Security → Your connections to third-party apps & services), a \
                               changed password, or a Google app in Testing."
                .to_string();
            if google_testing == Some(true) {
                message.push_str(
                    " ReMa's Google app is in Testing, where Google ends every sign-in about 7 \
                     days after it is made. That is a setting of the Google app, not a fault in \
                     ReMa or your account; the app's owner ends the weekly sign-outs by \
                     publishing it (Google Auth Platform → Audience → Publish app).",
                );
            }
            message
        }
        Some(ReauthCause::Revoked) => format!(
            "{name} ended this connection: its sign-ins were revoked or ReMa's access was \
             removed. Reconnect to continue."
        ),
        Some(ReauthCause::SessionEnded) => format!(
            "{name} signed this session out (after a password change or reset, or a sign-out \
             from every device). Sign in again to continue."
        ),
        Some(ReauthCause::ConsentRequired) => format!(
            "{name} needs your consent again before ReMa can use this account: it was not \
             given, or it was withdrawn (your organization may require an administrator to \
             give it). Reconnect and allow access on {name}'s screen."
        ),
        Some(ReauthCause::Inactive) => format!(
            "{name} ended this connection because it was not used for a while (ReMa renews it \
             daily while ReMa runs). Reconnect to continue."
        ),
        Some(ReauthCause::SecurityPolicy) => format!(
            "Your organization's security policy asks you to sign in to {name} again (for \
             example for multi-factor authentication, or because it limits how long a sign-in \
             lasts). Reconnect to continue."
        ),
        Some(ReauthCause::AccountBlocked) => format!(
            "This {name} account is disabled, locked or no longer exists, so ReMa cannot use \
             it. Reconnect with an account that works."
        ),
        Some(ReauthCause::TokenLifetime) => format!(
            "{name} access lasts a limited time and cannot be renewed without you. Reconnect to \
             continue."
        ),
        Some(ReauthCause::GoogleTesting) | None => {
            format!("Reconnect required: {name} access was revoked or has expired.")
        }
    }
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
        // The details say Google does not name the cause, and what it can be.
        let detail = refresh.detail.as_deref().unwrap();
        assert!(detail.contains("Google does not say why"), "{detail}");
        assert!(detail.contains("app in Testing"), "{detail}");
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
    fn a_refused_renewal_is_traced_to_its_cause() {
        use ReauthCause::*;
        let google = "invalid_grant: Token has been expired or revoked.";
        let cases: &[(ProviderId, &str, Option<ReauthCause>)] = &[
            // Google never says why: no cause is invented from the grant's age.
            (ProviderId::Google, google, Some(Revoked)),
            (ProviderId::Google, "invalid_grant: Bad Request", Some(Revoked)),
            (ProviderId::Google, "invalid_grant: Account has been deleted", Some(AccountBlocked)),
            (ProviderId::Google, "HTTP 503", None),
            (ProviderId::Microsoft, "invalid_grant: AADSTS700082: The refresh token has expired due to inactivity.", Some(Inactive)),
            (ProviderId::Microsoft, "invalid_grant: AADSTS70008: The provided authorization code or refresh token has expired due to inactivity.", Some(Inactive)),
            (ProviderId::Microsoft, "invalid_grant: AADSTS50173: The provided grant has expired due to it being revoked.", Some(Revoked)),
            (ProviderId::Microsoft, "invalid_grant: AADSTS50133: Session in the token has been invalidated. Sign in again.", Some(SessionEnded)),
            (ProviderId::Microsoft, "invalid_grant: AADSTS65001: The user or administrator has not consented to use the application.", Some(ConsentRequired)),
            (ProviderId::Microsoft, "interaction_required: AADSTS50076: Due to a configuration change made by your administrator, you must use multi-factor authentication.", Some(SecurityPolicy)),
            (ProviderId::Microsoft, "invalid_grant: AADSTS70043: The refresh token has expired due to a sign-in frequency policy.", Some(SecurityPolicy)),
            (ProviderId::Microsoft, "invalid_grant: AADSTS50057: The user account is disabled.", Some(AccountBlocked)),
            (ProviderId::Microsoft, "invalid_grant: something else", None),
            (ProviderId::Linkedin, "LinkedIn rejected the access token", Some(TokenLifetime)),
        ];
        for (provider, reason, cause) in cases {
            assert_eq!(
                reauth_cause(*provider, reason),
                *cause,
                "{provider:?} {reason}"
            );
        }
        // Every cause has its own words; only the unknown one is generic.
        let mut seen = std::collections::HashSet::new();
        for cause in [
            Revoked,
            SessionEnded,
            ConsentRequired,
            Inactive,
            SecurityPolicy,
            AccountBlocked,
            TokenLifetime,
        ] {
            let text = reauth_message(ProviderId::Microsoft, Some(cause), None);
            assert!(!text.starts_with("Reconnect required"), "{text}");
            assert!(seen.insert(text));
        }
        assert!(reauth_message(ProviderId::Google, None, None).starts_with("Reconnect required"));
        // No exact lifetime is promised for an unused Microsoft grant.
        let inactive = reauth_message(ProviderId::Microsoft, Some(Inactive), None);
        assert!(inactive.contains("not used for a while"), "{inactive}");
        assert!(!inactive.contains("90"), "{inactive}");
        assert!(
            reauth_message(ProviderId::Microsoft, Some(SessionEnded), None)
                .contains("Sign in again")
        );
        assert!(
            reauth_message(ProviderId::Microsoft, Some(ConsentRequired), None).contains("consent")
        );
        // Google: what it said, plus the Testing context only when the build
        // says the app is in Testing (and then as "about 7 days").
        for testing in [None, Some(false)] {
            let text = reauth_message(ProviderId::Google, Some(Revoked), testing);
            assert!(text.starts_with("Google ended this sign-in"), "{text}");
            assert!(text.contains("Google does not say why"), "{text}");
            assert!(!text.contains("ReMa's Google app is in Testing"), "{text}");
        }
        let text = reauth_message(ProviderId::Google, Some(Revoked), Some(true));
        assert!(text.contains("Google does not say why"), "{text}");
        assert!(text.contains("about 7 days"), "{text}");
        assert!(
            text.contains("not a fault in ReMa or your account"),
            "{text}"
        );
        // A cause recorded by an earlier version reads the same way.
        assert_eq!(
            reauth_message(ProviderId::Google, Some(GoogleTesting), Some(true)),
            text
        );
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
