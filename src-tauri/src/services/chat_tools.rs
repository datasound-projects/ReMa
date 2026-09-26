//! MCP tools for one chat answer.
//!
//! Only servers that are enabled in Settings *and* selected in this chat are
//! offered, under request-unique names (`mcp_<server>_<tool>`). A tool the
//! server marks read-only runs at once (it still shows in the answer);
//! every other call waits for the user's approval, with its arguments
//! shown: Allow once, Allow for this chat, or Deny. Stopping the answer
//! denies whatever is still waiting.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use serde_json::Value;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::{
    error::{AppError, AppResult},
    llm::{BoxFuture, ToolBox, ToolCall, ToolExecutor, ToolOutput, ToolSpec},
    mcp::client::{tool_info, Connection},
    models::chat::{ActivityKind, ApprovalDecision, ChatEvent, ToolActivity, ToolStatus},
    services::mcp,
    state::AppState,
};

/// Most tools offered in one request (providers cap the count).
const MAX_TOOLS: usize = 100;
const MAX_ARGUMENTS_SHOWN: usize = 2_000;
const MAX_DETAIL: usize = 300;

/// (assistant message, tool call id) → where the user's answer goes.
type Pending = HashMap<(i64, String), oneshot::Sender<ApprovalDecision>>;
/// conversation → (server, tool) allowed until ReMa quits.
type Allowed = HashMap<i64, HashSet<(i64, String)>>;

/// Approvals waiting for the user, and tools allowed for a whole chat.
#[derive(Clone, Default)]
pub struct Approvals {
    pending: Arc<Mutex<Pending>>,
    allowed: Arc<Mutex<Allowed>>,
}

impl Approvals {
    fn wait(&self, message_id: i64, call_id: &str) -> oneshot::Receiver<ApprovalDecision> {
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap()
            .insert((message_id, call_id.to_string()), tx);
        rx
    }

    fn forget(&self, message_id: i64, call_id: &str) {
        self.pending
            .lock()
            .unwrap()
            .remove(&(message_id, call_id.to_string()));
    }

    /// The user's answer to an approval request.
    pub fn respond(
        &self,
        message_id: i64,
        call_id: &str,
        decision: ApprovalDecision,
    ) -> AppResult<()> {
        let sender = self
            .pending
            .lock()
            .unwrap()
            .remove(&(message_id, call_id.to_string()))
            .ok_or_else(|| AppError::validation("This request is no longer waiting."))?;
        let _ = sender.send(decision);
        Ok(())
    }

    fn allowed(&self, conversation_id: i64, server_id: i64, tool: &str) -> bool {
        self.allowed
            .lock()
            .unwrap()
            .get(&conversation_id)
            .is_some_and(|set| set.contains(&(server_id, tool.to_string())))
    }

    fn allow(&self, conversation_id: i64, server_id: i64, tool: &str) {
        self.allowed
            .lock()
            .unwrap()
            .entry(conversation_id)
            .or_default()
            .insert((server_id, tool.to_string()));
    }
}

/// A tool as offered to the model.
struct Offered {
    name: String,
    server_id: i64,
    server: String,
    tool: String,
    read_only: bool,
    connection: Arc<Connection>,
}

/// Runs one answer's tool calls.
pub struct ChatTools {
    state: AppState,
    conversation_id: i64,
    message_id: i64,
    cancel: CancellationToken,
    offered: Vec<Offered>,
}

/// `mcp_<server>_<tool>`: `[a-zA-Z0-9_-]`, at most 64 characters, unique.
pub fn exposed_name(server: &str, tool: &str, taken: &HashSet<String>) -> String {
    let slug: String = server
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|p| !p.is_empty())
        .filter(|p| *p != "mcp")
        .collect::<Vec<_>>()
        .join("_")
        .chars()
        .take(20)
        .collect();
    let slug = if slug.is_empty() {
        "server".to_string()
    } else {
        slug
    };
    let tool: String = tool
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let base: String = format!("mcp_{slug}_{tool}").chars().take(64).collect();
    let mut name = base.clone();
    let mut n = 2;
    while taken.contains(&name) {
        let suffix = format!("_{n}");
        name = base.chars().take(64 - suffix.len()).collect::<String>() + &suffix;
        n += 1;
    }
    name
}

