//! Notifications about important email, application and calendar changes:
//! stored in ReMa (the bell) and shown by the operating system.

use crate::{
    db::notifications::{self as repo, NewNotification},
    error::AppResult,
    models::jobs::NotificationItem,
    state::AppState,
    time::now_ms,
};

/// A notification to add.
pub struct Notice {
    pub kind: &'static str,
    pub title: String,
    pub body: String,
    pub application_id: Option<i64>,
    pub interview_id: Option<i64>,
    /// One notification per key (e.g. per email or per conflict).
    pub dedupe_key: Option<String>,
}

/// Adds a notification and shows it (once per `dedupe_key`). Failures are
/// logged, never fatal: a missed notification must not stop a sync.
pub fn add(state: &AppState, notice: Notice) {
    let inserted = state.db.call(|c| {
        repo::insert(
            c,
            &NewNotification {
                kind: notice.kind,
                title: &notice.title,
                body: &notice.body,
                application_id: notice.application_id,
                interview_id: notice.interview_id,
                dedupe_key: notice.dedupe_key.as_deref(),
                created_at: now_ms(),
            },
        )
    });
    match inserted {
        Ok(true) => {
            state.events.notify(&notice.title, &notice.body);
            state.events.notifications_changed();
        }
        Ok(false) => {}
        Err(error) => eprintln!("notification could not be stored: {error}"),
    }
}

pub fn list(state: &AppState) -> AppResult<Vec<NotificationItem>> {
    state.db.call(|c| repo::list(c, 100))
}

pub fn mark_read(state: &AppState, ids: Option<Vec<i64>>) -> AppResult<()> {
    state
        .db
        .call(|c| repo::mark_read(c, ids.as_deref(), now_ms()))?;
    state.events.notifications_changed();
    Ok(())
}

pub fn clear(state: &AppState) -> AppResult<()> {
    state.db.call(|c| repo::clear(c))?;
    state.events.notifications_changed();
    Ok(())
}
