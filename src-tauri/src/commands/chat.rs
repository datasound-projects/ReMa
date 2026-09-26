use tauri::State;

use crate::{
    error::AppResult,
    models::{
        chat::{
            ApprovalDecision, Conversation, ConversationDetail, Message, SendMessageInput,
            SendMessageResult,
        },
        provider::ModelRef,
    },
    services::chat,
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn list_conversations(state: State<'_, AppState>) -> AppResult<Vec<Conversation>> {
    chat::list_conversations(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn get_conversation(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<ConversationDetail> {
    chat::get_conversation(&state, id)
}

#[tauri::command]
#[specta::specta]
pub async fn send_message(
    state: State<'_, AppState>,
    input: SendMessageInput,
) -> AppResult<SendMessageResult> {
    chat::send_message(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_message(
    state: State<'_, AppState>,
    message_id: i64,
    model: ModelRef,
) -> AppResult<Message> {
    chat::retry(&state, message_id, model).await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_generation(state: State<'_, AppState>, message_id: i64) -> AppResult<()> {
    chat::stop(&state, message_id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_conversation(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    chat::delete_conversation(&state, id)
}

/// Stores the agents and MCP servers selected in a conversation.
#[tauri::command]
#[specta::specta]
pub async fn set_conversation_selections(
    state: State<'_, AppState>,
    id: i64,
    agent_ids: Vec<String>,
    mcp_server_ids: Vec<i64>,
) -> AppResult<Conversation> {
    chat::set_selections(&state, id, &agent_ids, &mcp_server_ids)
}

/// The user's answer to a tool call waiting for approval.
#[tauri::command]
#[specta::specta]
pub async fn respond_tool_approval(
    state: State<'_, AppState>,
    message_id: i64,
    call_id: String,
    decision: ApprovalDecision,
) -> AppResult<()> {
    chat::respond_to_tool(&state, message_id, &call_id, decision)
}
