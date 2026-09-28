//! MCP (Model Context Protocol) client: ReMa connects to MCP servers the
//! user configured and makes their tools available to a model — only for
//! servers enabled in Settings, and only in chats where the user selected
//! them.
//!
//! ```text
//! Settings (configure, enable) ─ services::mcp ─ McpContext ─ client (rmcp) ─ server
//! Chat (+ menu selection) ─ services::chat_tools ─ McpContext::call ─┘
//! ```
//!
//! Connections are opened when first needed (a chat request, Connect or
//! Test in Settings) and kept while the server stays enabled. Disabling or
//! removing a server closes its connection; a local server's whole process
//! group stops with it. Nothing reconnects on its own after that. A
//! connection the server itself ended (its process exited, its event
//! stream closed) fails the calls that were waiting and is reopened on
//! the next use, with a short bounded backoff; the failed call is not
//! repeated.

pub mod client;
pub mod config;
pub mod import;
pub mod legacy_sse;
pub mod oauth;
#[cfg(test)]
mod schema_checks;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio_util::sync::CancellationToken;

use crate::models::mcp::{McpState, McpStatus};
use client::{ConnectError, Connection, ServerConfig};
use rmcp::transport::auth::CredentialStore;

/// The waits before each attempt to reopen a connection the server ended
/// (a first connection gets one immediate attempt). Stopped by
/// `disconnect` (disabling or removing the server) and at exit.
const RECONNECT_BACKOFF: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

struct Inner {
    live: Mutex<HashMap<i64, Arc<Connection>>>,
    status: Mutex<HashMap<i64, McpStatus>>,
    /// One connection attempt per server at a time.
    locks: Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
    /// Browser sign-ins in progress.
    sign_ins: Mutex<HashMap<i64, CancellationToken>>,
    /// Reconnections in progress (cancelled to stop their waits).
    reconnects: Mutex<HashMap<i64, CancellationToken>>,
    backoff: [Duration; 3],
}

impl Inner {
    fn new(backoff: [Duration; 3]) -> Self {
        Self {
            live: Mutex::default(),
            status: Mutex::default(),
            locks: Mutex::default(),
            sign_ins: Mutex::default(),
            reconnects: Mutex::default(),
            backoff,
        }
    }
}

/// Live MCP connections and their status. Cheap to clone.
#[derive(Clone)]
pub struct McpContext {
    inner: Arc<Inner>,
}

impl Default for McpContext {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner::new(RECONNECT_BACKOFF)),
        }
    }
}

impl McpContext {
    /// A context whose reconnections wait `backoff` (tests).
    #[cfg(test)]
    pub fn with_backoff(backoff: [Duration; 3]) -> Self {
        Self {
            inner: Arc::new(Inner::new(backoff)),
        }
    }

    /// The status shown for a server (`enabled` comes from its settings).
    pub fn status(&self, id: i64, enabled: bool) -> McpStatus {
        if !enabled {
            return McpStatus::of(McpState::Disabled);
        }
        if let Some(conn) = self.inner.live.lock().unwrap().get(&id) {
            if conn.is_closed() {
                return McpStatus {
                    message: Some("The server stopped. Connect again to use it.".into()),
                    ..McpStatus::of(McpState::Error)
                };
            }
        }
        self.inner
            .status
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .unwrap_or_else(|| McpStatus::of(McpState::Disconnected))
    }

    fn set_status(&self, id: i64, status: McpStatus) {
        self.inner.status.lock().unwrap().insert(id, status);
    }

    /// The live connection if it is still open.
    pub fn live(&self, id: i64) -> Option<Arc<Connection>> {
        let live = self.inner.live.lock().unwrap();
        live.get(&id).filter(|c| !c.is_closed()).cloned()
    }

