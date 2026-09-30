//! ReMa's public OAuth client configuration, resolved when ReMa is built.
//!
//! `build.rs` includes this file: it reads `connectors.toml` and the
//! environment, checks every value, fails a release build that lacks the
//! Google or Microsoft registration, and compiles the result into the app
//! (`connectors::config`). The app's tests include it too.
//!
//! Desktop apps are public OAuth clients (RFC 8252): everything here ships in
//! every copy of ReMa and none of it is a secret. That includes the Google
//! "Desktop app" client secret, which Google does not treat as confidential
//! for installed apps and marks optional at its token endpoint: a build may
//! leave it out, and ReMa then sends no `client_secret` at all.

/// One setting: its place in `connectors.toml` and the environment variable
/// that overrides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub table: &'static str,
    pub name: &'static str,
    pub env: &'static str,
}

pub const GOOGLE_CLIENT_ID: Key = Key {
    table: "google",
    name: "desktop_client_id",
    env: "GOOGLE_DESKTOP_CLIENT_ID",
};
/// Optional: Google's token endpoint accepts native clients without it.
pub const GOOGLE_CLIENT_SECRET: Key = Key {
    table: "google",
    name: "desktop_client_secret",
    env: "GOOGLE_DESKTOP_CLIENT_SECRET",
};
/// Whether the Google app is in Testing (Google ends every sign-in about 7
/// days after it is made) or in production: "testing", "production" or
/// empty (not said).
pub const GOOGLE_PUBLISHING_STATUS: Key = Key {
    table: "google",
    name: "publishing_status",
    env: "GOOGLE_PUBLISHING_STATUS",
};
pub const MICROSOFT_CLIENT_ID: Key = Key {
    table: "microsoft",
    name: "public_client_id",
    env: "MICROSOFT_PUBLIC_CLIENT_ID",
};
pub const MICROSOFT_TENANT: Key = Key {
    table: "microsoft",
    name: "tenant",
    env: "MICROSOFT_TENANT",
};
pub const LINKEDIN_CLIENT_ID: Key = Key {
    table: "linkedin",
    name: "client_id",
    env: "LINKEDIN_CLIENT_ID",
};
pub const LINKEDIN_APPROVED_SCOPES: Key = Key {
    table: "linkedin",
    name: "approved_scopes",
    env: "LINKEDIN_APPROVED_SCOPES",
};

/// Every key (`build.rs` iterates them).
#[allow(dead_code)]
pub const KEYS: [Key; 7] = [
    GOOGLE_CLIENT_ID,
    GOOGLE_CLIENT_SECRET,
    GOOGLE_PUBLISHING_STATUS,
    MICROSOFT_CLIENT_ID,
    MICROSOFT_TENANT,
    LINKEDIN_CLIENT_ID,
    LINKEDIN_APPROVED_SCOPES,
];

/// Restricted LinkedIn scopes ReMa knows how to use.
pub const LINKEDIN_KNOWN_SCOPES: [&str; 1] = ["r_1st_connections"];

/// Microsoft authorities that accept accounts from any organization and/or
/// personal Microsoft accounts. A single tenant would lock every other
/// user out, so release builds accept only these.
pub const MULTI_TENANT_AUTHORITIES: [&str; 3] = ["common", "organizations", "consumers"];

/// The configuration compiled into ReMa.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectorConfig {
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    /// "testing", "production" or "" (not said).
    pub google_publishing_status: String,
    pub microsoft_client_id: Option<String>,
    pub microsoft_tenant: String,
    pub linkedin_client_id: Option<String>,
    pub linkedin_approved_scopes: String,
    /// Settings a debug build is missing (a release build fails instead).
    pub missing: Vec<&'static str>,
    /// Where the values came from: the file, the environment, or both.
    pub source: &'static str,
}

