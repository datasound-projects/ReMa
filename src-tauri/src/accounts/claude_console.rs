//! Anthropic through a Claude Console account: the official Anthropic CLI.
//!
//! `ant auth login` opens the Claude Console sign-in in the browser and
//! receives the result on a local callback; `ant` stores the token and
//! refreshes it. For each batch of requests ReMa asks for a short-lived
//! access token (`ant auth print-credentials --access-token`, documented for
//! handing the credential to another program) and calls the Anthropic API
//! directly. The token stays in Rust memory for at most a minute and is
//! never stored by ReMa or shown to the UI.
//!
//! This is a Claude Console (API) account, billed like an API key. Claude.ai
//! Free/Pro/Max sign-in is not offered: Anthropic does not permit
//! third-party apps to offer Claude.ai login or to route requests through
//! those plans (https://code.claude.com/docs/en/legal-and-compliance).
//!
//! The CLI uses a ReMa-owned configuration folder (`ANTHROPIC_CONFIG_DIR`),
//! so it neither reads nor changes the user's own `ant` profiles.

use std::{
    path::PathBuf,
    process::Stdio,
    sync::Mutex,
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tokio_util::sync::CancellationToken;

use super::{locate, AccountRuntime, RuntimeStatus, SignInAttempt, SignInOutcome};
use crate::{
    error::{AppError, AppResult},
    llm::BoxFuture,
    secrets::Credential,
};

const OVERRIDE_VAR: &str = "REMA_ANT_PATH";
const INSTALL_HINT: &str = "Claude Console sign-in uses Anthropic’s command-line tool. ReMa ships it; if it is missing, reinstall ReMa or install it with “brew install anthropics/tap/ant”, then try again.";
/// How long ReMa reuses an access token. `ant` refreshes tokens that expire
/// within two minutes, so a reused token is always still valid.
const TOKEN_REUSE: Duration = Duration::from_secs(60);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const SIGN_IN_TIMEOUT: &str = "10m";

pub struct ClaudeConsole {
    config_dir: PathBuf,
    token: Mutex<Option<(String, Instant)>>,
}

struct Output {
    success: bool,
    stdout: String,
    stderr: String,
}

impl ClaudeConsole {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            token: Mutex::default(),
        }
    }

    async fn locate(&self) -> Option<locate::Located> {
        locate::find("ant", OVERRIDE_VAR)
            .await
            .map(|(located, _)| located)
    }

    fn command(&self, located: &locate::Located, args: &[&str]) -> Command {
        let mut command = Command::new(&located.path);
        command
            .args(args)
            .env("ANTHROPIC_CONFIG_DIR", &self.config_dir)
            .env("PATH", located.search_path())
            // Only the Console sign-in: never another credential source.
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .env_remove("ANTHROPIC_PROFILE")
            .env_remove("ANTHROPIC_BASE_URL")
            .env_remove("ANTHROPIC_IDENTITY_TOKEN")
            .env_remove("ANTHROPIC_IDENTITY_TOKEN_FILE")
            .env_remove("ANTHROPIC_FEDERATION_RULE_ID")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command
    }

    async fn run(&self, located: &locate::Located, args: &[&str]) -> AppResult<Output> {
        let _ = std::fs::create_dir_all(&self.config_dir);
        let output = tokio::time::timeout(COMMAND_TIMEOUT, self.command(located, args).output())
            .await
            .map_err(|_| AppError::provider("The Anthropic CLI did not respond"))?
            .map_err(|e| AppError::provider(format!("could not run the Anthropic CLI: {e}")))?;
        Ok(Output {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn cached_token(&self) -> Option<String> {
        self.token
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(_, at)| at.elapsed() < TOKEN_REUSE)
            .map(|(token, _)| token.clone())
    }

    fn forget_token(&self) {
        *self.token.lock().unwrap() = None;
    }

    /// Asks `ant` for the current access token (refreshed if needed).
    async fn fetch_token(&self, located: &locate::Located) -> AppResult<TokenCheck> {
        let output = self
            .run(located, &["auth", "print-credentials", "--access-token"])
            .await?;
        let check = classify_token_output(&output);
        if let TokenCheck::Valid(token) = &check {
            *self.token.lock().unwrap() = Some((token.clone(), Instant::now()));
        }
        Ok(check)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TokenCheck {
    Valid(String),
    Expired,
    SignedOut,
    Failed(String),
}

fn classify_token_output(output: &Output) -> TokenCheck {
    let stderr = output.stderr.to_lowercase();
    if !output.success {
        return if stderr.contains("not logged in") || stderr.contains("no such file") {
            TokenCheck::SignedOut
        } else {
            TokenCheck::Failed(last_line(&output.stderr).unwrap_or_default())
        };
    }
    // `ant` still prints a token it could not refresh; an expired one is useless.
    if stderr.contains("expired at") {
        return TokenCheck::Expired;
    }
    match output.stdout.trim() {
        token if !token.is_empty() && !token.contains(char::is_whitespace) => {
            TokenCheck::Valid(token.to_string())
        }
        _ => TokenCheck::SignedOut,
    }
}

/// "ana@example.com · Acme" from the `Logged in to <org> as <email>` line of
/// `ant auth status`.
fn status_label(status: &str) -> Option<String> {
    let line = status
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("Logged in "))?;
    let (org, email) = match line.strip_prefix("to ") {
        Some(rest) => match rest.rsplit_once(" as ") {
            Some((org, email)) => (Some(org), Some(email)),
            None => (Some(rest), None),
        },
        None => (None, line.strip_prefix("as ")),
    };
    match (email.map(str::trim), org.map(str::trim)) {
        (Some(email), Some(org)) => Some(format!("{email} · {org}")),
        (Some(email), None) => Some(email.to_string()),
        (None, Some(org)) => Some(org.to_string()),
        (None, None) => None,
    }
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(|l| l.chars().take(300).collect())
}

/// Why `ant auth login` ended without success, from its output.
fn login_failure(output: &str) -> SignInOutcome {
    let lower = output.to_lowercase();
    if lower.contains("access_denied") || lower.contains("denied") || lower.contains("cancel") {
        SignInOutcome::Cancelled
    } else if lower.contains("timed out") {
        SignInOutcome::Failed("The sign-in timed out. Try again.".into())
    } else if lower.contains("couldn't launch browser")
        || lower.contains("could not launch browser")
    {
        SignInOutcome::Failed(
            "The Anthropic CLI could not open your browser. Check your default browser and try again."
                .into(),
        )
    } else {
        SignInOutcome::Failed(match last_line(output) {
            Some(line) => format!("The sign-in failed: {line}"),
            None => "The sign-in did not complete.".into(),
        })
    }
}

impl AccountRuntime for ClaudeConsole {
    fn status(&self) -> BoxFuture<'_, AppResult<RuntimeStatus>> {
        Box::pin(async move {
            let Some(located) = self.locate().await else {
                return Ok(RuntimeStatus::NotInstalled(INSTALL_HINT.into()));
            };
            match self.fetch_token(&located).await? {
                TokenCheck::SignedOut => Ok(RuntimeStatus::SignedOut),
                TokenCheck::Expired => Ok(RuntimeStatus::Expired),
                TokenCheck::Failed(detail) => Err(AppError::provider(format!(
                    "The Anthropic CLI failed: {detail}"
                ))),
                TokenCheck::Valid(_) => {
                    let status = self.run(&located, &["auth", "status"]).await?;
                    Ok(RuntimeStatus::SignedIn {
                        label: status_label(&status.stdout),
                    })
                }
            }
        })
    }

    fn start_sign_in(&self, _device_code: bool) -> BoxFuture<'_, AppResult<SignInAttempt>> {
        Box::pin(async move {
            let located = self
                .locate()
                .await
                .ok_or_else(|| AppError::configuration(INSTALL_HINT))?;
            self.forget_token();
            let _ = std::fs::create_dir_all(&self.config_dir);
            // `ant` opens the browser itself and waits for the local
            // callback. Its standard input stays open (and unused) so it
            // never falls back to reading a pasted code.
            let mut command =
                self.command(&located, &["auth", "login", "--timeout", SIGN_IN_TIMEOUT]);
            command.stdin(Stdio::piped());
            let mut child = command
                .spawn()
                .map_err(|e| AppError::provider(format!("could not run the Anthropic CLI: {e}")))?;
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            let stdin = child.stdin.take();

            let cancel = CancellationToken::new();
            let cancelled = cancel.clone();
            let outcome = Box::pin(async move {
                let _stdin = stdin;
                // Collect output; stop early if the browser cannot open
                // (`ant` would otherwise wait for a pasted code).
                let output = std::sync::Arc::new(Mutex::new(String::new()));
                let no_browser = CancellationToken::new();
                for stream in [
                    stdout.map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
                    stderr.map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
                ]
                .into_iter()
                .flatten()
                {
                    let output = output.clone();
                    let no_browser = no_browser.clone();
                    tauri::async_runtime::spawn(async move {
                        let mut lines = BufReader::new(stream).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            if line.contains("couldn't launch browser") {
                                no_browser.cancel();
                            }
                            let mut text = output.lock().unwrap();
                            text.push_str(&line);
                            text.push('\n');
                        }
                    });
                }
                let status = tokio::select! {
                    _ = cancelled.cancelled() => {
                        let _ = child.start_kill();
                        return SignInOutcome::Cancelled;
                    }
                    _ = no_browser.cancelled() => {
                        let _ = child.start_kill();
                        return login_failure("couldn't launch browser");
                    }
                    status = child.wait() => status,
                };
                // Let the readers take the last lines.
                tokio::time::sleep(Duration::from_millis(50)).await;
                let text = output.lock().unwrap().clone();
                match status {
                    Ok(status) if status.success() => SignInOutcome::Succeeded,
                    Ok(_) => login_failure(&text),
                    Err(e) => SignInOutcome::Failed(format!("The Anthropic CLI stopped: {e}")),
                }
            });
            Ok(SignInAttempt {
                // The CLI opens the page itself.
                open_url: None,
                user_code: None,
                outcome,
                cancel,
            })
        })
    }

    fn sign_out(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async move {
            self.forget_token();
            let located = self
                .locate()
                .await
                .ok_or_else(|| AppError::configuration(INSTALL_HINT))?;
            let output = self.run(&located, &["auth", "logout"]).await?;
            if output.success {
                Ok(())
            } else {
                Err(AppError::provider(format!(
                    "The Anthropic CLI could not sign out: {}",
                    last_line(&output.stderr).unwrap_or_default()
                )))
            }
        })
    }

    fn credential(&self) -> BoxFuture<'_, AppResult<Option<Credential>>> {
        Box::pin(async move {
            let token = match self.cached_token() {
                Some(token) => token,
                None => {
                    let located = self
                        .locate()
                        .await
                        .ok_or_else(|| AppError::configuration(INSTALL_HINT))?;
                    match self.fetch_token(&located).await? {
                        TokenCheck::Valid(token) => token,
                        TokenCheck::Expired => return Err(AppError::authentication(
                            "Your Claude Console sign-in has expired. Sign in again in Settings.",
                        )),
                        TokenCheck::SignedOut => return Err(AppError::authentication(
                            "You are signed out of the Claude Console. Sign in again in Settings.",
                        )),
                        TokenCheck::Failed(detail) => {
                            return Err(AppError::provider(format!(
                                "The Anthropic CLI failed: {detail}"
                            )))
                        }
                    }
                }
            };
            Ok(Some(Credential::OAuth {
                access_token: token,
                refresh_token: None,
                expires_at: None,
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(success: bool, stdout: &str, stderr: &str) -> Output {
        Output {
            success,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn classifies_token_requests() {
        assert_eq!(
            classify_token_output(&output(true, "sk-ant-oat-123\n", "")),
            TokenCheck::Valid("sk-ant-oat-123".into())
        );
        assert_eq!(
            classify_token_output(&output(
                false,
                "",
                "not logged in (profile \"default\"): failed to read config file"
            )),
            TokenCheck::SignedOut
        );
        assert_eq!(
            classify_token_output(&output(
                true,
                "old-token\n",
                "warning: token for profile \"default\" expired at 2026-09-25T10:00:00Z and refresh failed: 400"
            )),
            TokenCheck::Expired
        );
        // Refresh failed but the token is still valid for a while.
        assert!(matches!(
            classify_token_output(&output(
                true,
                "tok\n",
                "warning: token for profile \"default\" expires at 2026-09-25T10:00:00Z and refresh failed: 503"
            )),
            TokenCheck::Valid(_)
        ));
    }

    #[test]
    fn reads_the_account_from_auth_status() {
        let status = "Active profile:  default\n\nCredentials\n  Logged in to Acme Corp as ana@example.com\n";
        assert_eq!(
            status_label(status).as_deref(),
            Some("ana@example.com · Acme Corp")
        );
        assert_eq!(
            status_label("  Logged in as ana@example.com").as_deref(),
            Some("ana@example.com")
        );
        assert_eq!(
            status_label("Credentials\n  (profile \"default\" not configured)"),
            None
        );
    }

    #[test]
    fn explains_failed_sign_ins() {
        assert!(matches!(
            login_failure("Waiting for authentication...\nCode: timed out waiting for authorization"),
            SignInOutcome::Failed(m) if m.contains("timed out")
        ));
        assert_eq!(
            login_failure("error: access_denied"),
            SignInOutcome::Cancelled
        );
        assert!(matches!(
            login_failure("(couldn't launch browser automatically: exec: \"xdg-open\")"),
            SignInOutcome::Failed(m) if m.contains("open your browser")
        ));
    }
}
