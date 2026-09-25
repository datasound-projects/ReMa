//! SQLite persistence.
//!
//! One connection guarded by a mutex: ReMa is a single-user desktop app and
//! every query is short, so this is simpler and faster than a pool. Callers
//! hold the lock only for the duration of `Database::call`.

pub mod analytics;
pub mod conversations;
pub mod jobs;
pub mod profile;
pub mod providers;
pub mod tasks;

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// Ordered schema migrations. Never edit a shipped migration; append a new one.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_initial.sql"),
    include_str!("migrations/0002_google_jobs.sql"),
    include_str!("migrations/0003_profile.sql"),
    include_str!("migrations/0004_job_analytics.sql"),
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

fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let current = usize::try_from(current).unwrap_or(0);
    if current > MIGRATIONS.len() {
        return Err(AppError::database(format!(
            "database schema version {current} is newer than this version of ReMa supports"
        )));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
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
