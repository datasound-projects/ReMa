//! Release checks (Spec B §80) that run with every `cargo test`, so that a
//! release cannot fail on what a plain CI build never exercises: the Tauri
//! packages of the frontend and the app agree (the Tauri CLI refuses to
//! package otherwise), the backend opens the system browser through the
//! opener plugin, and no webview may open addresses itself. The connector
//! registrations are checked by `build.rs` (see `connectors::build_config`).

use std::path::Path;

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// "2.12.0" or "^2.12.0" → (2, 12).
fn major_minor(version: &str) -> (u64, u64) {
    let mut parts = version
        .trim_start_matches(['^', '~', '='])
        .split('.')
        .map(|p| p.parse::<u64>().unwrap());
    (parts.next().unwrap(), parts.next().unwrap())
}

#[test]
fn the_tauri_npm_packages_match_the_rust_crate() {
    let lock = std::fs::read_to_string(manifest_dir().join("Cargo.lock")).unwrap();
    let crate_version = lock
        .split("[[package]]")
        .find(|p| p.contains("\nname = \"tauri\"\n"))
        .and_then(|p| p.lines().find_map(|l| l.strip_prefix("version = \"")))
        .map(|v| v.trim_end_matches('"').to_string())
        .expect("tauri in Cargo.lock");
    let package: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_dir().join("../package.json")).unwrap(),
    )
    .unwrap();
    for (section, name) in [
        ("dependencies", "@tauri-apps/api"),
        ("devDependencies", "@tauri-apps/cli"),
    ] {
        let npm = package[section][name].as_str().expect(name);
        assert_eq!(
            major_minor(npm),
            major_minor(&crate_version),
            "{name} {npm} must match the tauri crate {crate_version}, or `tauri build` refuses to package"
        );
    }
}

#[test]
fn the_opener_plugin_is_registered_and_no_webview_can_open_addresses() {
    let lib = include_str!("lib.rs");
    assert!(lib.contains(".plugin(tauri_plugin_opener::init())"));
    let mut files = 0;
    for entry in std::fs::read_dir(manifest_dir().join("capabilities")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "json") {
            files += 1;
            let capability: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let permissions = capability["permissions"].to_string();
            assert!(
                !permissions.contains("opener:") && !permissions.contains("shell:"),
                "{}: {permissions}",
                path.display()
            );
        }
    }
    assert!(files >= 1);
}
