//! Connector metadata: accounts, per-connector choices and sync state,
//! incremental sync cursors. Tokens are never stored here.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    db::providers as settings,
    error::AppResult,
    models::connectors::{ConnectorId, ConnectorPreferences, InterviewMode, ProviderId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    Connected,
    ReauthRequired,
}

crate::models::text_enum!(AccountStatus {
    Connected => "connected",
    ReauthRequired => "reauth_required",
});

/// A connected provider account (`ProviderAccount`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRecord {
    pub provider: ProviderId,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub granted_scopes: Vec<String>,
    pub status: AccountStatus,
    pub status_reason: Option<String>,
    pub connected_at: i64,
    pub updated_at: i64,
}

fn account_from_row(row: &Row) -> rusqlite::Result<AccountRecord> {
    let provider: String = row.get(0)?;
    let scopes: String = row.get(4)?;
    let status: String = row.get(5)?;
    Ok(AccountRecord {
        provider: ProviderId::parse(&provider).unwrap_or(ProviderId::Google),
        account_id: row.get(1)?,
        email: row.get(2)?,
        display_name: row.get(3)?,
        granted_scopes: scopes.split_whitespace().map(str::to_string).collect(),
        status: AccountStatus::parse(&status).unwrap_or(AccountStatus::ReauthRequired),
        status_reason: row.get(6)?,
        connected_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

const ACCOUNT_COLUMNS: &str = "provider, account_id, email, display_name, granted_scopes, status,
    status_reason, connected_at, updated_at";

pub fn account(conn: &Connection, provider: ProviderId) -> AppResult<Option<AccountRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {ACCOUNT_COLUMNS} FROM connector_accounts WHERE provider = ?1"),
            [provider.as_str()],
            account_from_row,
        )
        .optional()?)
}

pub fn save_account(conn: &Connection, account: &AccountRecord) -> AppResult<()> {
    conn.execute(
        "INSERT INTO connector_accounts (provider, account_id, email, display_name, granted_scopes,
             status, status_reason, connected_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (provider) DO UPDATE SET
             account_id = excluded.account_id, email = excluded.email,
             display_name = excluded.display_name, granted_scopes = excluded.granted_scopes,
             status = excluded.status, status_reason = excluded.status_reason,
             connected_at = excluded.connected_at, updated_at = excluded.updated_at",
        params![
            account.provider.as_str(),
            account.account_id,
            account.email,
            account.display_name,
            account.granted_scopes.join(" "),
            account.status.as_str(),
            account.status_reason,
            account.connected_at,
            account.updated_at,
        ],
    )?;
    Ok(())
}

pub fn set_account_status(
    conn: &Connection,
    provider: ProviderId,
    status: AccountStatus,
    reason: Option<&str>,
    now: i64,
) -> AppResult<()> {
    conn.execute(
        "UPDATE connector_accounts SET status = ?2, status_reason = ?3, updated_at = ?4
         WHERE provider = ?1",
        params![provider.as_str(), status.as_str(), reason, now],
    )?;
    Ok(())
}

pub fn delete_account(conn: &Connection, provider: ProviderId) -> AppResult<()> {
    conn.execute(
        "DELETE FROM connector_accounts WHERE provider = ?1",
        [provider.as_str()],
    )?;
    conn.execute(
        "DELETE FROM sync_cursors WHERE provider = ?1",
        [provider.as_str()],
    )?;
    Ok(())
}

/// A connector's choice and sync state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorRecord {
    pub id: ConnectorId,
    pub enabled: bool,
    pub background_sync: bool,
    pub last_sync_started_at: Option<i64>,
    pub last_sync_completed_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_error_detail: Option<String>,
    pub next_sync_at: Option<i64>,
}

