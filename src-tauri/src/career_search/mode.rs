//! How chats and scheduled tasks answer questions that need the web
//! (Settings → Career Search).
//!
//! By default a model whose provider hosts a web search (OpenAI, ChatGPT
//! through Codex, Anthropic, Gemini 3) searches itself and writes the
//! answer, as in ChatGPT and Claude; ReMa's job tools (ReMa MCP) sit next
//! to its search. Models without a search of their own (local servers,
//! Gemini before 3, a provider that refused its search) get ReMa's
//! search-first answers. The verified mode keeps ReMa's search-first
//! answers for every model: ReMa searches, opens and checks each posting,
//! and the model writes about what was found.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{db::providers as settings, error::AppResult, state::AppState};

const KEY: &str = "career_search.answer_mode";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AnswerMode {
    /// The model searches the web with its provider's own search.
    #[default]
    ModelSearch,
    /// ReMa searches and checks every posting first.
    Verified,
}

pub fn get(state: &AppState) -> AnswerMode {
    match state
        .db
        .call(|c| settings::get_setting(c, KEY))
        .ok()
        .flatten()
        .as_deref()
    {
        Some("verified") => AnswerMode::Verified,
        _ => AnswerMode::ModelSearch,
    }
}

pub fn set(state: &AppState, mode: AnswerMode) -> AppResult<()> {
    let value = match mode {
        AnswerMode::ModelSearch => "model_search",
        AnswerMode::Verified => "verified",
    };
    state.db.call(|c| settings::set_setting(c, KEY, value))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing};

    #[test]
    fn the_models_own_search_is_the_default_and_the_choice_is_kept() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        assert_eq!(get(&state), AnswerMode::ModelSearch);
        set(&state, AnswerMode::Verified).unwrap();
        assert_eq!(get(&state), AnswerMode::Verified);
        set(&state, AnswerMode::ModelSearch).unwrap();
        assert_eq!(get(&state), AnswerMode::ModelSearch);
    }
}
