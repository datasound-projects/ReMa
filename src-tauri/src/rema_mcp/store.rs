//! Local persistence for ReMa MCP: normalized job records (never raw pages)
//! under stable ids, the keys that identify them, and search snapshots.
//!
//! A job keeps its id across searches: any key it was seen under (a source
//! job id, its canonical URL, a discovered link) leads back to it. Keys are
//! never moved to another job, so two distinct requisitions are never
//! merged by a later sighting.

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use super::contract::{DescriptionState, JobRecord, JobSummary, SCHEMA_VERSION};
use crate::{
    analytics::normalize,
    error::{AppError, AppResult},
};

fn json<T: serde::Serialize>(value: &T) -> AppResult<String> {
    serde_json::to_string(value).map_err(|e| AppError::internal(format!("ReMa MCP cache: {e}")))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

/// The keys a record is known by.
pub fn keys(record: &JobRecord) -> Vec<String> {
    let mut keys: Vec<String> = record
        .source_ids
        .iter()
        .map(|s| format!("src:{}:{}", s.source, s.id.to_lowercase()))
        .collect();
    let urls = record
        .links
        .canonical_url
        .iter()
        .chain(record.links.discovered.iter().map(|d| &d.url));
    for url in urls {
        if let Some(canonical) = normalize::canonical_url(url) {
            keys.push(format!("url:{canonical}"));
        }
    }
    keys.dedup();
    keys
}

fn rank(state: DescriptionState) -> u8 {
    match state {
        DescriptionState::Full => 3,
        DescriptionState::Partial => 2,
        DescriptionState::SnippetOnly => 1,
        DescriptionState::Missing => 0,
    }
}

/// The id a record is stored under: an existing job sharing a key, else a
/// new id derived from its first key.
pub fn find(conn: &Connection, keys: &[String]) -> AppResult<Option<String>> {
    for key in keys {
        let found: Option<String> = conn
            .query_row(
                "SELECT job_id FROM rema_mcp_job_keys WHERE key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Option<JobRecord>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT record FROM rema_mcp_jobs WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

/// Saves a record under its stable id (assigning one if new) and returns
/// the stored record. A poorer copy (e.g. only a search snippet) never
/// replaces richer content; its links are added.
pub fn upsert(conn: &mut Connection, mut record: JobRecord, now: i64) -> AppResult<JobRecord> {
    let known = keys(&record);
    let tx = conn.transaction()?;
    let existing_id = find(&tx, &known)?;
    let existing = match &existing_id {
        Some(id) => get(&tx, id)?,
        None => None,
    };
    let id = existing_id.unwrap_or_else(|| {
        let seed = known
            .first()
            .cloned()
            .unwrap_or_else(|| record.title.clone());
        format!("rj_{}", &sha(&seed)[..16])
    });
    record.id = id.clone();
    let first_seen = existing
        .as_ref()
        .map(|e| e.dates.first_seen_at.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| super::extract::iso(now));
    record.dates.first_seen_at = first_seen;
    if let Some(old) = existing {
        let richer_old = rank(old.description.state) > rank(record.description.state);
        let mut links = if richer_old {
            old.links.discovered.clone()
        } else {
            record.links.discovered.clone()
        };
        for link in old
            .links
            .discovered
            .iter()
            .chain(record.links.discovered.iter())
        {
            if !links.contains(link) {
                links.push(link.clone());
            }
        }
        if richer_old {
            let mut kept = old;
            kept.links.discovered = links;
            for dup in record.quality.possible_duplicates {
                if !kept.quality.possible_duplicates.contains(&dup) {
                    kept.quality.possible_duplicates.push(dup);
                }
            }
            record = kept;
        } else {
            record.links.discovered = links;
            for dup in old.quality.possible_duplicates {
                if !record.quality.possible_duplicates.contains(&dup) {
                    record.quality.possible_duplicates.push(dup);
                }
            }
        }
    }
    let json = json(&record)?;
    tx.execute(
        "INSERT INTO rema_mcp_jobs (id, record, schema_version, first_seen_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT (id) DO UPDATE SET record = excluded.record,
             schema_version = excluded.schema_version, updated_at = excluded.updated_at",
        params![id, json, SCHEMA_VERSION, now],
    )?;
    for key in keys(&record).into_iter().chain(known) {
        tx.execute(
            "INSERT OR IGNORE INTO rema_mcp_job_keys (key, job_id) VALUES (?1, ?2)",
            params![key, id],
        )?;
    }
    tx.commit()?;
    Ok(record)
}

/// Records a possible duplicate on both jobs (they stay separate).
pub fn mark_possible_duplicates(conn: &mut Connection, a: &str, b: &str) -> AppResult<()> {
    for (this, other) in [(a, b), (b, a)] {
        if let Some(mut record) = get(conn, this)? {
            if !record
                .quality
                .possible_duplicates
                .iter()
                .any(|d| d == other)
            {
                record.quality.possible_duplicates.push(other.to_string());
                conn.execute(
                    "UPDATE rema_mcp_jobs SET record = ?2 WHERE id = ?1",
                    params![this, json(&record)?],
                )?;
            }
        }
    }
    Ok(())
}

/// A stored search: its jobs in order, and what the envelope said.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub jobs: Vec<JobSummary>,
    pub unresolved: Vec<JobSummary>,
    pub envelope: serde_json::Value,
}

pub fn save_snapshot(
    conn: &Connection,
    id: &str,
    query_key: &str,
    snapshot: &Snapshot,
    now: i64,
    expires_at: i64,
) -> AppResult<()> {
    conn.execute(
        "DELETE FROM rema_mcp_snapshots WHERE expires_at < ?1",
        [now - 24 * 3_600_000],
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO rema_mcp_snapshots (id, query_key, created_at, expires_at, data)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, query_key, now, expires_at, json(snapshot)?],
    )?;
    Ok(())
}

/// (snapshot, query key, created, expires)
pub fn snapshot(conn: &Connection, id: &str) -> AppResult<Option<(Snapshot, String, i64, i64)>> {
    let row: Option<(String, String, i64, i64)> = conn
        .query_row(
            "SELECT data, query_key, created_at, expires_at FROM rema_mcp_snapshots WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(row.and_then(|(data, key, created, expires)| {
        serde_json::from_str(&data)
            .ok()
            .map(|s| (s, key, created, expires))
    }))
}

/// The newest unexpired snapshot for a query.
pub fn fresh_snapshot(conn: &Connection, query_key: &str, now: i64) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT id FROM rema_mcp_snapshots WHERE query_key = ?1 AND expires_at > ?2
             ORDER BY created_at DESC LIMIT 1",
            params![query_key, now],
            |r| r.get(0),
        )
        .optional()?)
}

/// Removes ReMa MCP's cached jobs and searches (nothing else).
pub fn clear(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "DELETE FROM rema_mcp_snapshots; DELETE FROM rema_mcp_job_keys; DELETE FROM rema_mcp_jobs;",
    )?;
    Ok(())
}

pub fn counts(conn: &Connection) -> AppResult<(i64, i64)> {
    let jobs = conn.query_row("SELECT COUNT(*) FROM rema_mcp_jobs", [], |r| r.get(0))?;
    let searches = conn.query_row("SELECT COUNT(*) FROM rema_mcp_snapshots", [], |r| r.get(0))?;
    Ok((jobs, searches))
}
