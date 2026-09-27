//! First run and every start (§7, §58, §81). ReMa's career search needs no
//! setup and has no wizard: this checks that its parts are in place, moves
//! an older search-service setting out of the way, and learns what the
//! connected models can do — without running a search.

use crate::{
    db::providers as settings,
    error::AppResult,
    retrieval::backend::{ServiceKind, KIND_KEY, URL_KEY},
    state::AppState,
};

use super::{capabilities, registry, status};

/// Brings career search up: router, source registry, discovery providers,
/// page fetcher, extractor, citations, caches, source health and the local
/// model tools are part of ReMa and need nothing from the user.
pub async fn run(state: &AppState) {
    eprintln!("{}", summary());
    match migrate_search_service(state) {
        Ok(Some(note)) => eprintln!("[career-search] migration {note}"),
        Ok(None) => {}
        Err(error) => eprintln!("[career-search] migration skipped: {error}"),
    }
    // Local servers first (whether one is Unsloth Studio), then every
    // connected provider's runtime.
    status::detect_capabilities(state).await;
    let Ok(rows) = state.db.call(|c| crate::db::providers::list(c)) else {
        return;
    };
    for row in rows {
        capabilities::provision(state, &row.id).await;
    }
}

/// What is ready, in one diagnostic line (no user data).
pub fn summary() -> String {
    let sources = registry::SOURCES.iter().filter(|s| s.enabled).count();
    format!(
        "[career-search] bootstrap router=ready registry_sources={sources} discovery={} \
         fetcher=public-addresses-only extractor=ready citations=ready health=ready \
         local_tools={}",
        super::discovery::provider_names().join(","),
        crate::retrieval::tools::specs()
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// §81: an older search-service setting (none, Brave, Tavily, SearXNG) no
/// longer decides whether ReMa searches. A service that is still valid stays
/// as an optional extra source under Advanced, with its key where it is (the
/// OS credential store). A value that names no service ("none", "", an old
/// name) is cleared. Nothing else is touched, no secret is deleted.
pub fn migrate_search_service(state: &AppState) -> AppResult<Option<String>> {
    let kind = state.db.call(|c| settings::get_setting(c, KIND_KEY))?;
    let Some(kind) = kind else {
        return Ok(None);
    };
    if ServiceKind::parse(kind.trim()).is_some() {
        return Ok(Some(format!(
            "optional_service={} kept_as_advanced=true",
            kind.trim()
        )));
    }
    state.db.call(|c| {
        settings::delete_setting(c, KIND_KEY)?;
        settings::delete_setting(c, URL_KEY)
    })?;
    Ok(Some("optional_service=none cleared=true".into()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing};

    #[tokio::test]
    async fn an_old_search_setting_never_decides_whether_remas_search_runs() {
        let (state, _) = testing::state(Arc::new(FakeLanguageModel::replying(&[])));
        // Nothing set: nothing to do.
        assert_eq!(migrate_search_service(&state).unwrap(), None);

        // "None" from an older version is cleared; no service is left.
        state
            .db
            .call(|c| settings::set_setting(c, KIND_KEY, "none"))
            .unwrap();
        assert_eq!(
            migrate_search_service(&state).unwrap().as_deref(),
            Some("optional_service=none cleared=true")
        );
        assert!(state
            .db
            .call(|c| settings::get_setting(c, KIND_KEY))
            .unwrap()
            .is_none());

        // Brave with its key stays an optional extra; the key is untouched.
        state
            .db
            .call(|c| settings::set_setting(c, KIND_KEY, "brave"))
            .unwrap();
        state
            .vault
            .set_text(crate::retrieval::backend::SECRET, "brave-key")
            .await
            .unwrap();
        assert_eq!(
            migrate_search_service(&state).unwrap().as_deref(),
            Some("optional_service=brave kept_as_advanced=true")
        );
        assert_eq!(
            state
                .vault
                .get_text(crate::retrieval::backend::SECRET)
                .await
                .unwrap()
                .as_deref(),
            Some("brave-key")
        );
    }

    #[test]
    fn the_bootstrap_line_names_the_parts_and_no_user_data() {
        let line = summary();
        assert!(line.starts_with("[career-search] bootstrap router=ready"));
        assert!(line.contains("duckduckgo"));
        assert!(line.contains("local_tools=rema_career_search,rema_read_page"));
    }
}
