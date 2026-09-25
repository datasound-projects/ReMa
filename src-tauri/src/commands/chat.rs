use tauri::State;

use crate::{
    error::AppResult,
    models::{
        chat::{Conversation, ConversationDetail, Message, SendMessageInput, SendMessageResult},
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
