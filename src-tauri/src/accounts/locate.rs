//! Finding the official command-line runtimes.
//!
//! ReMa ships Codex and the Anthropic CLI inside its app bundle (next to its
//! executable; development builds use `src-tauri/runtimes/`, filled by
//! `pnpm runtimes`). A copy installed on the computer is used too, and the
//! newest version wins, so updating either one is enough.
//!
//! Apps opened from the Finder, the Dock or the Start menu do not inherit
//! the terminal's `PATH`, so besides `PATH` ReMa looks in the folders the
//! usual installers use (Homebrew, npm, Volta, nvm, pnpm, Go, …) and, on
//! macOS and Linux, asks the login shell.

use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

/// Where a runtime is, and the folders its launcher may need on `PATH`
/// (npm's `codex` is a Node script that needs `node` next to it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub path: PathBuf,
    pub path_dirs: Vec<PathBuf>,
}

impl Located {
    pub fn at(path: PathBuf) -> Self {
        let path_dirs = path.parent().map(Path::to_path_buf).into_iter().collect();
        Self { path, path_dirs }
    }

    /// `PATH` for the runtime: its own folder first, then the inherited one.
    pub fn search_path(&self) -> std::ffi::OsString {
        let inherited = env::var_os("PATH").unwrap_or_default();
        let dirs = self
            .path_dirs
            .iter()
            .cloned()
            .chain(env::split_paths(&inherited))
            .collect::<Vec<_>>();
        env::join_paths(dirs).unwrap_or(inherited)
    }
}

/// A runtime version, e.g. `[0, 157, 1]`.
pub type Version = [u64; 3];

/// Finds `name`. An explicit path in `override_var` is used as is.
/// Otherwise the copy shipped with ReMa and one installed on the computer
/// (`PATH`, the usual install folders, the login shell) are compared, and
/// the newest wins; ReMa's own copy wins a tie.
pub async fn find(name: &str, override_var: &str) -> Option<(Located, Option<Version>)> {
    if let Some(path) = env::var_os(override_var).map(PathBuf::from) {
        if !is_executable(&path) {
            return None;
        }
        let located = Located::at(path);
        let version = version(&located).await;
        return Some((located, version));
    }
    let mut candidates: Vec<PathBuf> = bundled(name);
    let installed = match search(name, path_dirs().into_iter().chain(install_dirs())) {
        Some(path) => Some(path),
        None => login_shell_lookup(name).await,
    };
    candidates.extend(installed);
    candidates.dedup_by(|a, b| same_file(a, b));

    let mut best: Option<(Located, Option<Version>)> = None;
    for path in candidates {
        let located = Located::at(path);
        let version = version(&located).await;
        let newer = match &best {
            None => true,
            Some((_, current)) => version > *current,
        };
        if newer {
            best = Some((located, version));
        }
    }
    best
}

/// Finds an installed program by name (`PATH`, the usual install folders,
/// the login shell), e.g. the command of a local MCP server.
pub async fn which(name: &str) -> Option<Located> {
    if let Some(path) = search(name, path_dirs().into_iter().chain(install_dirs())) {
        return Some(Located::at(path));
    }
    login_shell_lookup(name).await.map(Located::at)
}

/// The inherited `PATH` plus the usual install folders, for programs that
/// start other programs (`npx` starts `node`).
pub fn extended_path(first: &[PathBuf]) -> std::ffi::OsString {
    let dirs: Vec<PathBuf> = first
        .iter()
        .cloned()
        .chain(path_dirs())
        .chain(install_dirs().into_iter().filter(|d| d.is_dir()))
        .collect();
    env::join_paths(dirs).unwrap_or_default()
}

/// Whether `path` is an executable file.
pub fn is_executable_file(path: &Path) -> bool {
    is_executable(path)
}

/// Copies shipped with ReMa: Tauri places bundled runtimes next to the app's
/// executable; development builds use the ones `pnpm runtimes` fetched.
fn bundled(name: &str) -> Vec<PathBuf> {
    let exe = if cfg!(windows) { ".exe" } else { "" };
    let mut paths = Vec::new();
    if let Some(dir) = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        paths.push(dir.join(format!("{name}{exe}")));
    }
    if cfg!(debug_assertions) {
        paths.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("runtimes")
                .join(format!("{name}-{}{exe}", env!("REMA_TARGET_TRIPLE"))),
        );
    }
    paths.into_iter().filter(|p| is_executable(p)).collect()
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Asks the runtime for its version (`codex-cli 0.157.1`, `ant version
/// 1.35.0`). `None` if it does not answer in time or prints no version.
pub async fn version(located: &Located) -> Option<Version> {
    let mut command = tokio::process::Command::new(&located.path);
    command
        .arg("--version")
        .env("PATH", located.search_path())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = tokio::time::timeout(Duration::from_secs(20), command.output())
        .await
        .ok()?
        .ok()?;
    parse_version(&String::from_utf8_lossy(&output.stdout))
}

/// The last `x.y.z` in the output; a leading `v` and pre-release or build
/// suffixes are ignored.
pub fn parse_version(text: &str) -> Option<Version> {
    text.split_whitespace().rev().find_map(|token| {
        let token = token.trim_start_matches('v');
        let mut parts = token.split(['.', '-', '+']).map(|p| p.parse::<u64>().ok());
        Some([parts.next()??, parts.next()??, parts.next()??])
    })
}