fn connector_from_row(row: &Row) -> rusqlite::Result<ConnectorRecord> {
    let id: String = row.get(0)?;
    Ok(ConnectorRecord {
        id: ConnectorId::parse(&id).unwrap_or(ConnectorId::Gmail),
        enabled: row.get(1)?,
        background_sync: row.get(2)?,
        last_sync_started_at: row.get(3)?,
        last_sync_completed_at: row.get(4)?,
        last_success_at: row.get(5)?,
        last_error: row.get(6)?,
        last_error_detail: row.get(7)?,
        next_sync_at: row.get(8)?,
    })
}

const CONNECTOR_COLUMNS: &str = "id, enabled, background_sync, last_sync_started_at,
    last_sync_completed_at, last_success_at, last_error, last_error_detail, next_sync_at";

pub fn connector(conn: &Connection, id: ConnectorId) -> AppResult<ConnectorRecord> {
    Ok(conn.query_row(
        &format!("SELECT {CONNECTOR_COLUMNS} FROM connectors WHERE id = ?1"),
        [id.as_str()],
        connector_from_row,
    )?)
}

pub fn connectors(conn: &Connection) -> AppResult<Vec<ConnectorRecord>> {
    ConnectorId::ALL
        .iter()
        .map(|id| connector(conn, *id))
        .collect()
}

pub fn set_enabled(conn: &Connection, id: ConnectorId, enabled: bool, now: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE connectors SET enabled = ?2, updated_at = ?3,
             next_sync_at = CASE WHEN ?2 = 1 THEN ?3 ELSE NULL END,
             last_error = CASE WHEN ?2 = 1 THEN last_error ELSE NULL END,
             last_error_detail = CASE WHEN ?2 = 1 THEN last_error_detail ELSE NULL END
         WHERE id = ?1",
        params![id.as_str(), enabled, now],
    )?;
    Ok(())
}

pub fn set_background_sync(conn: &Connection, id: ConnectorId, on: bool) -> AppResult<()> {
    conn.execute(
        "UPDATE connectors SET background_sync = ?2 WHERE id = ?1",
        params![id.as_str(), on],
    )?;
    Ok(())
}

pub fn sync_started(conn: &Connection, id: ConnectorId, now: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE connectors SET last_sync_started_at = ?2 WHERE id = ?1",
        params![id.as_str(), now],
    )?;
    Ok(())
}

/// Records the end of a sync and when the next one is due.
pub fn sync_finished(
    conn: &Connection,
    id: ConnectorId,
    now: i64,
    error: Option<(&str, &str)>,
    next_sync_at: Option<i64>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE connectors SET last_sync_completed_at = ?2,
             last_success_at = CASE WHEN ?3 IS NULL THEN ?2 ELSE last_success_at END,
             last_error = ?3, last_error_detail = ?4, next_sync_at = ?5
         WHERE id = ?1",
        params![
            id.as_str(),
            now,
            error.map(|e| e.0),
            error.map(|e| e.1),
            next_sync_at
        ],
    )?;
    Ok(())
}

pub fn schedule_sync(conn: &Connection, id: ConnectorId, at: Option<i64>) -> AppResult<()> {
    conn.execute(
        "UPDATE connectors SET next_sync_at = ?2 WHERE id = ?1",
        params![id.as_str(), at],
    )?;
    Ok(())
}

pub fn clear_sync_state(conn: &Connection, id: ConnectorId) -> AppResult<()> {
    conn.execute(
        "UPDATE connectors SET last_sync_started_at = NULL, last_sync_completed_at = NULL,
             last_success_at = NULL, last_error = NULL, last_error_detail = NULL,
             next_sync_at = NULL
         WHERE id = ?1",
        [id.as_str()],
    )?;
    Ok(())
}

// ── Cursors ─────────────────────────────────────────────────────────

pub fn cursor(
    conn: &Connection,
    provider: ProviderId,
    account_id: &str,
    resource: &str,
) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT cursor FROM sync_cursors WHERE provider = ?1 AND account_id = ?2 AND resource = ?3",
            params![provider.as_str(), account_id, resource],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn save_cursor(
    conn: &Connection,
    provider: ProviderId,
    account_id: &str,
    resource: &str,
    cursor: &str,
    now: i64,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO sync_cursors (provider, account_id, resource, cursor, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (provider, account_id, resource) DO UPDATE SET
             cursor = excluded.cursor, updated_at = excluded.updated_at",
        params![provider.as_str(), account_id, resource, cursor, now],
    )?;
    Ok(())
}

