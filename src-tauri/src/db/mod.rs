//! SQLite persistence.
//!
//! One connection guarded by a mutex: ReMa is a single-user desktop app and
//! every query is short, so this is simpler and faster than a pool. Callers
//! hold the lock only for the duration of `Database::call`.

pub mod agents;
pub mod analytics;
pub mod connectors;
pub mod conversations;
pub mod jobs;
pub mod mcp;
pub mod notifications;
pub mod portfolio;
pub mod profile;
pub mod providers;
pub mod tasks;

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

/// Ordered schema migrations. Never edit a shipped migration; append a new one.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_initial.sql"),
    include_str!("migrations/0002_google_jobs.sql"),
    include_str!("migrations/0003_profile.sql"),
    include_str!("migrations/0004_job_analytics.sql"),
    include_str!("migrations/0005_provider_connections.sql"),
    include_str!("migrations/0006_profile_agents_mcp.sql"),
    include_str!("migrations/0007_rema_mcp.sql"),
    include_str!("migrations/0008_connectors.sql"),
    include_str!("migrations/0009_job_mail_sync.sql"),
];

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Opens (or creates) the database file and applies pending migrations.
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    /// In-memory database, used by tests.
    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> AppResult<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Runs `f` with exclusive access to the connection.
    pub fn call<T>(&self, f: impl FnOnce(&mut Connection) -> AppResult<T>) -> AppResult<T> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| AppError::internal("database lock poisoned"))?;
        f(&mut conn)
    }
}

/// Applies pending migrations. Foreign keys are off meanwhile (SQLite's
/// procedure for rebuilding a table others refer to) and every migration is
/// checked for broken references before it commits.
fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let current = usize::try_from(current).unwrap_or(0);
    if current > MIGRATIONS.len() {
        return Err(AppError::database(format!(
            "database schema version {current} is newer than this version of ReMa supports"
        )));
    }
    if current == MIGRATIONS.len() {
        return Ok(());
    }
    let enforced: bool = conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let result = (|| {
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            let broken: Option<String> = tx
                .query_row(
                    "SELECT \"table\" FROM pragma_foreign_key_check LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(table) = broken {
                return Err(AppError::database(format!(
                    "migration {} left broken references in {table}",
                    index + 1
                )));
            }
            tx.pragma_update(None, "user_version", (index + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    })();
    if enforced {
        conn.pragma_update(None, "foreign_keys", "ON")?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_all_migrations_once() {
        let db = Database::open_in_memory().unwrap();
        let version: i64 = db
            .call(|c| Ok(c.pragma_query_value(None, "user_version", |r| r.get(0))?))
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());

        // Re-running is a no-op.
        db.call(migrate).unwrap();
    }

    #[test]
    fn keeps_references_when_rebuilding_profile_documents() {
        // A database as ReMa 0006 found it: a document used by a custom field
        // and a certificate in the library.
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        conn.execute_batch(
            "INSERT INTO profile_documents
                 (id, name, kind, format, original_name, file_name, size, sha256, text, created_at, updated_at)
             VALUES (7, 'Old CV', 'cv', 'pdf', 'old.pdf', 'a.pdf', 10, 'x', 'text', 1, 1),
                    (8, 'New CV', 'cv', 'docx', 'new.docx', 'b.docx', 10, 'y', NULL, 2, 2),
                    (9, 'AWS', 'certificate', 'png', 'aws.png', 'c.png', 10, 'z', NULL, 3, 3);
             INSERT INTO profile_fields (position, label, kind, value, document_id)
             VALUES (0, 'Certificate', 'file', '', 9);",
        )
        .unwrap();

        migrate(&mut conn).unwrap();
        let db = Database {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.call(|c| {
            let field: Option<i64> =
                c.query_row("SELECT document_id FROM profile_fields", [], |r| r.get(0))?;
            assert_eq!(field, Some(9), "the custom field still points at its file");
            let primary: i64 = c.query_row(
                "SELECT id FROM profile_documents WHERE is_primary = 1",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(primary, 8, "the newest CV is primary");
            let credential: (String, i64) = c.query_row(
                "SELECT title, document_id FROM credentials",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(credential, ("AWS".into(), 9));
            let enforced: bool = c.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
            assert!(enforced, "foreign keys are back on");
            // WEBP is accepted now.
            c.execute(
                "INSERT INTO profile_documents
                     (name, kind, format, original_name, file_name, size, sha256, created_at, updated_at)
                 VALUES ('Badge', 'certificate', 'webp', 'b.webp', 'd.webp', 1, 'w', 4, 4)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn one_mail_task_becomes_job_mail_sync_and_nothing_else_reads_mail() {
        // A database as ReMa 0008 found it: two mail tasks, a prompt task and
        // a mail connector syncing in the background.
        let mut conn = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..8] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 8).unwrap();
        conn.execute_batch(
            r#"INSERT INTO scheduled_tasks
                 (id, name, prompt, provider_id, model_id, schedule, timezone, start_at, status,
                  next_run_at, created_at, updated_at, kind)
             VALUES
                 (3, 'Mail monitor', '', 'p', 'm', '{"kind":"daily","every":1}', 'UTC', 0, 'active', 100, 0, 0,
                  '{"type":"job_applications","lookbackDays":30,"syncCalendar":true}'),
                 (4, 'Old mail monitor', '', 'p', 'm', '{"kind":"daily","every":1}', 'UTC', 0, 'active', 100, 0, 0,
                  '{"type":"job_applications","lookbackDays":7,"syncCalendar":false}'),
                 (5, 'Morning digest', 'Summarize', 'p', 'm', '{"kind":"daily","every":1}', 'UTC', 0, 'active', 100, 0, 0,
                  '{"type":"prompt"}');
             UPDATE connectors SET enabled = 1, background_sync = 1, next_sync_at = 5 WHERE id = 'gmail';"#,
        )
        .unwrap();

        migrate(&mut conn).unwrap();
        type Row = (i64, String, Option<String>, bool, Option<i64>);
        let tasks: Vec<Row> = conn
            .prepare(
                "SELECT id, name, builtin, enabled, next_run_at FROM scheduled_tasks ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            tasks,
            [
                (
                    3,
                    "Job Mail & Interview Sync".into(),
                    Some("job_mail_sync".into()),
                    true,
                    Some(100)
                ),
                (4, "Old mail monitor".into(), None, false, None),
                (5, "Morning digest".into(), None, true, Some(100)),
            ]
        );
        let gmail: (bool, Option<i64>) = conn
            .query_row(
                "SELECT background_sync, next_sync_at FROM connectors WHERE id = 'gmail'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            gmail,
            (false, None),
            "connectors no longer read mail on their own"
        );
    }

    #[test]
    fn enforces_foreign_keys() {
        let db = Database::open_in_memory().unwrap();
        let result = db.call(|c| {
            c.execute(
                "INSERT INTO messages (conversation_id, role, content, status, created_at)
                 VALUES (999, 'user', 'hi', 'complete', 0)",
                [],
            )?;
            Ok(())
        });
        assert!(result.is_err());
    }
}
