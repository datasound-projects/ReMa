//! Portfolio Studio documents. Content is stored as JSON that the service
//! validated; a row that no longer parses is shown empty, never lost.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::{AppError, AppResult},
    models::portfolio::{
        CoverLetter, PageSize, PortfolioContent, PortfolioDocument, PortfolioInput, PortfolioKind,
        PortfolioStyle,
    },
};

const COLUMNS: &str = "id, name, template_id, page_size, accent, content, created_at, updated_at, \
                       kind, style, letter, source_document_id";

fn from_row(row: &Row) -> rusqlite::Result<PortfolioDocument> {
    let page_size: String = row.get(3)?;
    let content: String = row.get(5)?;
    let kind: String = row.get(8)?;
    let style: String = row.get(9)?;
    let letter: String = row.get(10)?;
    Ok(PortfolioDocument {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: PortfolioKind::parse(&kind).unwrap_or_default(),
        template_id: row.get(2)?,
        page_size: PageSize::parse(&page_size).unwrap_or(PageSize::A4),
        accent: row.get(4)?,
        style: serde_json::from_str::<PortfolioStyle>(&style).unwrap_or_default(),
        content: serde_json::from_str::<PortfolioContent>(&content).unwrap_or_default(),
        letter: serde_json::from_str::<CoverLetter>(&letter).unwrap_or_default(),
        source_document_id: row.get(11)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

/// Most recently edited first.
pub fn list(conn: &Connection) -> AppResult<Vec<PortfolioDocument>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM portfolio_documents ORDER BY updated_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<PortfolioDocument> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM portfolio_documents WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("CV not found"))
}

fn json<T: serde::Serialize>(value: &T) -> AppResult<String> {
    serde_json::to_string(value).map_err(|e| AppError::internal(e.to_string()))
}

pub fn insert(conn: &Connection, input: &PortfolioInput, now: i64) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO portfolio_documents
             (name, template_id, page_size, accent, content, created_at, updated_at,
              kind, style, letter, source_document_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?8, ?9, ?10)",
        params![
            input.name,
            input.template_id,
            input.page_size.as_str(),
            input.accent,
            json(&input.content)?,
            now,
            input.kind.as_str(),
            json(&input.style)?,
            json(&input.letter)?,
            input.source_document_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, input: &PortfolioInput, now: i64) -> AppResult<()> {
    let updated = conn.execute(
        "UPDATE portfolio_documents
         SET name = ?2, template_id = ?3, page_size = ?4, accent = ?5, content = ?6,
             updated_at = ?7, kind = ?8, style = ?9, letter = ?10, source_document_id = ?11
         WHERE id = ?1",
        params![
            id,
            input.name,
            input.template_id,
            input.page_size.as_str(),
            input.accent,
            json(&input.content)?,
            now,
            input.kind.as_str(),
            json(&input.style)?,
            json(&input.letter)?,
            input.source_document_id,
        ],
    )?;
    if updated == 0 {
        return Err(AppError::not_found("CV not found"));
    }
    Ok(())
}

/// Changes only the name (a rename from the document list).
pub fn rename(conn: &Connection, id: i64, name: &str, now: i64) -> AppResult<()> {
    let updated = conn.execute(
        "UPDATE portfolio_documents SET name = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, name, now],
    )?;
    if updated == 0 {
        return Err(AppError::not_found("Document not found"));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    if conn.execute("DELETE FROM portfolio_documents WHERE id = ?1", [id])? == 0 {
        return Err(AppError::not_found("CV not found"));
    }
    Ok(())
}