/// Resolves the configuration: a non-empty environment variable wins over
/// `connectors.toml`. In a release build Google (client ID; the Desktop
/// client's secret is optional) and Microsoft (client ID, multi-tenant
/// authority) are mandatory; a malformed value fails every build. Error
/// messages name settings by their key, never by their value.
pub fn resolve(
    file: &dyn Fn(Key) -> Option<String>,
    env: &dyn Fn(&str) -> Option<String>,
    release: bool,
) -> Result<ConnectorConfig, Vec<String>> {
    let mut errors = Vec::new();
    let (mut from_file, mut from_env) = (false, false);
    let mut value = |key: Key| -> Option<String> {
        let clean = |v: String| Some(v.trim().to_string()).filter(|v| !v.is_empty());
        if let Some(v) = env(key.env).and_then(clean) {
            from_env = true;
            return Some(v);
        }
        let v = file(key).and_then(clean);
        from_file |= v.is_some();
        v
    };
    let google_client_id = value(GOOGLE_CLIENT_ID);
    let google_client_secret = value(GOOGLE_CLIENT_SECRET);
    let google_publishing_status = value(GOOGLE_PUBLISHING_STATUS)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let microsoft_client_id = value(MICROSOFT_CLIENT_ID);
    let microsoft_tenant = value(MICROSOFT_TENANT).unwrap_or_else(|| "common".to_string());
    let linkedin_client_id = value(LINKEDIN_CLIENT_ID);
    let linkedin_approved_scopes = value(LINKEDIN_APPROVED_SCOPES).unwrap_or_default();

    let describe = |key: Key| format!("{} ([{}] {})", key.env, key.table, key.name);
    let mut check = |key: Key, value: &Option<String>, valid: fn(&str) -> bool, what: &str| {
        if let Some(v) = value {
            if !valid(v) {
                errors.push(format!("{} is not {what}", describe(key)));
            }
        }
    };
    check(
        GOOGLE_CLIENT_ID,
        &google_client_id,
        valid_google_client_id,
        "a Google OAuth client ID (…apps.googleusercontent.com)",
    );
    check(
        GOOGLE_CLIENT_SECRET,
        &google_client_secret,
        valid_google_client_secret,
        "a Google Desktop client secret",
    );
    check(
        MICROSOFT_CLIENT_ID,
        &microsoft_client_id,
        valid_guid,
        "a Microsoft application (client) ID (a GUID)",
    );
    check(
        LINKEDIN_CLIENT_ID,
        &linkedin_client_id,
        valid_linkedin_client_id,
        "a LinkedIn client ID",
    );
    if !["", "testing", "production"].contains(&google_publishing_status.as_str()) {
        errors.push(format!(
            "{} must be testing or production (or empty when not known)",
            describe(GOOGLE_PUBLISHING_STATUS)
        ));
    }
    if !valid_tenant(&microsoft_tenant) {
        errors.push(format!(
            "{} must be common, organizations, consumers, a tenant ID or a tenant domain",
            describe(MICROSOFT_TENANT)
        ));
    } else if release && !MULTI_TENANT_AUTHORITIES.contains(&microsoft_tenant.as_str()) {
        errors.push(format!(
            "{} is \"{microsoft_tenant}\": a release build must accept every organization and \
             personal accounts (use \"common\")",
            describe(MICROSOFT_TENANT)
        ));
    }
    for scope in linkedin_approved_scopes.split_whitespace() {
        if !LINKEDIN_KNOWN_SCOPES.contains(&scope) {
            errors.push(format!(
                "{} lists \"{scope}\", which ReMa does not use",
                describe(LINKEDIN_APPROVED_SCOPES)
            ));
        }
    }
    if google_client_id.is_none() && google_client_secret.is_some() {
        errors.push(format!(
            "{} is set without {}: the secret belongs to a Desktop client ID",
            describe(GOOGLE_CLIENT_SECRET),
            describe(GOOGLE_CLIENT_ID)
        ));
    }

    // The Google client secret is optional, so it is never "missing".
    let mut missing = Vec::new();
    for (key, present) in [
        (GOOGLE_CLIENT_ID, google_client_id.is_some()),
        (MICROSOFT_CLIENT_ID, microsoft_client_id.is_some()),
    ] {
        if !present {
            if release {
                errors.push(format!(
                    "{} is missing: release builds must include ReMa's Google and Microsoft \
                     registrations",
                    describe(key)
                ));
            } else {
                missing.push(key.env);
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(ConnectorConfig {
        google_client_id,
        google_client_secret,
        google_publishing_status,
        microsoft_client_id,
        microsoft_tenant,
        linkedin_client_id,
        linkedin_approved_scopes,
        missing,
        source: match (from_file, from_env) {
            (true, true) => "connectors.toml and the environment",
            (false, true) => "the environment",
            (true, false) => "connectors.toml",
            (false, false) => "nothing",
        },
    })
}

/// The Rust source compiled into ReMa (`connectors::config`).
#[allow(dead_code)]
pub fn to_rust(config: &ConnectorConfig) -> String {
    let opt = |v: &Option<String>| match v {
        Some(v) => format!("Some({v:?})"),
        None => "None".to_string(),
    };
    format!(
        "// Generated by build.rs from connectors.toml and the environment.\n\
         // Public OAuth client configuration: nothing here is a secret.\n\
         pub const GOOGLE_CLIENT_ID: Option<&str> = {};\n\
         pub const GOOGLE_CLIENT_SECRET: Option<&str> = {};\n\
         pub const GOOGLE_PUBLISHING_STATUS: &str = {:?};\n\
         pub const MICROSOFT_CLIENT_ID: Option<&str> = {};\n\
         pub const MICROSOFT_TENANT: &str = {:?};\n\
         pub const LINKEDIN_CLIENT_ID: Option<&str> = {};\n\
         pub const LINKEDIN_APPROVED_SCOPES: &str = {:?};\n\
         pub const SOURCE: &str = {:?};\n",
        opt(&config.google_client_id),
        opt(&config.google_client_secret),
        config.google_publishing_status,
        opt(&config.microsoft_client_id),
        config.microsoft_tenant,
        opt(&config.linkedin_client_id),
        config.linkedin_approved_scopes,
        config.source,
    )
}

#[allow(dead_code)]
pub fn valid_google_client_id(v: &str) -> bool {
    v.len() <= 200
        && v.strip_suffix(".apps.googleusercontent.com")
            .is_some_and(|prefix| {
                !prefix.is_empty()
                    && prefix
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
}

#[allow(dead_code)]
pub fn valid_google_client_secret(v: &str) -> bool {
    (8..=128).contains(&v.len()) && v.chars().all(|c| c.is_ascii_graphic() && c != '"')
}

#[allow(dead_code)]
pub fn valid_guid(v: &str) -> bool {
    v.len() == 36
        && v.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn valid_tenant(v: &str) -> bool {
    MULTI_TENANT_AUTHORITIES.contains(&v)
        || valid_guid(v)
        || (v.contains('.')
            && v.len() <= 253
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'))
}

#[allow(dead_code)]
pub fn valid_linkedin_client_id(v: &str) -> bool {
    (8..=64).contains(&v.len()) && v.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const GOOGLE_ID: &str = "123456789012-abcdefghijklmnop.apps.googleusercontent.com";
    const GOOGLE_SECRET: &str = "GOCSPX-desktop-not-confidential";
    const MICROSOFT_ID: &str = "0b1c2d3e-4f50-4617-8a9b-c0d1e2f3a4b5";

    fn values(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    fn run(
        file: &HashMap<&'static str, String>,
        env: &HashMap<&'static str, String>,
        release: bool,
    ) -> Result<ConnectorConfig, Vec<String>> {
        resolve(
            &|key| file.get(key.env).cloned(),
            &|name| env.get(name).cloned(),
            release,
        )
    }

    fn complete() -> HashMap<&'static str, String> {
        values(&[
            ("GOOGLE_DESKTOP_CLIENT_ID", GOOGLE_ID),
            ("GOOGLE_DESKTOP_CLIENT_SECRET", GOOGLE_SECRET),
            ("MICROSOFT_PUBLIC_CLIENT_ID", MICROSOFT_ID),
        ])
    }

    #[test]
    fn a_release_build_without_google_or_microsoft_fails() {
        let errors = run(&HashMap::new(), &HashMap::new(), true).unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        for name in ["GOOGLE_DESKTOP_CLIENT_ID", "MICROSOFT_PUBLIC_CLIENT_ID"] {
            assert!(
                errors
                    .iter()
                    .any(|e| e.starts_with(name) && e.contains("missing")),
                "{name}: {errors:?}"
            );
        }
        // Blank values count as missing.
        let blank = values(&[("GOOGLE_DESKTOP_CLIENT_ID", "  ")]);
        assert!(run(&blank, &HashMap::new(), true).is_err());
        let config = run(&complete(), &HashMap::new(), true).unwrap();
        assert_eq!(config.google_client_id.as_deref(), Some(GOOGLE_ID));
        assert_eq!(config.microsoft_tenant, "common");
        assert!(config.missing.is_empty());
        assert_eq!(config.source, "connectors.toml");
    }

    #[test]
    fn a_development_build_without_them_names_what_is_missing() {
        let config = run(&HashMap::new(), &HashMap::new(), false).unwrap();
        assert_eq!(
            config.missing,
            ["GOOGLE_DESKTOP_CLIENT_ID", "MICROSOFT_PUBLIC_CLIENT_ID"]
        );
        assert_eq!(config.google_client_id, None);
        assert_eq!(config.source, "nothing");
    }

    #[test]
    fn the_environment_overrides_the_file_and_every_value_is_checked() {
        let env = values(&[(
            "MICROSOFT_PUBLIC_CLIENT_ID",
            "ffffffff-0000-4000-8000-000000000001",
        )]);
        let config = run(&complete(), &env, true).unwrap();
        assert_eq!(
            config.microsoft_client_id.as_deref(),
            Some("ffffffff-0000-4000-8000-000000000001")
        );
        assert_eq!(config.source, "connectors.toml and the environment");

        for (name, bad) in [
            ("GOOGLE_DESKTOP_CLIENT_ID", "my-client-id"),
            ("GOOGLE_DESKTOP_CLIENT_ID", "a b.apps.googleusercontent.com"),
            ("GOOGLE_DESKTOP_CLIENT_SECRET", "short"),
            ("MICROSOFT_PUBLIC_CLIENT_ID", "not-a-guid"),
            ("MICROSOFT_TENANT", "contoso tenant"),
            ("LINKEDIN_CLIENT_ID", "abc"),
            (
                "LINKEDIN_APPROVED_SCOPES",
                "r_1st_connections r_fullprofile",
            ),
        ] {
            let env = values(&[(name, bad)]);
            // Malformed values fail development builds too.
            let errors = run(&complete(), &env, false).unwrap_err();
            assert!(
                errors.iter().any(|e| e.starts_with(name)),
                "{name}: {errors:?}"
            );
        }
    }

    #[test]
    fn the_google_desktop_client_secret_is_optional() {
        // A release with the client ID but no secret builds: Google's token
        // endpoint accepts native clients without one.
        let file = values(&[
            ("GOOGLE_DESKTOP_CLIENT_ID", GOOGLE_ID),
            ("MICROSOFT_PUBLIC_CLIENT_ID", MICROSOFT_ID),
        ]);
        let config = run(&file, &HashMap::new(), true).unwrap();
        assert_eq!(config.google_client_id.as_deref(), Some(GOOGLE_ID));
        assert_eq!(config.google_client_secret, None);
        assert!(config.missing.is_empty());
        assert!(to_rust(&config).contains("pub const GOOGLE_CLIENT_SECRET: Option<&str> = None;"));
        // A secret without a client ID is a mistake, named by its key only.
        let orphan = values(&[
            ("GOOGLE_DESKTOP_CLIENT_SECRET", GOOGLE_SECRET),
            ("MICROSOFT_PUBLIC_CLIENT_ID", MICROSOFT_ID),
        ]);
        let errors = run(&orphan, &HashMap::new(), false).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.starts_with("GOOGLE_DESKTOP_CLIENT_SECRET")),
            "{errors:?}"
        );
        assert!(
            errors.iter().all(|e| !e.contains(GOOGLE_SECRET)),
            "no value is printed"
        );
        // Missing-setting messages name the key, never a value.
        let errors = run(&HashMap::new(), &HashMap::new(), true).unwrap_err();
        assert!(errors.iter().all(|e| e.contains("is missing")));
    }

    #[test]
    fn a_release_accepts_every_organization_and_personal_accounts() {
        let single = values(&[("MICROSOFT_TENANT", "0b1c2d3e-4f50-4617-8a9b-c0d1e2f3a4b5")]);
        let errors = run(&complete(), &single, true).unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("every organization")),
            "{errors:?}"
        );
        // A development build may use one tenant (a test directory).
        assert!(run(&complete(), &single, false).is_ok());
        for tenant in MULTI_TENANT_AUTHORITIES {
            let env = values(&[("MICROSOFT_TENANT", tenant)]);
            assert!(run(&complete(), &env, true).is_ok(), "{tenant}");
        }
    }

    #[test]
    fn the_compiled_source_holds_the_values_as_string_literals() {
        let mut config = run(&complete(), &HashMap::new(), true).unwrap();
        config.linkedin_approved_scopes = "r_1st_connections".into();
        let source = to_rust(&config);
        assert!(source.contains(&format!(
            "pub const GOOGLE_CLIENT_ID: Option<&str> = Some(\"{GOOGLE_ID}\");"
        )));
        assert!(source.contains("pub const LINKEDIN_CLIENT_ID: Option<&str> = None;"));
        assert!(source.contains("pub const MICROSOFT_TENANT: &str = \"common\";"));
        assert!(
            source.contains("pub const LINKEDIN_APPROVED_SCOPES: &str = \"r_1st_connections\";")
        );
    }

    #[test]
    fn the_google_apps_publishing_status_is_testing_production_or_unsaid() {
        for (given, kept) in [
            ("Testing", "testing"),
            ("production", "production"),
            ("", ""),
        ] {
            let mut env = complete();
            env.insert("GOOGLE_PUBLISHING_STATUS", given.into());
            let config = run(&HashMap::new(), &env, true).unwrap();
            assert_eq!(config.google_publishing_status, kept);
            assert!(to_rust(&config).contains(&format!(
                "pub const GOOGLE_PUBLISHING_STATUS: &str = {kept:?};"
            )));
        }
        let mut env = complete();
        env.insert("GOOGLE_PUBLISHING_STATUS", "beta".into());
        let errors = run(&HashMap::new(), &env, true).unwrap_err();
        assert!(errors[0].contains("GOOGLE_PUBLISHING_STATUS"), "{errors:?}");
    }

    #[test]
    fn the_committed_file_names_every_setting() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/connectors.toml"))
            .unwrap();
        let table: toml::Table = text.parse().unwrap();
        for key in KEYS {
            let value = table
                .get(key.table)
                .and_then(|t| t.get(key.name))
                .unwrap_or_else(|| panic!("[{}] {} is missing", key.table, key.name));
            assert!(value.is_str(), "[{}] {}", key.table, key.name);
            assert!(text.contains(key.env), "{} is documented", key.env);
        }
        // Whatever it holds is well formed.
        let file = |key: Key| {
            table
                .get(key.table)
                .and_then(|t| t.get(key.name))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        resolve(&file, &|_| None, false).unwrap();
    }
}
