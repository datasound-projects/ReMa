//! IPC surface: the list of commands exposed to the frontend and the
//! generator for their TypeScript bindings (`src/generated/bindings.ts`).
//!
//! Rust is the source of truth. After changing a command or a model, the
//! bindings are regenerated automatically by `pnpm tauri dev`, or explicitly
//! with `pnpm bindings`. `cargo test` fails if the committed file is stale.

use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

use specta_typescript::Typescript;
use tauri_specta::{collect_commands, Builder, ErrorHandlingMode};

use crate::commands;

/// Path of the generated bindings, relative to this crate.
pub const BINDINGS_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../src/generated/bindings.ts");

const HEADER: &str = "// Source of truth: the Rust commands and models in src-tauri.\n// Regenerate with `pnpm bindings` (or run `pnpm tauri dev`).";

/// Every command the frontend may call. Register new commands here.
pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![commands::system::get_app_status])
        // Failed commands reject the promise; `src/services/ipc.ts` turns the
        // rejection into an `ApiError`.
        .error_handling(ErrorHandlingMode::Throw)
}

/// Renders the TypeScript bindings for the given builder.
pub fn render_bindings(builder: &Builder<tauri::Wry>) -> Result<String, String> {
    // Unique per call: tests may render concurrently.
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rema-bindings-{}-{id}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("bindings.ts");
    builder
        .export(Typescript::default().header(HEADER), &file)
        .map_err(|e| e.to_string())?;
    let rendered = fs::read_to_string(&file).map_err(|e| e.to_string());
    let _ = fs::remove_dir_all(&dir);
    rendered
}

/// Writes the bindings to [`BINDINGS_PATH`], only if their content changed
/// (so the Vite dev server doesn't reload needlessly).
pub fn export_bindings(builder: &Builder<tauri::Wry>) -> Result<(), String> {
    let rendered = render_bindings(builder)?;
    let path = Path::new(BINDINGS_PATH);
    if fs::read_to_string(path).ok().as_deref() == Some(rendered.as_str()) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, rendered).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_bindings_are_up_to_date() {
        let rendered = render_bindings(&builder()).expect("render bindings");
        let committed = fs::read_to_string(BINDINGS_PATH).unwrap_or_default();

        assert!(
            committed == rendered,
            "src/generated/bindings.ts is out of date. Run `pnpm bindings` and commit the result."
        );
    }

    /// Run via `pnpm bindings`.
    #[test]
    #[ignore = "writes src/generated/bindings.ts; run explicitly with `pnpm bindings`"]
    fn export() {
        export_bindings(&builder()).expect("export bindings");
    }
}
