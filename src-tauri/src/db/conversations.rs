//! Conversations and messages.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::{
        chat::{Conversation, Message, MessageRole, MessageStatus},
        provider::ModelRef,
    },
};

const CONVERSATION_COLUMNS: &str =
    "id, title, provider_id, model_id, created_at, updated_at, profile_context";
const MESSAGE_COLUMNS: &str =
    "id, conversation_id, role, content, status, error, provider_id, model_id, created_at";

fn conversation_from_row(row: &Row) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        title: row.get(1)?,
        model: ModelRef {
            provider_id: row.get(2)?,
            model_id: row.get(3)?,
        },
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        profile_context: row.get(6)?,
    })
}

fn message_from_row(row: &Row) -> rusqlite::Result<Message> {
    let role: String = row.get(2)?;
    let status: String = row.get(4)?;
    let provider_id: Option<String> = row.get(6)?;
    let model_id: Option<String> = row.get(7)?;
    Ok(Message {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        role: MessageRole::parse(&role).unwrap_or(MessageRole::User),
        content: row.get(3)?,
        status: MessageStatus::parse(&status).unwrap_or(MessageStatus::Error),
        error: row.get(5)?,
        model: provider_id
            .zip(model_id)
            .map(|(provider_id, model_id)| ModelRef {
                provider_id,
                model_id,
            }),
        created_at: row.get(8)?,
    })
}

pub fn create(
    conn: &Connection,
    title: &str,
    model: &ModelRef,
    now: i64,
) -> AppResult<Conversation> {
    conn.execute(
        "INSERT INTO conversations (title, provider_id, model_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![title, model.provider_id, model.model_id, now],
    )?;
    get(conn, conn.last_insert_rowid())
}

/// Whether the user shares their Profile in this conversation.
pub fn set_profile_context(conn: &Connection, id: i64, enabled: bool) -> AppResult<()> {
    conn.execute(
        "UPDATE conversations SET profile_context = ?2 WHERE id = ?1",
        params![id, enabled],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> AppResult<Conversation> {
    conn.query_row(
        &format!("SELECT {CONVERSATION_COLUMNS} FROM conversations WHERE id = ?1"),
        [id],
        conversation_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Conversation not found"))
}

/// Most recently active first.
pub fn list(conn: &Connection, limit: u32) -> AppResult<Vec<Conversation>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {CONVERSATION_COLUMNS} FROM conversations ORDER BY updated_at DESC, id DESC LIMIT ?1"
    ))?;
    let rows = stmt.query_map([limit], conversation_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Records activity and the model used most recently.
pub fn touch(conn: &Connection, id: i64, model: &ModelRef, now: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE conversations SET provider_id = ?2, model_id = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, model.provider_id, model.model_id, now],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    let deleted = conn.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
    if deleted == 0 {
        return Err(AppError::not_found("Conversation not found"));
    }
    Ok(())
}

pub struct NewMessage<'a> {
    pub conversation_id: i64,
    pub role: MessageRole,
    pub content: &'a str,
    pub status: MessageStatus,
    pub model: Option<&'a ModelRef>,
    pub created_at: i64,
}

pub fn insert_message(conn: &Connection, message: NewMessage) -> AppResult<Message> {
    conn.execute(
        "INSERT INTO messages (conversation_id, role, content, status, provider_id, model_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            message.conversation_id,
            message.role.as_str(),
            message.content,
            message.status.as_str(),
            message.model.map(|m| &m.provider_id),
            message.model.map(|m| &m.model_id),
            message.created_at,
        ],
    )?;
    get_message(conn, conn.last_insert_rowid())
}

pub fn get_message(conn: &Connection, id: i64) -> AppResult<Message> {
    conn.query_row(
        &format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"),
        [id],
        message_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Message not found"))
}

/// Oldest first.
pub fn list_messages(conn: &Connection, conversation_id: i64) -> AppResult<Vec<Message>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM messages WHERE conversation_id = ?1 ORDER BY id"
    ))?;
    let rows = stmt.query_map([conversation_id], message_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn last_message(conn: &Connection, conversation_id: i64) -> AppResult<Option<Message>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {MESSAGE_COLUMNS} FROM messages WHERE conversation_id = ?1
                 ORDER BY id DESC LIMIT 1"
            ),
            [conversation_id],
            message_from_row,
        )
        .optional()?)
}

