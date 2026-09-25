//! OpenAI through a ChatGPT account: the official Codex runtime.
//!
//! ReMa runs `codex app-server` — the JSON-RPC interface that OpenAI's own
//! Codex SDKs (`openai-codex` for Python, `@openai/codex-sdk`) and the
//! Codex IDE extension are built on — as a private child process over
//! stdio. Codex performs the ChatGPT sign-in (browser or device code),
//! stores and refreshes the credentials, lists the models the account may
//! use and sends every model request. ReMa never sees a token.
//!
//! The runtime is private to ReMa: its own `CODEX_HOME` (the user's Codex
//! CLI configuration, sessions, MCP servers and skills are not loaded),
//! credentials in the OS keychain when available, no saved history, and
//! chat turns on ephemeral threads with every agent tool turned off
//! (shell, file edits, web search, apps, plugins, sub-agents), a read-only
//! sandbox in an empty folder and approvals always declined.

use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, oneshot},
};
use tokio_util::sync::CancellationToken;

use super::{locate, AccountRuntime, RuntimeStatus, SignInAttempt, SignInOutcome};
use crate::{
    error::{AppError, AppResult},
    llm::{BoxFuture, ChatRequest, DeltaSink, FetchedModel, Finish},
    models::chat::MessageRole,
};

/// Oldest Codex release whose app-server protocol ReMa uses (the minimum
/// runtime of the official Python SDK). Tested with 0.157.
const MIN_VERSION: [u64; 3] = [0, 151, 0];
const OVERRIDE_VAR: &str = "REMA_CODEX_PATH";
const INSTALL_HINT: &str = "ChatGPT sign-in uses OpenAI’s Codex app. Install it with “brew install --cask codex” or “npm install -g @openai/codex”, then try again.";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// A turn that reports nothing for this long is considered stuck.
const TURN_IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Instructions when a request has no system prompt: Codex's own base
/// instructions are for a coding agent.
const DEFAULT_INSTRUCTIONS: &str = "You are a helpful assistant.";

/// `-c key=value` settings for ReMa's private runtime. Unknown keys are
/// ignored by Codex, so these stay safe across versions.
const SETTINGS: &[&str] = &[
    // Credentials in the OS keychain when available, else in CODEX_HOME.
    r#"cli_auth_credentials_store="auto""#,
    r#"forced_login_method="chatgpt""#,
    r#"history.persistence="none""#,
    "check_for_update_on_startup=false",
    r#"web_search="disabled""#,
    r#"sandbox_mode="read-only""#,
    r#"approval_policy="never""#,
    "agents.enabled=false",
    "project_root_markers=[]",
    "include_environment_context=false",
    "include_apps_instructions=false",
    "include_permissions_instructions=false",
    "include_collaboration_mode_instructions=false",
    "skills.bundled.enabled=false",
    "skills.include_instructions=false",
    "tools.experimental_request_user_input.enabled=false",
];

/// Agent features turned off (as `-c features.<name>=false`).
const DISABLED_FEATURES: &[&str] = &[
    "shell_tool",
    "unified_exec",
    "shell_snapshot",
    "view_image",
    "image_generation",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "in_app_browser",
    "multi_agent",
    "multi_agent_v2",
    "code_mode",
    "code_mode_host",
    "apps",
    "plugins",
    "hooks",
    "goals",
    "memories",
    "skill_search",
    "tool_suggest",
    "workspace_dependencies",
    "sleep_tool",
    "realtime_conversation",
];

/// A notification from the app-server.
#[derive(Debug, Clone)]
pub struct Notification {
    pub method: String,
    pub params: Value,
}

enum Launch {
    Process {
        home: PathBuf,
        workdir: PathBuf,
    },
    #[cfg(test)]
    Test(Arc<dyn Fn() -> Arc<Connection> + Send + Sync>),
}

/// ReMa's private Codex runtime.
pub struct CodexRuntime {
    launch: Launch,
    client_version: String,
    workdir: PathBuf,
    current: Mutex<Option<Arc<Connection>>>,
    starting: tokio::sync::Mutex<()>,
}

/// Why the runtime could not start.
enum StartError {
    /// Missing or too old: the user has to install or update it.
    Unavailable(String),
    Failed(AppError),
}

impl From<StartError> for AppError {
    fn from(error: StartError) -> Self {
        match error {
            StartError::Unavailable(message) => AppError::configuration(message),
            StartError::Failed(error) => error,
        }
    }
}

impl From<AppError> for StartError {
    fn from(error: AppError) -> Self {
        Self::Failed(error)
    }
}

impl CodexRuntime {
    /// `home` holds the runtime's configuration and (without a keychain) its
    /// credentials; `workdir` is the empty folder turns run in.
    pub fn new(home: PathBuf, workdir: PathBuf, client_version: impl Into<String>) -> Self {
        Self {
            launch: Launch::Process {
                home,
                workdir: workdir.clone(),
            },
            client_version: client_version.into(),
            workdir,
            current: Mutex::default(),
            starting: tokio::sync::Mutex::default(),
        }
    }

    #[cfg(test)]
    fn with_connector(connect: impl Fn() -> Arc<Connection> + Send + Sync + 'static) -> Self {
        Self {
            launch: Launch::Test(Arc::new(connect)),
            client_version: "0.0.0".into(),
            workdir: std::env::temp_dir(),
            current: Mutex::default(),
            starting: tokio::sync::Mutex::default(),
        }
    }

