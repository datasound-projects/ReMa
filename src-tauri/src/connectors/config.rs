//! ReMa's own OAuth app registrations as this build received them
//! (`connectors.toml`, overridden by the environment; see `build.rs` and
//! [`super::build_config`]). Release builds always include Google and
//! Microsoft: the build fails without them.

include!(concat!(env!("OUT_DIR"), "/connector_apps.rs"));

/// One line for the start-up log: which registrations this build has (never
/// their values).
pub fn summary() -> String {
    let state = |present: bool| if present { "ready" } else { "missing" };
    format!(
        "[connector] config google={} microsoft={} linkedin={} tenant={MICROSOFT_TENANT} source={}",
        state(GOOGLE_CLIENT_ID.is_some() && GOOGLE_CLIENT_SECRET.is_some()),
        state(MICROSOFT_CLIENT_ID.is_some()),
        if LINKEDIN_CLIENT_ID.is_some() {
            "ready"
        } else {
            "off"
        },
        SOURCE.replace(' ', "_"),
    )
}
