//! ReMa's own OAuth app registrations as this build received them
//! (`connectors.toml`, overridden by the environment; see `build.rs` and
//! [`super::build_config`]). Release builds always include Google and
//! Microsoft: the build fails without them.

include!(concat!(env!("OUT_DIR"), "/connector_apps.rs"));

/// What this build says about its Google app's publishing status.
pub fn google_in_testing() -> Option<bool> {
    match GOOGLE_PUBLISHING_STATUS {
        "testing" => Some(true),
        "production" => Some(false),
        _ => None,
    }
}

/// One line for the start-up log: which registrations this build has (never
/// their values).
pub fn summary() -> String {
    let state = |present: bool| if present { "ready" } else { "missing" };
    format!(
        "[connector] config google={} google_status={} microsoft={} linkedin={} \
         tenant={MICROSOFT_TENANT} source={}",
        state(GOOGLE_CLIENT_ID.is_some() && GOOGLE_CLIENT_SECRET.is_some()),
        if GOOGLE_PUBLISHING_STATUS.is_empty() {
            "unsaid"
        } else {
            GOOGLE_PUBLISHING_STATUS
        },
        state(MICROSOFT_CLIENT_ID.is_some()),
        if LINKEDIN_CLIENT_ID.is_some() {
            "ready"
        } else {
            "off"
        },
        SOURCE.replace(' ', "_"),
    )
}