    /// The running app-server, started on first use and after it exits.
    async fn connection(&self) -> Result<Arc<Connection>, StartError> {
        if let Some(conn) = self.live() {
            return Ok(conn);
        }
        let _starting = self.starting.lock().await;
        if let Some(conn) = self.live() {
            return Ok(conn);
        }
        let conn = match &self.launch {
            Launch::Process { home, workdir } => spawn_app_server(home, workdir).await?,
            #[cfg(test)]
            Launch::Test(connect) => connect(),
        };
        conn.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "rema",
                    "title": "ReMa",
                    "version": self.client_version,
                },
            }),
        )
        .await
        .map_err(|e| {
            StartError::Failed(AppError::provider(format!(
                "Codex did not start{}",
                conn.stderr_hint()
                    .map_or(format!(": {e}"), |h| format!(": {h}"))
            )))
        })?;
        conn.notify("initialized", None).await?;
        *self.current.lock().unwrap() = Some(conn.clone());
        Ok(conn)
    }

    fn live(&self) -> Option<Arc<Connection>> {
        self.current
            .lock()
            .unwrap()
            .clone()
            .filter(|c| !c.closed.is_cancelled())
    }

    /// The models this ChatGPT account may use, as the runtime lists them.
    pub async fn list_models(&self) -> AppResult<Vec<FetchedModel>> {
        let conn = self.connection().await?;
        let mut models = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..20 {
            let page = conn
                .request(
                    "model/list",
                    json!({ "cursor": cursor, "includeHidden": false }),
                )
                .await?;
            models.extend(
                page.get("data")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            cursor = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(parse_models(&models))
    }

    /// Runs one chat turn on a fresh ephemeral thread: ReMa's system prompt
    /// replaces Codex's instructions, earlier turns are added as history and
    /// the answer streams back.
    pub async fn stream_chat(
        &self,
        model_id: &str,
        request: &ChatRequest,
        cancel: CancellationToken,
        on_delta: DeltaSink<'_>,
    ) -> AppResult<Finish> {
        let conn = self.connection().await?;
        let (history, prompt) = split_turns(request);
        let instructions = request
            .system
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_INSTRUCTIONS);
        let started = conn
            .request(
                "thread/start",
                json!({
                    "ephemeral": true,
                    "baseInstructions": instructions,
                    "cwd": self.workdir,
                    "sandbox": "read-only",
                    "approvalPolicy": "never",
                    "model": model_id,
                }),
            )
            .await?;
        let thread_id = started
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::provider("Codex did not start a conversation"))?
            .to_string();
        let mut route = conn.subscribe(format!("thread:{thread_id}"));
        let result = run_turn(
            &conn, &mut route, &thread_id, history, &prompt, &cancel, on_delta,
        )
        .await;
        drop(route);
        // Release the thread; nothing is kept (it was never saved).
        let _ = conn
            .request("thread/unsubscribe", json!({ "threadId": thread_id }))
            .await;
        result
    }
}

async fn run_turn(
    conn: &Arc<Connection>,
    route: &mut Route,
    thread_id: &str,
    history: Vec<Value>,
    prompt: &str,
    cancel: &CancellationToken,
    on_delta: DeltaSink<'_>,
) -> AppResult<Finish> {
    if !history.is_empty() {
        conn.request(
            "thread/inject_items",
            json!({ "threadId": thread_id, "items": history }),
        )
        .await?;
    }
    let turn = conn
        .request(
            "turn/start",
            json!({
                "threadId": thread_id,
                "input": [{ "type": "text", "text": prompt }],
            }),
        )
        .await?;
    let turn_id = turn
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::provider("Codex did not start the turn"))?
        .to_string();

    let mut stream = TurnText::default();
    let mut last_error: Option<Value> = None;
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = conn
                    .request("turn/interrupt", json!({ "threadId": thread_id, "turnId": turn_id }))
                    .await;
                return Ok(Finish::Cancelled);
            }
            next = tokio::time::timeout(TURN_IDLE_TIMEOUT, route.recv()) => next,
        };
        let notification = match next {
            Err(_) => return Err(AppError::provider("Codex stopped responding")),
            Ok(None) => return Err(stopped()),
            Ok(Some(n)) => n,
        };
        let params = &notification.params;
        let for_turn = params.get("turnId").and_then(Value::as_str) == Some(turn_id.as_str());
        match notification.method.as_str() {
            "item/agentMessage/delta" if for_turn => {
                let item = params.get("itemId").and_then(Value::as_str).unwrap_or("");
                let delta = params.get("delta").and_then(Value::as_str).unwrap_or("");
                stream.push(item, delta, on_delta);
            }
            "item/completed" if for_turn => {
                let item = params.get("item").unwrap_or(&Value::Null);
                if item.get("type").and_then(Value::as_str) == Some("agentMessage") {
                    let id = item.get("id").and_then(Value::as_str).unwrap_or("");
                    let text = item.get("text").and_then(Value::as_str).unwrap_or("");
                    stream.complete(id, text, on_delta);
                }
            }
            "error" if for_turn => {
                if !params
                    .get("willRetry")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    last_error = params.get("error").cloned();
                }
            }
            "turn/completed" => {
                let turn = params.get("turn").unwrap_or(&Value::Null);
                if turn.get("id").and_then(Value::as_str) != Some(turn_id.as_str()) {
                    continue;
                }
                return match turn.get("status").and_then(Value::as_str) {
                    Some("completed") => Ok(Finish::Complete),
                    Some("interrupted") => Ok(Finish::Cancelled),
                    _ => Err(turn_error(
                        turn.get("error")
                            .filter(|e| !e.is_null())
                            .or(last_error.as_ref()),
                    )),
                };
            }
            _ => {}
        }
    }
}

/// Streams the text of the turn's agent messages; separate messages are
/// separated by a blank line.
#[derive(Default)]
struct TurnText {
    item: String,
    item_text: String,
    any: bool,
}

