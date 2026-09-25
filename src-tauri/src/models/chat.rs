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
    pub created_at: i64,
    pub updated_at: i64,
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
}

/// The conversation list changed (created, renamed, updated, deleted).
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ConversationsChanged;
