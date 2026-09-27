//! Tauri command handlers.
//!
//! Commands are thin adapters: they extract state and arguments, delegate to
//! a service, and return `AppResult<T>`. No business logic lives here.
//!
//! Commands are `async` so they run off the main thread and never block the UI.

pub mod agents;
pub mod analytics;
pub mod applications;
pub mod browser;
pub mod business;
pub mod chat;
pub mod connectors;
pub mod mcp;
pub mod network;
pub mod portfolio;
pub mod profile;
pub mod providers;
pub mod rema_mcp;
pub mod system;
pub mod tasks;
pub mod websearch;