impl TurnText {
    fn push(&mut self, item: &str, delta: &str, on_delta: DeltaSink<'_>) {
        if delta.is_empty() {
            return;
        }
        if item != self.item {
            if self.any {
                on_delta("\n\n");
            }
            self.item = item.to_string();
            self.item_text.clear();
        }
        self.item_text.push_str(delta);
        self.any = true;
        on_delta(delta);
    }

    /// The final text of a message: sends whatever the deltas missed.
    fn complete(&mut self, item: &str, text: &str, on_delta: DeltaSink<'_>) {
        if item == self.item {
            if let Some(rest) = text.strip_prefix(self.item_text.as_str()) {
                if !rest.is_empty() {
                    on_delta(rest);
                    self.item_text.push_str(rest);
                }
            }
        } else if !text.is_empty() {
            self.push(item, text, on_delta);
        }
    }
}

fn stopped() -> AppError {
    AppError::provider("Codex stopped unexpectedly. Try again.")
}

/// Maps a Codex turn error to a message the user can act on.
fn turn_error(error: Option<&Value>) -> AppError {
    let message = error
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("the request failed")
        .trim()
        .to_string();
    let info = error.and_then(|e| e.get("codexErrorInfo"));
    let code = info.and_then(|i| {
        i.as_str()
            .map(str::to_string)
            .or_else(|| i.as_object().and_then(|o| o.keys().next().cloned()))
    });
    match code.as_deref() {
        Some("unauthorized") => AppError::authentication(
            "Your ChatGPT sign-in has expired or was revoked. Sign in again in Settings.",
        ),
        Some("usageLimitExceeded") => AppError::provider(format!(
            "Your ChatGPT plan’s usage limit is reached: {message}"
        )),
        Some("rateLimitExceeded") => {
            AppError::provider("ChatGPT’s rate limit is reached. Try again shortly.")
        }
        Some("contextWindowExceeded") => {
            AppError::provider("The conversation is too long for this model.")
        }
        Some("serverOverloaded" | "internalServerError") => {
            AppError::provider(format!("OpenAI is unavailable right now: {message}"))
        }
        Some(
            "httpConnectionFailed"
            | "responseStreamConnectionFailed"
            | "responseStreamDisconnected"
            | "responseTooManyFailedAttempts",
        ) => AppError::network(format!("could not reach OpenAI ({message})")),
        _ => AppError::provider(format!("ChatGPT: {message}")),
    }
}

/// Earlier turns as Responses-API history items, and the prompt to answer.
fn split_turns(request: &ChatRequest) -> (Vec<Value>, String) {
    let mut turns = request.normalized_turns();
    let prompt = match turns.last() {
        Some(last) if last.role == MessageRole::User => turns.pop().map(|t| t.content),
        _ => None,
    }
    .unwrap_or_else(|| "Continue.".to_string());
    let history = turns
        .into_iter()
        .map(|turn| match turn.role {
            MessageRole::User => json!({
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": turn.content }],
            }),
            MessageRole::Assistant => json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": turn.content }],
            }),
        })
        .collect();
    (history, prompt)
}

fn parse_models(entries: &[Value]) -> Vec<FetchedModel> {
    entries
        .iter()
        .filter(|m| !m.get("hidden").and_then(Value::as_bool).unwrap_or(false))
        .enumerate()
        .filter_map(|(index, m)| {
            let id = m.get("id").and_then(Value::as_str)?.to_string();
            let is_default = m.get("isDefault").and_then(Value::as_bool).unwrap_or(false);
            Some(FetchedModel {
                display_name: m
                    .get("displayName")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&id)
                    .to_string(),
                // Codex manages output limits itself.
                max_output_tokens: None,
                recommended: is_default || index < 3,
                id,
            })
        })
        .collect()
}

/// "ana@example.com · Plus" from `account/read`.
fn account_label(account: &Value) -> Option<String> {
    let email = account
        .get("email")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let plan = account
        .get("planType")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty() && *p != "unknown")
        .map(plan_name);
    match (email, plan) {
        (Some(email), Some(plan)) => Some(format!("{email} · {plan}")),
        (Some(email), None) => Some(email.to_string()),
        (None, Some(plan)) => Some(format!("ChatGPT {plan}")),
        (None, None) => None,
    }
}