    /// Reuses the open connection or opens one. `on_change` is called when
    /// the status changes (to notify the interface).
    pub async fn connect(
        &self,
        config: &ServerConfig,
        client_version: &str,
        oauth_store: Option<Arc<dyn CredentialStore>>,
        on_change: &(dyn Fn() + Send + Sync),
    ) -> Result<Arc<Connection>, ConnectError> {
        let lock = self
            .inner
            .locks
            .lock()
            .unwrap()
            .entry(config.id)
            .or_default()
            .clone();
        let _guard = lock.lock().await;
        if let Some(conn) = self.live(config.id) {
            return Ok(conn);
        }
        // A connection the server ended is reopened with patience; a
        // first connection (or Connect in Settings) gets one attempt.
        let lost = self.drop_connection(config.id);
        self.set_status(config.id, McpStatus::of(McpState::Connecting));
        on_change();
        let waits: &[Duration] = if lost {
            &self.inner.backoff
        } else {
            &[Duration::ZERO]
        };
        let cancel = CancellationToken::new();
        self.inner
            .reconnects
            .lock()
            .unwrap()
            .insert(config.id, cancel.clone());
        let mut result = Err(ConnectError::Failed("The server was not connected.".into()));
        for wait in waits {
            let stopped = tokio::select! {
                _ = cancel.cancelled() => true,
                _ = tokio::time::sleep(*wait) => false,
            };
            if stopped {
                // Disconnected (disabled, removed) meanwhile: its status
                // was cleared, and stays so.
                return Err(ConnectError::Failed("Reconnecting was stopped.".into()));
            }
            result = client::connect(config, client_version, oauth_store.clone()).await;
            match &result {
                // Signing in is up to the user; waiting does not help.
                Ok(_) | Err(ConnectError::NeedsSignIn(_)) => break,
                Err(ConnectError::Failed(_)) => {}
            }
        }
        self.inner.reconnects.lock().unwrap().remove(&config.id);
        match result {
            Ok(conn) => {
                let conn = Arc::new(conn);
                self.set_status(
                    config.id,
                    McpStatus {
                        state: McpState::Connected,
                        message: None,
                        tools: conn.tool_infos(),
                        protocol_version: conn.protocol_version.clone(),
                        server_info: conn.server_info.clone(),
                    },
                );
                self.inner
                    .live
                    .lock()
                    .unwrap()
                    .insert(config.id, conn.clone());
                on_change();
                Ok(conn)
            }
            Err(error) => {
                let state = match &error {
                    ConnectError::NeedsSignIn(_) => McpState::NeedsSignIn,
                    ConnectError::Failed(_) => McpState::Error,
                };
                self.set_status(
                    config.id,
                    McpStatus {
                        message: Some(error.to_string()),
                        ..McpStatus::of(state)
                    },
                );
                on_change();
                Err(error)
            }
        }
    }

    fn drop_connection(&self, id: i64) -> bool {
        match self.inner.live.lock().unwrap().remove(&id) {
            Some(conn) => {
                conn.close();
                true
            }
            None => false,
        }
    }

    /// Closes the connection (stopping a local server) and forgets its
    /// status.
    pub fn disconnect(&self, id: i64) {
        self.drop_connection(id);
        self.inner.status.lock().unwrap().remove(&id);
        if let Some(cancel) = self.inner.sign_ins.lock().unwrap().remove(&id) {
            cancel.cancel();
        }
        if let Some(cancel) = self.inner.reconnects.lock().unwrap().remove(&id) {
            cancel.cancel();
        }
    }

    /// Starts tracking a browser sign-in, cancelling an earlier one.
    pub fn begin_sign_in(&self, id: i64) -> CancellationToken {
        let cancel = CancellationToken::new();
        if let Some(previous) = self
            .inner
            .sign_ins
            .lock()
            .unwrap()
            .insert(id, cancel.clone())
        {
            previous.cancel();
        }
        cancel
    }

    pub fn end_sign_in(&self, id: i64) {
        self.inner.sign_ins.lock().unwrap().remove(&id);
    }

    pub fn cancel_sign_in(&self, id: i64) -> bool {
        match self.inner.sign_ins.lock().unwrap().remove(&id) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }

    pub fn signing_in(&self, id: i64) -> bool {
        self.inner.sign_ins.lock().unwrap().contains_key(&id)
    }

    /// Stops every connection (application exit).
    pub fn shutdown(&self) {
        for (_, conn) in self.inner.live.lock().unwrap().drain() {
            conn.close();
        }
        for (_, cancel) in self.inner.sign_ins.lock().unwrap().drain() {
            cancel.cancel();
        }
        for (_, cancel) in self.inner.reconnects.lock().unwrap().drain() {
            cancel.cancel();
        }
    }
}
