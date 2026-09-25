//! Tauri command handlers.
//!
//! Commands are thin adapters: they extract state and arguments, delegate to
//! a service, and return `AppResult<T>`. No business logic lives here.
//!
//! Commands are `async` so they run off the main thread and never block the UI.

pub mod analytics;
pub mod browser;
pub mod chat;
pub mod google;
pub mod profile;
pub mod providers;
pub mod system;
pub mod tasks;
