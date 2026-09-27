//! In-app notifications (important application, interview and connector
//! changes). A `dedupe_key` keeps one notification per event.

use rusqlite::{params, Connection};

use crate::{error::AppResult, models::jobs::NotificationItem};

pub struct NewNotification<'a> {
    pub kind: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub application_id: Option<i64>,
    pub interview_id: Option<i64>,
    pub dedupe_key: Option<&'a str>,
    pub created_at: i64,
}

/// Adds a notification; `false` if one with the same key already exists.
pub fn insert(conn: &Connection, n: &NewNotification<'_>) -> AppResult<bool> {
    let changed = conn.execute(
        "INSERT OR IGNORE INTO notifications (kind, title, body, application_id, interview_id,
             dedupe_key, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            n.kind,
            n.title,
            n.body,
            n.application_id,
            n.interview_id,
            n.dedupe_key,
            n.created_at
        ],
    )?;
    Ok(changed > 0)
}

pub fn list(conn: &Connection, limit: usize) -> AppResult<Vec<NotificationItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, title, body, application_id, created_at, read_at
         FROM notifications ORDER BY created_at DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |r| {
        Ok(NotificationItem {
            id: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            body: r.get(3)?,
            application_id: r.get(4)?,
            created_at: r.get(5)?,
            read: r.get::<_, Option<i64>>(6)?.is_some(),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn mark_read(conn: &Connection, ids: Option<&[i64]>, now: i64) -> AppResult<()> {
    match ids {
        None => {
            conn.execute(
                "UPDATE notifications SET read_at = ?1 WHERE read_at IS NULL",
                [now],
            )?;
        }
        Some(ids) => {
            let mut stmt = conn.prepare(
                "UPDATE notifications SET read_at = ?2 WHERE id = ?1 AND read_at IS NULL",
            )?;
            for id in ids {
                stmt.execute(params![id, now])?;
            }
        }
    }
    Ok(())
}

pub fn clear(conn: &Connection) -> AppResult<()> {
    conn.execute("DELETE FROM notifications", [])?;
    Ok(())
}