fn path_dirs() -> Vec<PathBuf> {
    env::var_os("PATH")
        .map(|p| env::split_paths(&p).collect())
        .unwrap_or_default()
}

/// Folders where installers put command-line tools, most specific first.
fn install_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    let mut dirs = Vec::new();
    if cfg!(windows) {
        if let Some(appdata) = env::var_os("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
        if let Some(local) = env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            dirs.push(local.join("Programs").join("codex"));
            dirs.push(local.join("pnpm"));
            dirs.push(local.join("Volta").join("bin"));
        }
        if let Some(home) = &home {
            dirs.push(home.join("go").join("bin"));
            dirs.push(home.join(".bun").join("bin"));
        }
        return dirs;
    }
    dirs.extend(
        [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/home/linuxbrew/.linuxbrew/bin",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    if let Some(home) = &home {
        for relative in [
            ".local/bin",
            "bin",
            ".npm-global/bin",
            ".volta/bin",
            ".bun/bin",
            "Library/pnpm",
            ".local/share/pnpm",
            "go/bin",
            ".cargo/bin",
        ] {
            dirs.push(home.join(relative));
        }
        dirs.extend(nvm_bin_dirs(&home.join(".nvm/versions/node")));
    }
    dirs
}

/// `~/.nvm/versions/node/*/bin`, newest version first.
fn nvm_bin_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut versions: Vec<(Vec<u64>, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let parts = name
                .trim_start_matches('v')
                .split('.')
                .map(|p| p.parse::<u64>().ok())
                .collect::<Option<Vec<_>>>()?;
            Some((parts, entry.path().join("bin")))
        })
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions.into_iter().map(|(_, dir)| dir).collect()
}

fn home_dir() -> Option<PathBuf> {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The first `dir/name` (with Windows launcher extensions) that exists.
fn search(name: &str, dirs: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        ["exe", "cmd", "bat"]
            .iter()
            .map(|ext| format!("{name}.{ext}"))
            .collect()
    } else {
        vec![name.to_string()]
    };
    dirs.into_iter()
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// Asks the user's login shell, which knows `PATH` changes from shell
/// profiles (e.g. a custom npm prefix). Bounded, and only the resolved path
/// is used.
async fn login_shell_lookup(name: &str) -> Option<PathBuf> {
    if cfg!(windows) || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let shell = env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())?;
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(shell)
            .args(["-lc", &format!("command -v {name}")])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    let found = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .rfind(|line| line.starts_with('/'))
        .map(PathBuf::from)?;
    is_executable(&found).then_some(found)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::state::testing::temp_dir;

    fn executable(dir: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn searches_folders_in_order_and_skips_non_executables() {
        let (a, b) = (temp_dir(), temp_dir());
        std::fs::write(a.join("codex"), "not executable").unwrap();
        let wanted = executable(&b, "codex");
        assert_eq!(search("codex", [a.clone(), b.clone()]), Some(wanted));
        assert_eq!(search("ant", [a, b]), None);
    }

    #[test]
    fn prefers_the_newest_nvm_node() {
        let root = temp_dir();
        for version in ["v18.20.1", "v22.3.0", "v20.11.0", "system"] {
            std::fs::create_dir_all(root.join(version).join("bin")).unwrap();
        }
        let dirs = nvm_bin_dirs(&root);
        assert_eq!(dirs.len(), 3);
        assert!(dirs[0].ends_with("v22.3.0/bin"));
        assert!(dirs[2].ends_with("v18.20.1/bin"));
    }

    #[test]
    fn reads_versions_from_runtime_output() {
        assert_eq!(parse_version("codex-cli 0.157.1\n"), Some([0, 157, 1]));
        assert_eq!(parse_version("ant version 1.35.0"), Some([1, 35, 0]));
        assert_eq!(parse_version("tool v2.0.3-beta.1"), Some([2, 0, 3]));
        assert_eq!(parse_version("no version here"), None);
        assert!(Some([0, 158, 0]) > Some([0, 157, 9]));
        assert!(
            Some([0, 1, 0]) > None,
            "a known version beats an unknown one"
        );
    }

    #[tokio::test]
    async fn reads_the_version_of_each_copy() {
        let (old, new) = (temp_dir(), temp_dir());
        let script = |dir: &Path, version: &str| {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join("codex");
            std::fs::write(&path, format!("#!/bin/sh\necho codex-cli {version}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let older = script(&old, "0.150.0");
        let newer = script(&new, "0.160.2");
        let located = Located::at(newer.clone());
        assert_eq!(version(&located).await, Some([0, 160, 2]));
        assert_eq!(version(&Located::at(older)).await, Some([0, 150, 0]));
    }

    #[test]
    fn puts_the_runtime_folder_first_on_path() {
        let located = Located::at(PathBuf::from("/opt/tools/bin/codex"));
        let path = located.search_path();
        let first = env::split_paths(&path).next().unwrap();
        assert_eq!(first, PathBuf::from("/opt/tools/bin"));
    }
}
