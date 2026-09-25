//! Finding the official command-line runtimes on this computer.
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

/// Finds `name`: `override_var` (an explicit path) first, then `PATH`, the
/// usual install folders and the login shell.
pub async fn find(name: &str, override_var: &str) -> Option<Located> {
    if let Some(path) = env::var_os(override_var).map(PathBuf::from) {
        return is_executable(&path).then(|| Located::at(path));
    }
    if let Some(path) = search(name, path_dirs().into_iter().chain(install_dirs())) {
        return Some(Located::at(path));
    }
    login_shell_lookup(name).await.map(Located::at)
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
    fn puts_the_runtime_folder_first_on_path() {
        let located = Located::at(PathBuf::from("/opt/tools/bin/codex"));
        let path = located.search_path();
        let first = env::split_paths(&path).next().unwrap();
        assert_eq!(first, PathBuf::from("/opt/tools/bin"));
    }
}