fn plan_name(plan: &str) -> String {
    match plan {
        "free" => "Free".into(),
        "go" => "Go".into(),
        "plus" => "Plus".into(),
        "pro" | "prolite" => "Pro".into(),
        "team" => "Team".into(),
        "enterprise" => "Enterprise".into(),
        p if p.starts_with("edu") => "Edu".into(),
        p if p.contains("business") => "Business".into(),
        p if p.starts_with("enterprise") || p.starts_with("ent") => "Enterprise".into(),
        other => {
            let mut chars = other.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

impl AccountRuntime for CodexRuntime {
    fn status(&self) -> BoxFuture<'_, AppResult<RuntimeStatus>> {
        Box::pin(async move {
            let conn = match self.connection().await {
                Ok(conn) => conn,
                Err(StartError::Unavailable(message)) => {
                    return Ok(RuntimeStatus::NotInstalled(message))
                }
                Err(StartError::Failed(error)) => return Err(error),
            };
            let read = conn
                .request("account/read", json!({ "refreshToken": false }))
                .await?;
            let account = read.get("account").unwrap_or(&Value::Null);
            Ok(match account.get("type").and_then(Value::as_str) {
                Some("chatgpt") => RuntimeStatus::SignedIn {
                    label: account_label(account),
                },
                // No sign-in, or not a ChatGPT one (ReMa uses API keys directly).
                _ => RuntimeStatus::SignedOut,
            })
        })
    }

    fn start_sign_in(&self, device_code: bool) -> BoxFuture<'_, AppResult<SignInAttempt>> {
        Box::pin(async move {
            let conn = self.connection().await?;
            let kind = if device_code {
                "chatgptDeviceCode"
            } else {
                "chatgpt"
            };
            let started = conn
                .request("account/login/start", json!({ "type": kind }))
                .await?;
            let login_id = started
                .get("loginId")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::provider("Codex did not start the sign-in"))?
                .to_string();
            let url_field = if device_code {
                "verificationUrl"
            } else {
                "authUrl"
            };
            let open_url = started
                .get(url_field)
                .and_then(Value::as_str)
                .filter(|u| u.starts_with("https://"))
                .ok_or_else(|| AppError::provider("Codex returned no sign-in page"))?
                .to_string();
            let user_code = started
                .get("userCode")
                .and_then(Value::as_str)
                .map(str::to_string);

            let mut route = conn.subscribe(format!("login:{login_id}"));
            let cancel = CancellationToken::new();
            let cancelled = cancel.clone();
            let outcome = Box::pin(async move {
                let completed = tokio::select! {
                    _ = cancelled.cancelled() => {
                        let _ = conn
                            .request("account/login/cancel", json!({ "loginId": login_id }))
                            .await;
                        return SignInOutcome::Cancelled;
                    }
                    completed = tokio::time::timeout(SIGN_IN_TIMEOUT, route.recv()) => completed,
                };
                match completed {
                    Err(_) => {
                        let _ = conn
                            .request("account/login/cancel", json!({ "loginId": login_id }))
                            .await;
                        SignInOutcome::Failed("The sign-in timed out. Try again.".into())
                    }
                    Ok(None) => SignInOutcome::Failed("Codex stopped during sign-in.".into()),
                    Ok(Some(n)) => login_outcome(&n.params),
                }
            });
            Ok(SignInAttempt {
                open_url: Some(open_url),
                user_code,
                outcome,
                cancel,
            })
        })
    }

    fn sign_out(&self) -> BoxFuture<'_, AppResult<()>> {
        Box::pin(async move {
            let conn = self.connection().await?;
            conn.request("account/logout", Value::Null).await?;
            Ok(())
        })
    }

    fn supports_device_code(&self) -> bool {
        true
    }

    fn shutdown(&self) {
        if let Some(conn) = self.current.lock().unwrap().take() {
            conn.close();
        }
    }
}

fn login_outcome(params: &Value) -> SignInOutcome {
    if params.get("success").and_then(Value::as_bool) == Some(true) {
        return SignInOutcome::Succeeded;
    }
    let error = params
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let lower = error.to_lowercase();
    if lower.contains("not completed")
        || lower.contains("cancel")
        || lower.contains("access_denied")
    {
        SignInOutcome::Cancelled
    } else if error.is_empty() {
        SignInOutcome::Failed("The sign-in did not complete.".into())
    } else {
        SignInOutcome::Failed(format!("The sign-in failed: {error}"))
    }
}

/// Finds Codex, checks its version and starts `codex app-server`.
async fn spawn_app_server(
    home: &PathBuf,
    workdir: &PathBuf,
) -> Result<Arc<Connection>, StartError> {
    let located = locate::find("codex", OVERRIDE_VAR)
        .await
        .ok_or_else(|| StartError::Unavailable(INSTALL_HINT.into()))?;
    check_version(&located).await?;
    std::fs::create_dir_all(home).map_err(AppError::from)?;
    std::fs::create_dir_all(workdir).map_err(AppError::from)?;

    let mut command = Command::new(&located.path);
    for setting in SETTINGS {
        command.arg("-c").arg(setting);
    }
    for feature in DISABLED_FEATURES {
        command.arg("-c").arg(format!("features.{feature}=false"));
    }
    command
        .args(["app-server", "--listen", "stdio://"])
        .env("CODEX_HOME", home)
        .env("PATH", located.search_path())
        // Only the ChatGPT sign-in: never pick up keys from the environment.
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN")
        .env_remove("OPENAI_BASE_URL")
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    no_console_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| AppError::provider(format!("could not start Codex: {e}")))?;
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(AppError::internal("Codex pipes are unavailable").into());
    };
    let conn = Connection::start(stdout, stdin);
    conn.watch_stderr(stderr);
    *conn.child.lock().unwrap() = Some(child);
    Ok(conn)
}

