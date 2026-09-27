//! Connector metadata: accounts, per-connector choices and sync state,
//! incremental sync cursors. Tokens are never stored here.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::{
    error::AppResult,
    models::connectors::{ConnectorErrorCode, ConnectorId, ProviderId},
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
    pub last_sync_started_at: Option<i64>,
    pub last_sync_completed_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_error_detail: Option<String>,
    /// Set when the error came from a connection check (not a sync).
    pub last_error_code: Option<ConnectorErrorCode>,
}

fn connector_from_row(row: &Row) -> rusqlite::Result<ConnectorRecord> {
    let id: String = row.get(0)?;
    let code: Option<String> = row.get(7)?;
    Ok(ConnectorRecord {
        id: ConnectorId::parse(&id).unwrap_or(ConnectorId::Gmail),
        enabled: row.get(1)?,
        last_sync_started_at: row.get(2)?,
        last_sync_completed_at: row.get(3)?,
        last_success_at: row.get(4)?,
        last_error: row.get(5)?,
        last_error_detail: row.get(6)?,
        last_error_code: code.as_deref().and_then(ConnectorErrorCode::parse),
    })
}

// Mail is read only by the built-in task "Job Mail & Interview Sync"; the
// `background_sync` and `next_sync_at` columns are no longer used.
const CONNECTOR_COLUMNS: &str = "id, enabled, last_sync_started_at, last_sync_completed_at,
    last_success_at, last_error, last_error_detail, last_error_code";

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
             last_error = CASE WHEN ?2 = 1 THEN last_error ELSE NULL END,
             last_error_detail = CASE WHEN ?2 = 1 THEN last_error_detail ELSE NULL END,
             last_error_code = CASE WHEN ?2 = 1 THEN last_error_code ELSE NULL END
         WHERE id = ?1",
        params![id.as_str(), enabled, now],
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

/// Records the end of a sync (a task run that used this connector).
pub fn sync_finished(
    conn: &Connection,
    id: ConnectorId,
    now: i64,
    error: Option<(&str, &str)>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE connectors SET last_sync_completed_at = ?2,
             last_success_at = CASE WHEN ?3 IS NULL THEN ?2 ELSE last_success_at END,
             last_error = ?3, last_error_detail = ?4, last_error_code = NULL
         WHERE id = ?1",
        params![id.as_str(), now, error.map(|e| e.0), error.map(|e| e.1)],
    )?;
    Ok(())
}

/// Forgets the last error (after a reconnect) without claiming a sync: the
/// last success is where reading resumes if a sync position expires.
pub fn clear_error(conn: &Connection, id: ConnectorId) -> AppResult<()> {
    conn.execute(
        "UPDATE connectors SET last_error = NULL, last_error_detail = NULL,
             last_error_code = NULL
         WHERE id = ?1",
        [id.as_str()],
    )?;
    Ok(())
}

/// Records why the connection check after a sign-in failed.
pub fn set_check_error(
    conn: &Connection,
    id: ConnectorId,
    code: ConnectorErrorCode,
    message: &str,
    detail: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE connectors SET last_error = ?2, last_error_detail = ?3, last_error_code = ?4
         WHERE id = ?1",
        params![id.as_str(), message, detail, code.as_str()],
    )?;
    Ok(())
}

pub fn clear_sync_state(conn: &Connection, id: ConnectorId) -> AppResult<()> {
    conn.execute(
        "UPDATE connectors SET last_sync_started_at = NULL, last_sync_completed_at = NULL,
             last_success_at = NULL, last_error = NULL, last_error_detail = NULL,
             last_error_code = NULL
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
            assert!(gmail.enabled);
            assert_eq!(gmail.last_sync_started_at, None, "connecting reads no mail");

            save_cursor(c, ProviderId::Google, "sub-1", "mail:inbox", "123", 1)?;
            save_cursor(c, ProviderId::Google, "sub-1", "mail:inbox", "456", 2)?;
            assert_eq!(
                cursor(c, ProviderId::Google, "sub-1", "mail:inbox")?.as_deref(),
                Some("456")
            );
            delete_account(c, ProviderId::Google)?;
            assert!(cursor(c, ProviderId::Google, "sub-1", "mail:inbox")?.is_none());
            Ok(())
        })
        .unwrap();
    }
}
