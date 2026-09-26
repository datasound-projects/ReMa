use serde::{Deserialize, Serialize};
use specta::Type;

use super::{provider::ModelRef, text_enum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

text_enum!(MessageRole { User => "user", Assistant => "assistant" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Complete,
    /// The assistant is still generating this message.
    Streaming,
    /// The user stopped generation; `content` holds the partial response.
    Stopped,
    /// Generation failed; see `error`.
    Error,
}

text_enum!(MessageStatus {
    Complete => "complete",
    Streaming => "streaming",
    Stopped => "stopped",
    Error => "error",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    /// Model used for the most recent reply.
    pub model: ModelRef,
    /// The user shares their Profile with the model in this conversation.
    pub profile_context: bool,
    /// Agents selected for this conversation, in selection order.
    pub agent_ids: Vec<String>,
    /// MCP servers made available in this conversation, in selection order.
    pub mcp_server_ids: Vec<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    /// Waiting for the user to allow or deny the call.
    AwaitingApproval,
    Running,
    Completed,
    Failed,
    /// The user denied it, or the answer was stopped first.
    Denied,
    /// A selected server could not be used for this answer.
    Unavailable,
}

/// A tool call (or an unavailable server) while answering, shown with the
/// message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolActivity {
    /// The model's call id (unique within the message).
    pub id: String,
    pub server_id: Option<i64>,
    pub server: String,
    pub tool: String,
    pub status: ToolStatus,
    /// The arguments as compact JSON (shortened), shown before approval.
    pub arguments: String,
    /// What happened: a short result, an error or why it was denied.
    pub detail: Option<String>,
    /// The server marks the tool read-only (it ran without approval).
    pub read_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Allow,
    /// Allow this tool for the rest of the conversation (until ReMa quits).
    AllowForChat,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: i64,
    pub conversation_id: i64,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    pub error: Option<String>,
    /// For assistant messages: the model that produced it.
    pub model: Option<ModelRef>,
    /// Tools used while answering.
    pub activity: Vec<ToolActivity>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDetail {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageInput {
    /// `None` starts a new conversation.
    pub conversation_id: Option<i64>,
    pub content: String,
    pub model: ModelRef,
    /// Include the user's Profile (the composer's "Profile" toggle).
    pub use_profile: bool,
    /// Selected agents, in selection order.
    pub agent_ids: Vec<String>,
    /// MCP servers to make available, in selection order.
    pub mcp_server_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResult {
    pub conversation: Conversation,
    pub user_message: Message,
    /// Placeholder that fills in through `ChatEvent`s.
    pub assistant_message: Message,
}

/// Streaming updates for an assistant message, emitted by the backend.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatEvent {
    /// New text appended to a streaming message.
    #[serde(rename_all = "camelCase")]
    Delta {
        conversation_id: i64,
        message_id: i64,
        text: String,
    },
    /// The message reached a final state (complete, stopped or error).
    #[serde(rename_all = "camelCase")]
    Finished { message: Message },
    /// A tool call started, needs approval, or finished.
    #[serde(rename_all = "camelCase")]
    Activity {
        conversation_id: i64,
        message_id: i64,
        activity: ToolActivity,
    },
}

/// The conversation list changed (created, renamed, updated, deleted).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ConversationsChanged;