async fn check_version(located: &locate::Located) -> Result<(), StartError> {
    let mut command = Command::new(&located.path);
    command
        .arg("--version")
        .env("PATH", located.search_path())
        .stdin(Stdio::null())
        .kill_on_drop(true);
    no_console_window(&mut command);
    let output = tokio::time::timeout(Duration::from_secs(20), command.output())
        .await
        .map_err(|_| AppError::provider("Codex did not respond"))?
        .map_err(|e| AppError::provider(format!("could not run Codex: {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    match parse_version(&text) {
        // Development builds report 0.0.0.
        Some(v) if v == [0, 0, 0] || v >= MIN_VERSION => Ok(()),
        Some(v) => Err(StartError::Unavailable(format!(
            "ReMa needs Codex {}.{}.{} or newer (found {}.{}.{}). Update it with “codex update”, “brew upgrade --cask codex” or “npm install -g @openai/codex@latest”.",
            MIN_VERSION[0], MIN_VERSION[1], MIN_VERSION[2], v[0], v[1], v[2]
        ))),
        None => Err(StartError::Unavailable(INSTALL_HINT.into())),
    }
}

/// `codex-cli 0.157.0` → `[0, 157, 0]` (pre-release suffixes ignored).
fn parse_version(text: &str) -> Option<[u64; 3]> {
    let token = text.split_whitespace().last()?;
    let mut parts = token.split(['.', '-', '+']).map(|p| p.parse::<u64>().ok());
    Some([parts.next()??, parts.next()??, parts.next()??])
}

#[cfg(windows)]
fn no_console_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_console_window(_: &mut Command) {}

/// A JSON-RPC connection to an app-server (newline-delimited JSON).
pub struct Connection {
    writer: tokio::sync::Mutex<Box<dyn AsyncWrite + Send + Unpin>>,
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, oneshot::Sender<Result<Value, String>>>>,
    routes: Mutex<HashMap<String, mpsc::UnboundedSender<Notification>>>,
    closed: CancellationToken,
    child: Mutex<Option<Child>>,
    stderr: Mutex<VecDeque<String>>,
}

/// Notifications for one thread or sign-in; unregisters when dropped.
pub struct Route {
    key: String,
    rx: mpsc::UnboundedReceiver<Notification>,
    conn: Arc<Connection>,
}

impl Route {
    async fn recv(&mut self) -> Option<Notification> {
        tokio::select! {
            n = self.rx.recv() => n,
            _ = self.conn.closed.cancelled() => None,
        }
    }
}

impl Drop for Route {
    fn drop(&mut self) {
        self.conn.routes.lock().unwrap().remove(&self.key);
    }
}

impl Connection {
    /// Starts reading messages from `reader`.
    pub fn start(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Arc<Self> {
        let conn = Arc::new(Self {
            writer: tokio::sync::Mutex::new(Box::new(writer)),
            next_id: AtomicI64::new(1),
            pending: Mutex::default(),
            routes: Mutex::default(),
            closed: CancellationToken::new(),
            child: Mutex::default(),
            stderr: Mutex::default(),
        });
        let reading = conn.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            loop {
                tokio::select! {
                    _ = reading.closed.cancelled() => break,
                    line = lines.next_line() => match line {
                        Ok(Some(line)) => reading.handle(&line).await,
                        _ => break,
                    },
                }
            }
            reading.close();
        });
        conn
    }

    fn watch_stderr(self: &Arc<Self>, stderr: impl AsyncRead + Send + Unpin + 'static) {
        let conn = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut tail = conn.stderr.lock().unwrap();
                tail.push_back(line);
                if tail.len() > 20 {
                    tail.pop_front();
                }
            }
        });
    }

    /// The runtime's last error line, to explain a failed start.
    fn stderr_hint(&self) -> Option<String> {
        self.stderr
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|l| l.trim_start().starts_with("Error"))
            .map(|l| l.trim().chars().take(300).collect())
    }

    /// Stops the process and fails everything waiting on it.
    pub fn close(&self) {
        self.closed.cancel();
        for (_, waiter) in self.pending.lock().unwrap().drain() {
            let _ = waiter.send(Err("Codex stopped".into()));
        }
        self.routes.lock().unwrap().clear();
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.start_kill();
        }
    }

    async fn handle(&self, line: &str) {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").filter(|id| !id.is_null()).cloned();
        match (method, id) {
            (Some(method), Some(id)) => self.answer(method, id).await,
            (Some(method), None) => self.route(Notification {
                method: method.to_string(),
                params: message.get("params").cloned().unwrap_or(Value::Null),
            }),
            (None, Some(id)) => {
                let Some(waiter) = id
                    .as_i64()
                    .and_then(|id| self.pending.lock().unwrap().remove(&id))
                else {
                    return;
                };
                let result = match message.get("error") {
                    Some(error) => Err(error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                        .to_string()),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = waiter.send(result);
            }
            (None, None) => {}
        }
    }

    /// Requests from the runtime: approvals are declined (ReMa's threads run
    /// no tools), anything else is unsupported.
    async fn answer(&self, method: &str, id: Value) {
        let reply = match method {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                json!({ "id": id, "result": { "decision": "decline" } })
            }
            "execCommandApproval" | "applyPatchApproval" => {
                json!({ "id": id, "result": { "decision": "denied" } })
            }
            _ => json!({
                "id": id,
                "error": { "code": -32601, "message": "ReMa does not support this request" },
            }),
        };
        let _ = self.write(&reply).await;
    }

    fn route(&self, notification: Notification) {
        let params = &notification.params;
        let key = if notification.method == "account/login/completed" {
            params
                .get("loginId")
                .and_then(Value::as_str)
                .map(|id| format!("login:{id}"))
        } else {
            params
                .get("threadId")
                .and_then(Value::as_str)
                .map(|id| format!("thread:{id}"))
        };
        let Some(key) = key else { return };
        if let Some(tx) = self.routes.lock().unwrap().get(&key) {
            let _ = tx.send(notification);
        }
    }

    pub fn subscribe(self: &Arc<Self>, key: String) -> Route {
        let (tx, rx) = mpsc::unbounded_channel();
        self.routes.lock().unwrap().insert(key.clone(), tx);
        Route {
            key,
            rx,
            conn: self.clone(),
        }
    }

    async fn write(&self, message: &Value) -> AppResult<()> {
        let mut line =
            serde_json::to_vec(message).map_err(|e| AppError::internal(e.to_string()))?;
        line.push(b'\n');
        let mut writer = self.writer.lock().await;
        let written = async {
            writer.write_all(&line).await?;
            writer.flush().await
        }
        .await;
        written.map_err(|_| stopped())
    }

    pub async fn request(&self, method: &str, params: Value) -> AppResult<Value> {
        if self.closed.is_cancelled() {
            return Err(stopped());
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let mut message = json!({ "id": id, "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        if let Err(error) = self.write(&message).await {
            self.pending.lock().unwrap().remove(&id);
            return Err(error);
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(AppError::provider("Codex did not answer in time"))
            }
            Ok(Err(_)) => Err(stopped()),
            Ok(Ok(Err(message))) if message == "Codex stopped" => Err(stopped()),
            Ok(Ok(Err(message))) => Err(AppError::provider(format!("Codex: {message}"))),
            Ok(Ok(Ok(result))) => Ok(result),
        }
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> AppResult<()> {
        let mut message = json!({ "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }
        self.write(&message).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Turn;
    use tokio::io::{duplex, DuplexStream};

    /// An in-process stand-in for `codex app-server`: answers requests with
    /// `respond` and can push notifications.
    struct FakeServer {
        requests: Arc<Mutex<Vec<Value>>>,
        push: mpsc::UnboundedSender<Value>,
    }

    type Responder = Arc<dyn Fn(&str, &Value) -> Vec<Value> + Send + Sync>;

    fn fake_server(respond: Responder) -> (Arc<Connection>, FakeServer) {
        let (client_io, server_io) = duplex(1 << 16);
        let (client_read, client_write) = tokio::io::split(client_io);
        let conn = Connection::start(client_read, client_write);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (push, mut pushed) = mpsc::unbounded_channel::<Value>();
        let seen = requests.clone();
        tokio::spawn(async move {
            let (server_read, mut server_write) = tokio::io::split::<DuplexStream>(server_io);
            let mut lines = BufReader::new(server_read).lines();
            loop {
                tokio::select! {
                    line = lines.next_line() => {
                        let Ok(Some(line)) = line else { break };
                        let message: Value = serde_json::from_str(&line).unwrap();
                        seen.lock().unwrap().push(message.clone());
                        let (Some(method), Some(id)) = (message["method"].as_str(), message.get("id")) else {
                            continue;
                        };
                        for reply in respond(method, &message["params"]) {
                            let reply = if reply.get("method").is_some() {
                                reply
                            } else {
                                json!({ "id": id, "result": reply })
                            };
                            let mut bytes = serde_json::to_vec(&reply).unwrap();
                            bytes.push(b'\n');
                            server_write.write_all(&bytes).await.unwrap();
                        }
                    }
                    Some(message) = pushed.recv() => {
                        let mut bytes = serde_json::to_vec(&message).unwrap();
                        bytes.push(b'\n');
                        server_write.write_all(&bytes).await.unwrap();
                    }
                }
            }
        });
        (conn, FakeServer { requests, push })
    }

    fn runtime(
        respond: Responder,
    ) -> (
        CodexRuntime,
        Arc<Mutex<Vec<Value>>>,
        mpsc::UnboundedSender<Value>,
    ) {
        let (conn, server) = fake_server(respond);
        let runtime = CodexRuntime::with_connector(move || conn.clone());
        (runtime, server.requests, server.push)
    }

    fn methods(requests: &Arc<Mutex<Vec<Value>>>) -> Vec<String> {
        requests
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| m["method"].as_str().map(str::to_string))
            .collect()
    }

    fn basic(method: &str, _params: &Value) -> Vec<Value> {
        match method {
            "initialize" => vec![json!({ "userAgent": "rema/0.157.0" })],
            _ => vec![json!({})],
        }
    }

    #[tokio::test]
    async fn reads_the_signed_in_chatgpt_account() {
        let (runtime, requests, _) = runtime(Arc::new(|method, params| match method {
            "account/read" => {
                assert_eq!(params["refreshToken"], false);
                vec![json!({
                    "account": { "type": "chatgpt", "email": "ana@example.com", "planType": "plus" },
                    "requiresOpenaiAuth": true,
                })]
            }
            _ => basic(method, params),
        }));
        assert_eq!(
            runtime.status().await.unwrap(),
            RuntimeStatus::SignedIn {
                label: Some("ana@example.com · Plus".into())
            }
        );
        // The handshake identifies ReMa as itself, then is acknowledged.
        let log = requests.lock().unwrap().clone();
        assert_eq!(log[0]["method"], "initialize");
        assert_eq!(log[0]["params"]["clientInfo"]["name"], "rema");
        assert_eq!(log[1]["method"], "initialized");
        assert!(log[1].get("id").is_none());
    }

    #[tokio::test]
    async fn api_key_or_no_account_is_signed_out() {
        for account in [Value::Null, json!({ "type": "apiKey" })] {
            let (runtime, _, _) = runtime(Arc::new(move |method, params| match method {
                "account/read" => {
                    vec![json!({ "account": account.clone(), "requiresOpenaiAuth": true })]
                }
                _ => basic(method, params),
            }));
            assert_eq!(runtime.status().await.unwrap(), RuntimeStatus::SignedOut);
        }
    }

    #[tokio::test]
    async fn browser_sign_in_waits_for_the_completion_notification() {
        let (runtime, requests, push) = runtime(Arc::new(|method, params| match method {
            "account/login/start" => {
                assert_eq!(params["type"], "chatgpt");
                vec![json!({
                    "type": "chatgpt",
                    "loginId": "login-1",
                    "authUrl": "https://auth.openai.com/oauth/authorize?x=1",
                })]
            }
            _ => basic(method, params),
        }));
        let attempt = runtime.start_sign_in(false).await.unwrap();
        assert_eq!(
            attempt.open_url.as_deref(),
            Some("https://auth.openai.com/oauth/authorize?x=1")
        );
        assert_eq!(attempt.user_code, None);

        // A completion for another attempt is ignored.
        push.send(json!({ "method": "account/login/completed",
            "params": { "loginId": "other", "success": false, "error": "x" } }))
            .unwrap();
        push.send(json!({ "method": "account/login/completed",
            "params": { "loginId": "login-1", "success": true, "error": null } }))
            .unwrap();
        assert_eq!(attempt.outcome.await, SignInOutcome::Succeeded);
        assert!(methods(&requests).contains(&"account/login/start".to_string()));
    }

    #[tokio::test]
    async fn device_code_sign_in_returns_the_code_to_show() {
        let (runtime, _, _) = runtime(Arc::new(|method, params| match method {
            "account/login/start" => {
                assert_eq!(params["type"], "chatgptDeviceCode");
                vec![json!({
                    "type": "chatgptDeviceCode",
                    "loginId": "login-2",
                    "verificationUrl": "https://auth.openai.com/codex/device",
                    "userCode": "WXYZ-9876",
                })]
            }
            _ => basic(method, params),
        }));
        let attempt = runtime.start_sign_in(true).await.unwrap();
        assert_eq!(
            attempt.open_url.as_deref(),
            Some("https://auth.openai.com/codex/device")
        );
        assert_eq!(attempt.user_code.as_deref(), Some("WXYZ-9876"));
    }

    #[tokio::test]
    async fn cancelling_a_sign_in_asks_codex_to_stop_it() {
        let (runtime, requests, _) = runtime(Arc::new(|method, params| match method {
            "account/login/start" => vec![json!({
                "type": "chatgpt", "loginId": "login-3", "authUrl": "https://auth.openai.com/a",
            })],
            "account/login/cancel" => {
                assert_eq!(params["loginId"], "login-3");
                vec![json!({ "status": "canceled" })]
            }
            _ => basic(method, params),
        }));
        let attempt = runtime.start_sign_in(false).await.unwrap();
        attempt.cancel.cancel();
        assert_eq!(attempt.outcome.await, SignInOutcome::Cancelled);
        assert!(methods(&requests).contains(&"account/login/cancel".to_string()));
    }

    #[test]
    fn classifies_sign_in_results() {
        assert_eq!(
            login_outcome(
                &json!({ "success": false, "error": "Login server error: Login was not completed" })
            ),
            SignInOutcome::Cancelled
        );
        assert_eq!(
            login_outcome(&json!({ "success": false, "error": "access_denied" })),
            SignInOutcome::Cancelled
        );
        assert!(matches!(
            login_outcome(&json!({ "success": false, "error": "workspace blocked" })),
            SignInOutcome::Failed(m) if m.contains("workspace blocked")
        ));
    }

    #[tokio::test]
    async fn lists_the_account_models_without_hidden_ones() {
        let (runtime, _, _) = runtime(Arc::new(|method, params| match method {
            "model/list" if params["cursor"].is_null() => vec![json!({
                "data": [
                    { "id": "gpt-6-astra", "displayName": "GPT-6-Astra", "hidden": false, "isDefault": true },
                    { "id": "internal", "displayName": "Internal", "hidden": true, "isDefault": false },
                ],
                "nextCursor": "page-2",
            })],
            "model/list" => vec![json!({
                "data": [{ "id": "gpt-6-sol", "displayName": "GPT-6-Sol", "hidden": false, "isDefault": false }],
                "nextCursor": null,
            })],
            _ => basic(method, params),
        }));
        let models = runtime.list_models().await.unwrap();
        let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["gpt-6-astra", "gpt-6-sol"]);
        assert_eq!(models[0].display_name, "GPT-6-Astra");
        assert!(models[0].recommended);
        assert_eq!(models[0].max_output_tokens, None);
    }

    fn chat_request() -> ChatRequest {
        ChatRequest {
            system: Some("You are ReMa.".into()),
            turns: vec![
                Turn {
                    role: MessageRole::User,
                    content: "My name is Ana.".into(),
                },
                Turn {
                    role: MessageRole::Assistant,
                    content: "Hi Ana!".into(),
                },
                Turn {
                    role: MessageRole::User,
                    content: "What is my name?".into(),
                },
            ],
            max_output_tokens: None,
        }
    }

    fn turn_server(finish: Value) -> Responder {
        Arc::new(move |method, params| match method {
            "thread/start" => vec![json!({ "thread": { "id": "t1" }, "model": params["model"] })],
            "turn/start" => {
                let notify =
                    |method: &str, params: Value| json!({ "method": method, "params": params });
                vec![
                    json!({ "turn": { "id": "turn-1", "status": "inProgress", "items": [] } }),
                    notify(
                        "item/agentMessage/delta",
                        json!({ "threadId": "t1", "turnId": "turn-1", "itemId": "m1", "delta": "Your name" }),
                    ),
                    // Another thread's text never leaks into this one.
                    notify(
                        "item/agentMessage/delta",
                        json!({ "threadId": "t2", "turnId": "x", "itemId": "m9", "delta": "WRONG" }),
                    ),
                    notify(
                        "item/agentMessage/delta",
                        json!({ "threadId": "t1", "turnId": "turn-1", "itemId": "m1", "delta": " is" }),
                    ),
                    // The final text fills in anything the deltas missed.
                    notify(
                        "item/completed",
                        json!({ "threadId": "t1", "turnId": "turn-1",
                        "item": { "type": "agentMessage", "id": "m1", "text": "Your name is Ana." } }),
                    ),
                    notify(
                        "turn/completed",
                        json!({ "threadId": "t1", "turn": finish.clone() }),
                    ),
                ]
            }
            _ => basic(method, params),
        })
    }

    #[tokio::test]
    async fn streams_a_turn_on_an_ephemeral_locked_down_thread() {
        let (runtime, requests, _) = runtime(turn_server(
            json!({ "id": "turn-1", "status": "completed", "items": [] }),
        ));
        let mut text = String::new();
        let finish = runtime
            .stream_chat(
                "gpt-6-sol",
                &chat_request(),
                CancellationToken::new(),
                &mut |d| text.push_str(d),
            )
            .await
            .unwrap();
        assert_eq!(finish, Finish::Complete);
        assert_eq!(text, "Your name is Ana.");

        let log = requests.lock().unwrap().clone();
        let find = |method: &str| log.iter().find(|m| m["method"] == method).unwrap().clone();
        let thread = find("thread/start");
        assert_eq!(thread["params"]["ephemeral"], true);
        assert_eq!(thread["params"]["baseInstructions"], "You are ReMa.");
        assert_eq!(thread["params"]["sandbox"], "read-only");
        assert_eq!(thread["params"]["approvalPolicy"], "never");
        assert_eq!(thread["params"]["model"], "gpt-6-sol");

        let history = find("thread/inject_items")["params"]["items"].clone();
        assert_eq!(history[0]["role"], "user");
        assert_eq!(history[0]["content"][0]["text"], "My name is Ana.");
        assert_eq!(history[1]["role"], "assistant");
        assert_eq!(history[1]["content"][0]["type"], "output_text");
        assert_eq!(
            find("turn/start")["params"]["input"][0]["text"],
            "What is my name?"
        );
        // The thread is released afterwards.
        assert_eq!(find("thread/unsubscribe")["params"]["threadId"], "t1");
    }

    #[tokio::test]
    async fn maps_failed_turns_to_actionable_errors() {
        let (runtime, _, _) = runtime(turn_server(json!({
            "id": "turn-1", "status": "failed", "items": [],
            "error": { "message": "401 Unauthorized", "codexErrorInfo": "unauthorized" },
        })));
        let error = runtime
            .stream_chat("m", &chat_request(), CancellationToken::new(), &mut |_| {})
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::Authentication(_)));
        assert!(error.to_string().contains("Sign in again"));

        let limit = turn_error(Some(&json!({
            "message": "You've hit your usage limit", "codexErrorInfo": "usageLimitExceeded",
        })));
        assert!(limit.to_string().contains("usage limit"));
        let network = turn_error(Some(&json!({
            "message": "stream disconnected",
            "codexErrorInfo": { "responseStreamDisconnected": { "httpStatusCode": null } },
        })));
        assert!(matches!(network, AppError::Network(_)));
    }

    #[tokio::test]
    async fn stopping_a_turn_interrupts_it() {
        let (runtime, requests, _) = runtime(Arc::new(|method, params| match method {
            "thread/start" => vec![json!({ "thread": { "id": "t1" } })],
            "turn/start" => vec![json!({ "turn": { "id": "turn-1" } })],
            _ => basic(method, params),
        }));
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            stop.cancel();
        });
        let finish = runtime
            .stream_chat("m", &chat_request(), cancel, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(finish, Finish::Cancelled);
        assert!(methods(&requests).contains(&"turn/interrupt".to_string()));
    }

    #[tokio::test]
    async fn declines_approval_requests_from_the_runtime() {
        let (conn, server) = fake_server(Arc::new(basic));
        server
            .push
            .send(json!({ "id": 99, "method": "item/commandExecution/requestApproval", "params": {} }))
            .unwrap();
        server
            .push
            .send(json!({ "id": "q", "method": "item/tool/requestUserInput", "params": {} }))
            .unwrap();
        // A round trip guarantees both requests were handled.
        conn.request("ping", Value::Null).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let log = server.requests.lock().unwrap().clone();
        let reply = |id: Value| log.iter().find(|m| m["id"] == id).unwrap().clone();
        assert_eq!(reply(json!(99))["result"]["decision"], "decline");
        assert_eq!(reply(json!("q"))["error"]["code"], -32601);
    }

    /// Runs against a real Codex install:
    /// `REMA_CODEX_PATH=/path/to/codex cargo test real_codex -- --ignored`.
    /// Needs no account: it checks the handshake, sign-in start and cancel,
    /// and the model catalog.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn real_codex_runtime_smoke() {
        let home = crate::state::testing::temp_dir();
        let runtime = CodexRuntime::new(home.join("codex"), home.join("work"), "0.1.0");
        assert_eq!(runtime.status().await.unwrap(), RuntimeStatus::SignedOut);
        let models = runtime.list_models().await.unwrap();
        assert!(!models.is_empty());

        let attempt = runtime.start_sign_in(false).await.unwrap();
        let url = attempt.open_url.clone().unwrap();
        assert!(url.starts_with("https://auth.openai.com/"), "{url}");
        assert!(url.contains("originator=rema"), "{url}");
        attempt.cancel.cancel();
        assert_eq!(attempt.outcome.await, SignInOutcome::Cancelled);

        runtime.sign_out().await.unwrap();
        runtime.shutdown();
        println!(
            "models: {:?}",
            models.iter().map(|m| &m.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn parses_versions_and_labels() {
        assert_eq!(parse_version("codex-cli 0.157.0\n"), Some([0, 157, 0]));
        assert_eq!(
            parse_version("codex-cli 0.159.0-alpha.3"),
            Some([0, 159, 0])
        );
        assert_eq!(parse_version("nonsense"), None);
        assert!([0, 150, 9] < MIN_VERSION && [1, 0, 0] > MIN_VERSION);

        assert_eq!(
            account_label(&json!({ "email": "a@b.c", "planType": "pro" })).as_deref(),
            Some("a@b.c · Pro")
        );
        assert_eq!(
            account_label(&json!({ "email": null, "planType": "team" })).as_deref(),
            Some("ChatGPT Team")
        );
        assert_eq!(plan_name("self_serve_business_usage_based"), "Business");
    }

    #[test]
    fn a_trailing_assistant_turn_is_history() {
        let request = ChatRequest {
            turns: vec![
                Turn {
                    role: MessageRole::User,
                    content: "Hi".into(),
                },
                Turn {
                    role: MessageRole::Assistant,
                    content: "Hello".into(),
                },
            ],
            ..ChatRequest::default()
        };
        let (history, prompt) = split_turns(&request);
        assert_eq!(history.len(), 2);
        assert_eq!(prompt, "Continue.");
    }
}