pub fn delete_cursors(conn: &Connection, provider: ProviderId, prefix: &str) -> AppResult<()> {
    conn.execute(
        "DELETE FROM sync_cursors WHERE provider = ?1 AND resource LIKE ?2 || '%'",
        params![provider.as_str(), prefix],
    )?;
    Ok(())
}

// ── Preferences ─────────────────────────────────────────────────────

const KEY_INTERVIEW_MODE: &str = "connectors.interview_mode";
const KEY_SYNC_INTERVAL: &str = "connectors.sync_interval_minutes";
const KEY_PREP_BUFFER: &str = "connectors.prep_buffer_minutes";
pub const DEFAULT_SYNC_INTERVAL: u32 = 15;

pub fn preferences(conn: &Connection) -> AppResult<ConnectorPreferences> {
    let number = |key: &str, default: u32| -> AppResult<u32> {
        Ok(settings::get_setting(conn, key)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(default))
    };
    Ok(ConnectorPreferences {
        interview_mode: settings::get_setting(conn, KEY_INTERVIEW_MODE)?
            .as_deref()
            .and_then(InterviewMode::parse)
            .unwrap_or(InterviewMode::Ask),
        sync_interval_minutes: number(KEY_SYNC_INTERVAL, DEFAULT_SYNC_INTERVAL)?,
        prep_buffer_minutes: number(KEY_PREP_BUFFER, 0)?,
    })
}

pub fn save_preferences(conn: &Connection, prefs: &ConnectorPreferences) -> AppResult<()> {
    settings::set_setting(conn, KEY_INTERVIEW_MODE, prefs.interview_mode.as_str())?;
    settings::set_setting(
        conn,
        KEY_SYNC_INTERVAL,
        &prefs.sync_interval_minutes.to_string(),
    )?;
    settings::set_setting(
        conn,
        KEY_PREP_BUFFER,
        &prefs.prep_buffer_minutes.to_string(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn stores_accounts_connectors_and_cursors() {
        let db = Database::open_in_memory().unwrap();
        db.call(|c| {
            assert!(account(c, ProviderId::Google)?.is_none());
            let record = AccountRecord {
                provider: ProviderId::Google,
                account_id: Some("sub-1".into()),
                email: Some("me@example.com".into()),
                display_name: Some("Ana".into()),
                granted_scopes: vec!["openid".into(), "email".into()],
                status: AccountStatus::Connected,
                status_reason: None,
                connected_at: 1,
                updated_at: 1,
            };
            save_account(c, &record)?;
            assert_eq!(account(c, ProviderId::Google)?, Some(record));

            assert!(!connector(c, ConnectorId::Gmail)?.enabled);
            set_enabled(c, ConnectorId::Gmail, true, 5)?;
            let gmail = connector(c, ConnectorId::Gmail)?;
            assert!(gmail.enabled && gmail.background_sync);
            assert_eq!(gmail.next_sync_at, Some(5), "a new connector syncs at once");

            save_cursor(c, ProviderId::Google, "sub-1", "mail:inbox", "123", 1)?;
            save_cursor(c, ProviderId::Google, "sub-1", "mail:inbox", "456", 2)?;
            assert_eq!(
                cursor(c, ProviderId::Google, "sub-1", "mail:inbox")?.as_deref(),
                Some("456")
            );
            delete_account(c, ProviderId::Google)?;
            assert!(cursor(c, ProviderId::Google, "sub-1", "mail:inbox")?.is_none());

            let prefs = preferences(c)?;
            assert_eq!(prefs.interview_mode, InterviewMode::Ask, "ask by default");
            assert_eq!(prefs.sync_interval_minutes, DEFAULT_SYNC_INTERVAL);
            Ok(())
        })
        .unwrap();
    }
}
