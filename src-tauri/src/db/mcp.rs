//! MCP server configuration and each conversation's selected servers.
//! Secret values are never stored here (see `services::mcp`).

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::mcp::{McpAuth, McpTransport},
};

/// A stored server configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerRow {
    pub id: i64,
    pub name: String,
    pub transport: McpTransport,
    pub command: String,
    pub args: Vec<String>,
    pub env_names: Vec<String>,
    pub cwd: String,
    pub url: String,
    pub auth: McpAuth,
    pub header_name: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Validated configuration to store.
pub struct McpConfig<'a> {
    pub name: &'a str,
    pub transport: McpTransport,
    pub command: &'a str,
    pub args: &'a [String],
    pub env_names: &'a [String],
    pub cwd: &'a str,
    pub url: &'a str,
    pub auth: McpAuth,
    pub header_name: &'a str,
}

const COLUMNS: &str = "id, name, transport, command, args, env_names, cwd, url, auth, header_name,
    enabled, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<McpServerRow> {
    let transport: String = row.get(2)?;
    let args: String = row.get(4)?;
    let env_names: String = row.get(5)?;
    let auth: String = row.get(8)?;
    Ok(McpServerRow {
        id: row.get(0)?,
        name: row.get(1)?,
        transport: McpTransport::parse(&transport).unwrap_or(McpTransport::Stdio),
        command: row.get(3)?,
        args: serde_json::from_str(&args).unwrap_or_default(),
        env_names: serde_json::from_str(&env_names).unwrap_or_default(),
        cwd: row.get(6)?,
        url: row.get(7)?,
        auth: McpAuth::parse(&auth).unwrap_or(McpAuth::None),
        header_name: row.get(9)?,
        enabled: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

/// By name.
pub fn list(conn: &Connection) -> AppResult<Vec<McpServerRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM mcp_servers ORDER BY name COLLATE NOCASE, id"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<McpServerRow> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM mcp_servers WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("MCP server not found"))
}

fn json(values: &[String]) -> AppResult<String> {
    serde_json::to_string(values).map_err(|e| AppError::internal(e.to_string()))
}

fn name_taken(error: rusqlite::Error) -> AppError {
    match &error {
        rusqlite::Error::SqliteFailure(e, _)
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            AppError::validation("Another MCP server already has this name.")
        }
        _ => error.into(),
    }
}

/// New servers start disabled: enabling one is always the user's choice.
pub fn insert(conn: &Connection, config: &McpConfig, now: i64) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO mcp_servers (name, transport, command, args, env_names, cwd, url, auth,
             header_name, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?10)",
        params![
            config.name,
            config.transport.as_str(),
            config.command,
            json(config.args)?,
            json(config.env_names)?,
            config.cwd,
            config.url,
            config.auth.as_str(),
            config.header_name,
            now,
        ],
    )
    .map_err(name_taken)?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, config: &McpConfig, now: i64) -> AppResult<()> {
    let updated = conn
        .execute(
            "UPDATE mcp_servers SET name = ?2, transport = ?3, command = ?4, args = ?5,
                 env_names = ?6, cwd = ?7, url = ?8, auth = ?9, header_name = ?10, updated_at = ?11
             WHERE id = ?1",
            params![
                id,
                config.name,
                config.transport.as_str(),
                config.command,
                json(config.args)?,
                json(config.env_names)?,
                config.cwd,
                config.url,
                config.auth.as_str(),
                config.header_name,
                now,
            ],
        )
        .map_err(name_taken)?;
    if updated == 0 {
        return Err(AppError::not_found("MCP server not found"));
    }
    Ok(())
}

pub fn set_enabled(conn: &Connection, id: i64, enabled: bool, now: i64) -> AppResult<()> {
    let updated = conn.execute(
        "UPDATE mcp_servers SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, enabled, now],
    )?;
    if updated == 0 {
        return Err(AppError::not_found("MCP server not found"));
    }
    Ok(())
}

/// Deletes the server; conversations lose it through `ON DELETE CASCADE`.
pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    if conn.execute("DELETE FROM mcp_servers WHERE id = ?1", [id])? == 0 {
        return Err(AppError::not_found("MCP server not found"));
    }
    Ok(())
}

/// The servers selected for a conversation, in selection order.
pub fn conversation_servers(conn: &Connection, conversation_id: i64) -> AppResult<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT server_id FROM conversation_mcp_servers WHERE conversation_id = ?1 ORDER BY position",
    )?;
    let rows = stmt.query_map([conversation_id], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn set_conversation_servers(
    conn: &Connection,
    conversation_id: i64,
    server_ids: &[i64],
) -> AppResult<()> {
    conn.execute(
        "DELETE FROM conversation_mcp_servers WHERE conversation_id = ?1",
        [conversation_id],
    )?;
    for (position, server_id) in server_ids.iter().enumerate() {
        conn.execute(
            "INSERT INTO conversation_mcp_servers (conversation_id, server_id, position)
             VALUES (?1, ?2, ?3)",
            params![conversation_id, server_id, position as i64],
        )?;
    }
    Ok(())
}