/// Stores the final state of an assistant message.
pub fn finish_message(
    conn: &Connection,
    id: i64,
    content: &str,
    status: MessageStatus,
    error: Option<&str>,
) -> AppResult<Message> {
    conn.execute(
        "UPDATE messages SET content = ?2, status = ?3, error = ?4 WHERE id = ?1",
        params![id, content, status.as_str(), error],
    )?;
    get_message(conn, id)
}

/// Clears an assistant message so it can be generated again.
pub fn restart_message(conn: &Connection, id: i64, model: &ModelRef) -> AppResult<Message> {
    conn.execute(
        "UPDATE messages SET content = '', status = 'streaming', error = NULL,
             provider_id = ?2, model_id = ?3
         WHERE id = ?1",
        params![id, model.provider_id, model.model_id],
    )?;
    get_message(conn, id)
}

/// Marks messages left streaming by a previous session (crash or quit).
pub fn mark_interrupted(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE messages SET status = 'error', error = 'Interrupted: ReMa was closed while responding.'
         WHERE status = 'streaming'",
        [],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn model() -> ModelRef {
        ModelRef {
            provider_id: "openai".into(),
            model_id: "gpt-test".into(),
        }
    }

    #[test]
    fn stores_conversations_and_messages() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let conversation = create(c, "Hello", &model(), 1_000)?;
            let user = insert_message(
                c,
                NewMessage {
                    conversation_id: conversation.id,
                    role: MessageRole::User,
                    content: "Hi",
                    status: MessageStatus::Complete,
                    model: None,
                    created_at: 1_000,
                },
            )?;
            let reply = insert_message(
                c,
                NewMessage {
                    conversation_id: conversation.id,
                    role: MessageRole::Assistant,
                    content: "",
                    status: MessageStatus::Streaming,
                    model: Some(&model()),
                    created_at: 1_001,
                },
            )?;
            finish_message(c, reply.id, "Hello!", MessageStatus::Complete, None)?;

            let messages = list_messages(c, conversation.id)?;
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0], user);
            assert_eq!(messages[1].content, "Hello!");
            assert_eq!(messages[1].status, MessageStatus::Complete);
            assert_eq!(messages[1].model, Some(model()));
            assert_eq!(last_message(c, conversation.id)?.unwrap().id, reply.id);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn lists_most_recent_first_and_cascades_deletes() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let older = create(c, "Older", &model(), 1)?;
            let newer = create(c, "Newer", &model(), 2)?;
            assert_eq!(list(c, 10)?[0].id, newer.id);

            touch(c, older.id, &model(), 3)?;
            assert_eq!(list(c, 10)?[0].id, older.id);

            insert_message(
                c,
                NewMessage {
                    conversation_id: older.id,
                    role: MessageRole::User,
                    content: "x",
                    status: MessageStatus::Complete,
                    model: None,
                    created_at: 3,
                },
            )?;
            delete(c, older.id)?;
            assert!(list_messages(c, older.id)?.is_empty());
            assert!(matches!(get(c, older.id), Err(AppError::NotFound(_))));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn marks_interrupted_streams_as_errors() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            let conversation = create(c, "t", &model(), 1)?;
            let reply = insert_message(
                c,
                NewMessage {
                    conversation_id: conversation.id,
                    role: MessageRole::Assistant,
                    content: "",
                    status: MessageStatus::Streaming,
                    model: Some(&model()),
                    created_at: 1,
                },
            )?;
            assert_eq!(mark_interrupted(c)?, 1);
            assert_eq!(get_message(c, reply.id)?.status, MessageStatus::Error);
            Ok(())
        })
        .unwrap();
    }
}