fn shorten(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

impl ChatTools {
    /// Connects the selected, enabled servers and lists their tools.
    /// Servers that cannot be used are reported as activity; the answer goes
    /// on without them. `None` when no tools are available.
    pub async fn prepare(
        state: &AppState,
        conversation_id: i64,
        message_id: i64,
        server_ids: &[i64],
        cancel: CancellationToken,
    ) -> AppResult<(Option<ToolBox>, Vec<ToolActivity>)> {
        let servers = mcp::enabled_selection(state, server_ids)?;
        let mut notices = Vec::new();
        let mut offered: Vec<Offered> = Vec::new();
        let mut specs: Vec<ToolSpec> = Vec::new();
        let mut taken = HashSet::new();
        for server in servers {
            let connection = match mcp::connect_server(state, server.id).await {
                Ok(connection) => connection,
                Err(error) => {
                    notices.push(ToolActivity {
                        id: format!("server-{}", server.id),
                        server_id: Some(server.id),
                        server: server.name.clone(),
                        tool: String::new(),
                        status: ToolStatus::Unavailable,
                        arguments: String::new(),
                        detail: Some(shorten(&error.to_string(), MAX_DETAIL)),
                        read_only: false,
                        kind: ActivityKind::Mcp,
                        sources: Vec::new(),
                    });
                    continue;
                }
            };
            for tool in &connection.tools {
                if specs.len() >= MAX_TOOLS {
                    break;
                }
                let info = tool_info(tool);
                let name = exposed_name(&server.name, &info.name, &taken);
                taken.insert(name.clone());
                let about = match (&info.title, info.description.is_empty()) {
                    (Some(title), false) => format!("{title}: {}", info.description),
                    (Some(title), true) => title.clone(),
                    (None, _) => info.description.clone(),
                };
                specs.push(ToolSpec {
                    name: name.clone(),
                    description: format!("[{} MCP server] {about}", server.name),
                    input_schema: Value::Object((*tool.input_schema).clone()),
                });
                offered.push(Offered {
                    name,
                    server_id: server.id,
                    server: server.name.clone(),
                    tool: info.name,
                    read_only: info.read_only,
                    connection: connection.clone(),
                });
            }
        }
        if offered.is_empty() {
            return Ok((None, notices));
        }
        let executor = Arc::new(ChatTools {
            state: state.clone(),
            conversation_id,
            message_id,
            cancel,
            offered,
        });
        Ok((Some(ToolBox { specs, executor }), notices))
    }

    fn report(&self, activity: &ToolActivity) {
        self.state
            .generations
            .record_activity(self.message_id, activity.clone());
        self.state.events.chat(ChatEvent::Activity {
            conversation_id: self.conversation_id,
            message_id: self.message_id,
            activity: activity.clone(),
        });
    }

    async fn run(&self, call: &ToolCall) -> ToolOutput {
        let Some(tool) = self.offered.iter().find(|t| t.name == call.name) else {
            return ToolOutput::error(format!("There is no tool named {}.", call.name));
        };
        let mut activity = ToolActivity {
            id: call.id.clone(),
            server_id: Some(tool.server_id),
            server: tool.server.clone(),
            tool: tool.tool.clone(),
            status: ToolStatus::Running,
            arguments: shorten(&call.arguments.to_string(), MAX_ARGUMENTS_SHOWN),
            detail: None,
            read_only: tool.read_only,
            kind: ActivityKind::Mcp,
            sources: Vec::new(),
        };
        let Value::Object(arguments) = call.arguments.clone() else {
            activity.status = ToolStatus::Failed;
            activity.detail = Some("The model sent arguments that are not valid JSON.".into());
            self.report(&activity);
            return ToolOutput::error("The arguments were not a valid JSON object.");
        };

        let approvals = &self.state.approvals;
        if !tool.read_only && !approvals.allowed(self.conversation_id, tool.server_id, &tool.tool) {
            activity.status = ToolStatus::AwaitingApproval;
            let decision = approvals.wait(self.message_id, &call.id);
            self.report(&activity);
            let decision = tokio::select! {
                _ = self.cancel.cancelled() => None,
                decision = decision => decision.ok(),
            };
            approvals.forget(self.message_id, &call.id);
            match decision {
                Some(ApprovalDecision::Allow) => {}
                Some(ApprovalDecision::AllowForChat) => {
                    approvals.allow(self.conversation_id, tool.server_id, &tool.tool)
                }
                Some(ApprovalDecision::Deny) | None => {
                    activity.status = ToolStatus::Denied;
                    activity.detail = Some(if decision.is_some() {
                        "You declined this tool call.".into()
                    } else {
                        "The answer was stopped before this ran.".into()
                    });
                    self.report(&activity);
                    return ToolOutput::error(
                        "The user declined this tool call. Do not retry it; continue without it.",
                    );
                }
            }
            activity.status = ToolStatus::Running;
        }

        self.report(&activity);
        match tool
            .connection
            .call(&tool.tool, arguments, &self.cancel)
            .await
        {
            Ok((text, is_error)) => {
                activity.status = if is_error {
                    ToolStatus::Failed
                } else {
                    ToolStatus::Completed
                };
                activity.detail = Some(shorten(&text, MAX_DETAIL)).filter(|d| !d.is_empty());
                self.report(&activity);
                ToolOutput {
                    content: if text.is_empty() {
                        "(no output)".into()
                    } else {
                        text
                    },
                    is_error,
                }
            }
            Err(message) => {
                activity.status = ToolStatus::Failed;
                activity.detail = Some(shorten(&message, MAX_DETAIL));
                self.report(&activity);
                self.state.events.mcp_changed();
                ToolOutput::error(message)
            }
        }
    }
}

impl ToolExecutor for ChatTools {
    fn execute<'a>(&'a self, call: &'a ToolCall) -> BoxFuture<'a, ToolOutput> {
        Box::pin(self.run(call))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_unique_provider_safe_names() {
        let mut taken = HashSet::new();
        let a = exposed_name("LinkedIn MCP", "search.jobs", &taken);
        assert_eq!(a, "mcp_linkedin_search_jobs");
        taken.insert(a.clone());
        assert_eq!(
            exposed_name("LinkedIn MCP", "search.jobs", &taken),
            "mcp_linkedin_search_jobs_2"
        );
        let long = exposed_name("Local Files", &"x".repeat(100), &taken);
        assert_eq!(long.len(), 64);
        assert!(long
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        assert!(!exposed_name("MCP", "t", &taken).starts_with("mcp__"));
        assert_eq!(
            exposed_name("Überserver ✨", "t", &taken),
            "mcp_berserver_t"
        );
    }
}
