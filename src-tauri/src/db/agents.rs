//! Custom agents and each conversation's selected agents.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::agent::{Agent, AgentInput},
};

pub fn custom_id(id: i64) -> String {
    format!("custom:{id}")
}

fn from_row(row: &Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: custom_id(row.get(0)?),
        name: row.get(1)?,
        description: row.get(2)?,
        instructions: row.get(3)?,
        icon: row.get(4)?,
        builtin: false,
        updated_at: Some(row.get(5)?),
    })
}

const COLUMNS: &str = "id, name, description, instructions, icon, updated_at";

/// Alphabetical.
pub fn list(conn: &Connection) -> AppResult<Vec<Agent>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM custom_agents ORDER BY name COLLATE NOCASE, id"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<Agent> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM custom_agents WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Agent not found"))
}

pub fn insert(conn: &Connection, input: &AgentInput, now: i64) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO custom_agents (name, description, instructions, icon, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![
            input.name,
            input.description,
            input.instructions,
            input.icon,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, input: &AgentInput, now: i64) -> AppResult<()> {
    let updated = conn.execute(
        "UPDATE custom_agents SET name = ?2, description = ?3, instructions = ?4, icon = ?5,
             updated_at = ?6
         WHERE id = ?1",
        params![
            id,
            input.name,
            input.description,
            input.instructions,
            input.icon,
            now
        ],
    )?;
    if updated == 0 {
        return Err(AppError::not_found("Agent not found"));
    }
    Ok(())
}

/// Deletes the agent and removes it from every conversation.
pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    if conn.execute("DELETE FROM custom_agents WHERE id = ?1", [id])? == 0 {
        return Err(AppError::not_found("Agent not found"));
    }
    conn.execute(
        "DELETE FROM conversation_agents WHERE agent_id = ?1",
        [custom_id(id)],
    )?;
    Ok(())
}

/// The agents selected for a conversation, in selection order.
pub fn conversation_agents(conn: &Connection, conversation_id: i64) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT agent_id FROM conversation_agents WHERE conversation_id = ?1 ORDER BY position",
    )?;
    let rows = stmt.query_map([conversation_id], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn set_conversation_agents(
    conn: &Connection,
    conversation_id: i64,
    agent_ids: &[String],
) -> AppResult<()> {
    conn.execute(
        "DELETE FROM conversation_agents WHERE conversation_id = ?1",
        [conversation_id],
    )?;
    for (position, agent_id) in agent_ids.iter().enumerate() {
        conn.execute(
            "INSERT INTO conversation_agents (conversation_id, agent_id, position)
             VALUES (?1, ?2, ?3)",
            params![conversation_id, agent_id, position as i64],
        )?;
    }
    Ok(())
}
