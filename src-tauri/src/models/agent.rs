//! ReMa Agents: named instructions that shape how the selected model
//! answers. Stored locally; an agent is not a separate AI service.

use serde::{Deserialize, Serialize};
use specta::Type;

/// An agent, built into ReMa or created by the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    /// `builtin:<slug>` or `custom:<number>`.
    pub id: String,
    pub name: String,
    pub description: String,
    /// The fixed instructions added to requests when the agent is selected.
    pub instructions: String,
    /// One of `AGENT_ICONS`.
    pub icon: String,
    /// Built-in agents cannot be edited or deleted; duplicate them instead.
    pub builtin: bool,
    /// Custom agents: when last saved.
    pub updated_at: Option<i64>,
}

/// What the user enters for a custom agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentInput {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub icon: String,
}

/// Icons an agent can show (names of interface icons).
pub const AGENT_ICONS: &[&str] = &[
    "spark",
    "search",
    "match",
    "document",
    "interview",
    "strategy",
    "research",
    "target",
    "compass",
    "chat",
    "book",
    "code",
];

/// Agents were created, changed or deleted.
#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct AgentsChanged;
