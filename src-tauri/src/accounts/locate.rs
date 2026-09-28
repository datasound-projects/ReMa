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
//! macOS and Linux, asks the login shell for its environment once per run.
//! On Windows programs are resolved with `PATH` and `PATHEXT` only; no
//! shell is involved in finding anything.

use std::{
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    sync::RwLock,
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
/// (`PATH`, the usual install folders, the login shell's `PATH`) are
/// compared, and the newest wins; ReMa's own copy wins a tie.
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
        None => login_path_lookup(name).await,
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
/// the login shell's `PATH`), e.g. the command of a local MCP server.
pub async fn which(name: &str) -> Option<Located> {
    if let Some(path) = search(name, path_dirs().into_iter().chain(install_dirs())) {
        return Some(Located::at(path));
    }
    login_path_lookup(name).await.map(Located::at)
}

/// Where the user's shell would find `name` (an MCP server's program): the
/// login shell's `PATH` first, so the version a version manager such as nvm
/// selects wins over a system or Homebrew copy, as in a terminal; then
/// ReMa's own `PATH` and the usual install folders.
pub async fn which_as_shell(name: &str) -> Option<Located> {
    let login = login_environment().await;
    let dirs = shell_search_dirs(login.get(OsStr::new("PATH")).map(|p| p.as_os_str()));
    search(name, dirs).map(Located::at)
}

/// The folders a login shell searches, then ReMa's own and the usual install
/// folders.
fn shell_search_dirs(login_path: Option<&OsStr>) -> Vec<PathBuf> {
    login_path
        .map(|p| env::split_paths(p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(path_dirs())
        .chain(install_dirs())
        .collect()
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

/// Like [`extended_path`], with the login shell's `PATH` right after
/// `first` (see [`login_environment`]).
pub fn extended_path_with(first: &[PathBuf], login_path: Option<&OsStr>) -> OsString {
    let dirs: Vec<PathBuf> = first
        .iter()
        .cloned()
        .chain(
            login_path
                .map(|p| env::split_paths(p).collect::<Vec<_>>())
                .unwrap_or_default(),
        )
        .chain(path_dirs())
        .chain(install_dirs().into_iter().filter(|d| d.is_dir()))
        .fold(Vec::new(), |mut seen, dir| {
            if !seen.contains(&dir) {
                seen.push(dir);
            }
            seen
        });
    env::join_paths(dirs).unwrap_or_default()
}

/// Variables of the shell session itself, not of the user's setup, and the
/// two that carry ReMa's markers into the shell.
const SHELL_SESSION_VARS: &[&str] = &[
    "PWD",
    "OLDPWD",
    "SHLVL",
    "_",
    "PS1",
    "PS2",
    MARKER_BEGIN_VAR,
    MARKER_END_VAR,
];

/// How long a login shell may take to print its environment. Profiles that
/// wait for a network mount, a password prompt or a stuck `ssh-add` would
/// otherwise hold every MCP server and runtime lookup.
const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(10);

/// The environment variables through which the shell learns the two marker
/// lines, so the command it runs is the same fixed text every time.
const MARKER_BEGIN_VAR: &str = "REMA_ENV_BEGIN";
const MARKER_END_VAR: &str = "REMA_ENV_END";

/// The one command a POSIX shell (and fish, which reads `||` and `"$VAR"`
/// the same way) runs: the begin marker on a line of its own, the
/// environment (NUL-separated where `env -0` exists, otherwise one line per
/// variable), the end marker. Nothing is ever spliced into this text.
const POSIX_ENV_COMMAND: &str = "printf '\\n%s\\n' \"$REMA_ENV_BEGIN\"; \
     command env -0 || command env; \
     printf '\\n%s\\n' \"$REMA_ENV_END\"";

/// The same for csh and tcsh, which have no `command` builtin.
const CSH_ENV_COMMAND: &str = "printf '\\n%s\\n' \"$REMA_ENV_BEGIN\"; \
     /usr/bin/env -0 || /usr/bin/env; \
     printf '\\n%s\\n' \"$REMA_ENV_END\"";

/// The login environment read this run. It is handed out as a `'static`
/// reference (callers keep and iterate it), so a refresh leaks the previous
/// map: a few kilobytes, only when the user asks for a refresh.
static LOGIN_ENV: RwLock<Option<&'static HashMap<OsString, OsString>>> = RwLock::new(None);

/// Serializes readers so one run asks the shell once, not once per caller.
static LOGIN_ENV_READ: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The environment the user's login shell sets up (variables exported in
/// shell profiles: API keys, proxies, version managers, `PATH`), asked once
/// per run, as VS Code and Zed do. Apps opened from the Finder, the Dock or
/// a desktop launcher do not inherit it. Empty on Windows (programs there
/// get the user's environment from the system) or when the shell does not
/// answer in time, in which case ReMa's own environment is all there is.
/// Never blocks the caller's thread: the shell runs as a Tokio process.
pub async fn login_environment() -> &'static HashMap<OsString, OsString> {
    if let Some(env) = *LOGIN_ENV.read().unwrap_or_else(|e| e.into_inner()) {
        return env;
    }
    let _reading = LOGIN_ENV_READ.lock().await;
    // Another caller may have finished while this one waited for the lock.
    if let Some(env) = *LOGIN_ENV.read().unwrap_or_else(|e| e.into_inner()) {
        return env;
    }
    store_login_environment(read_login_environment().await)
}

/// Asks the login shell again (after the user edited a profile or installed
/// a version manager) and replaces the cached environment for this run.
pub async fn refresh_login_environment() -> &'static HashMap<OsString, OsString> {
    let _reading = LOGIN_ENV_READ.lock().await;
    store_login_environment(read_login_environment().await)
}

fn store_login_environment(
    env: HashMap<OsString, OsString>,
) -> &'static HashMap<OsString, OsString> {
    let env: &'static HashMap<OsString, OsString> = Box::leak(Box::new(env));
    *LOGIN_ENV.write().unwrap_or_else(|e| e.into_inner()) = Some(env);
    env
}

async fn read_login_environment() -> HashMap<OsString, OsString> {
    if cfg!(windows) {
        return HashMap::new();
    }
    let Some(shell) = user_shell().await else {
        return HashMap::new();
    };
    ask_shell(&shell, LOGIN_SHELL_TIMEOUT)
        .await
        .unwrap_or_default()
}

/// Runs `shell` as a login and interactive shell and reads what it exports.
/// The markers are random for this call, so nothing a profile prints
/// (Powerlevel10k, conda, a message of the day) can be mistaken for the
/// environment; only the text between the marker lines is read. `None`
/// when the shell is of an unknown kind, fails, or does not finish within
/// `timeout`, in which case it is killed and a warning is logged.
async fn ask_shell(shell: &Path, timeout: Duration) -> Option<HashMap<OsString, OsString>> {
    let args = shell_arguments(shell)?;
    let nonce = {
        let mut bytes = [0u8; 16];
        // Without system randomness the markers are still unique per run
        // (nothing a profile prints can predict them either way).
        let _ = getrandom::fill(&mut bytes);
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let begin = format!("__REMA_ENV_BEGIN_{nonce}__");
    let end = format!("__REMA_ENV_END_{nonce}__");
    let mut command = tokio::process::Command::new(shell);
    command
        .args(&args)
        .env(MARKER_BEGIN_VAR, &begin)
        .env(MARKER_END_VAR, &end)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        // Dropping the unfinished `output()` future (on timeout) kills the
        // shell, so a stuck profile does not linger.
        .kill_on_drop(true);
    match tokio::time::timeout(timeout, command.output()).await {
        Ok(Ok(output)) => Some(parse_env_block(&output.stdout, &begin, &end)),
        Ok(Err(error)) => {
            eprintln!(
                "locate: could not run the login shell {}: {error}; using ReMa's own environment",
                shell.display()
            );
            None
        }
        Err(_) => {
            eprintln!(
                "locate: the login shell {} did not print its environment within {}s and was \
                 stopped; using ReMa's own environment (Settings can ask it again)",
                shell.display(),
                timeout.as_secs()
            );
            None
        }
    }
}

/// The user's shell: `SHELL`, or (for an app started without it, as at
/// login) the shell in the user's account record.
async fn user_shell() -> Option<PathBuf> {
    if let Some(shell) = env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Some(shell);
    }
    let run = |program: &'static str, args: Vec<String>| async move {
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .ok()?
        .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    let user = match env::var("USER").or_else(|_| env::var("LOGNAME")) {
        Ok(user) if !user.is_empty() => user,
        _ => run("/usr/bin/id", vec!["-un".into()]).await?,
    };
    let shell = if cfg!(target_os = "macos") {
        let record = run(
            "/usr/bin/dscl",
            vec![
                ".".into(),
                "-read".into(),
                format!("/Users/{user}"),
                "UserShell".into(),
            ],
        )
        .await;
        // macOS's default since Catalina, if the record cannot be read.
        record
            .as_deref()
            .and_then(shell_from_dscl)
            .unwrap_or_else(|| PathBuf::from("/bin/zsh"))
    } else {
        shell_from_passwd(&run("getent", vec!["passwd".into(), user]).await?)?
    };
    Some(shell).filter(|p| p.is_absolute())
}

/// `UserShell: /bin/zsh` from `dscl . -read /Users/<name> UserShell`.
fn shell_from_dscl(record: &str) -> Option<PathBuf> {
    record
        .lines()
        .find_map(|l| l.trim().strip_prefix("UserShell:"))
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.is_absolute())
}

/// The last field of a `passwd` entry.
fn shell_from_passwd(entry: &str) -> Option<PathBuf> {
    let shell = entry.lines().next()?.rsplit(':').next()?.trim();
    Some(PathBuf::from(shell)).filter(|p| p.is_absolute())
}

/// How to ask `shell` for its environment, as a login and interactive
/// shell so both kinds of profile files are read (csh and tcsh take `-l`
/// only on its own, so they read `.cshrc`). The command is one of two fixed
/// strings; the markers reach the shell through its environment. `None`
/// for shells with another command syntax.
fn shell_arguments(shell: &Path) -> Option<Vec<String>> {
    let name = shell.file_name()?.to_string_lossy().to_string();
    match name.as_str() {
        "bash" | "zsh" | "sh" | "dash" | "ksh" | "mksh" | "fish" => Some(vec![
            "-i".into(),
            "-l".into(),
            "-c".into(),
            POSIX_ENV_COMMAND.into(),
        ]),
        "csh" | "tcsh" => Some(vec!["-i".into(), "-c".into(), CSH_ENV_COMMAND.into()]),
        _ => None,
    }
}

/// The `NAME=value` entries between the two marker lines. Entries are
/// NUL-separated when `env -0` printed them; otherwise one per line, and
/// only lines that look like `NAME=value` count, so the continuation lines
/// of a multi-line value (and anything else) are skipped. Whatever a
/// profile prints before the first marker or after the last is ignored.
fn parse_env_block(stdout: &[u8], begin: &str, end: &str) -> HashMap<OsString, OsString> {
    let text = String::from_utf8_lossy(stdout);
    // A marker counts only as a whole line: `env` itself prints the marker
    // variables, whose values contain the same text.
    let begin_line = format!("\n{begin}\n");
    let end_line = format!("\n{end}\n");
    let Some(from) = text.find(&begin_line).map(|i| i + begin_line.len()) else {
        return HashMap::new();
    };
    let Some(to) = text[from..].rfind(&end_line).map(|i| from + i) else {
        return HashMap::new();
    };
    let block = &text[from..to];
    let entries: Vec<&str> = if block.contains('\0') {
        block.split('\0').collect()
    } else {
        block.lines().collect()
    };
    entries
        .into_iter()
        .filter_map(|entry| entry.split_once('='))
        .filter(|(name, _)| {
            !name.is_empty()
                && !SHELL_SESSION_VARS.contains(name)
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
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
    let mut command = launch(&located.path, &["--version"]);
    command
        .env("PATH", located.search_path())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(20), command.output())
        .await
        .ok()?
        .ok()?;
    parse_version(&String::from_utf8_lossy(&output.stdout))
}

/// A command that runs the program at `path` with `fixed_args`: literals
/// from ReMa's own code, never text a user, a server or a model wrote.
///
/// On Windows, npm, pnpm, Volta and nvm-windows install programs as
/// `.cmd` wrappers, which only `cmd.exe` can run. Such a wrapper is started
/// as `cmd.exe /d /s /c ""<path>" <fixed args>"`: `/d` skips AutoRun
/// commands, `/s` keeps the quoted path (spaces included) as one program
/// name, and [`shim_command_line`] refuses anything cmd.exe would
/// interpret. Everything else starts directly, without any shell.
pub fn launch(path: &Path, fixed_args: &[&str]) -> tokio::process::Command {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if let Some(line) = shim_command_line(path, fixed_args) {
            // The system's own cmd.exe, not whichever is first on PATH.
            let cmd = env::var_os("SystemRoot")
                .map(|root| PathBuf::from(root).join("System32").join("cmd.exe"))
                .filter(|p| p.is_file())
                .unwrap_or_else(|| PathBuf::from("cmd.exe"));
            let mut command = tokio::process::Command::new(cmd);
            command.args(["/d", "/s", "/c"]).raw_arg(line);
            command.creation_flags(CREATE_NO_WINDOW);
            return command;
        }
        let mut command = tokio::process::Command::new(path);
        command.args(fixed_args);
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = tokio::process::Command::new(path);
        command.args(fixed_args);
        command
    }
}

/// The command line `cmd.exe /d /s /c` gets for a `.cmd` or `.bat` wrapper:
/// `""<path>" <args>"`, whose outer quotes `/s` removes. `None` for any
/// other program, and for a path or argument cmd.exe would not take
/// literally (quotes, `%` and `!` expansions, `^` escapes, control
/// characters); arguments must be plain option words.
pub fn shim_command_line(path: &Path, fixed_args: &[&str]) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    if extension != "cmd" && extension != "bat" {
        return None;
    }
    let path = path.to_str()?;
    if path.is_empty()
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '%' | '!' | '^'))
    {
        return None;
    }
    let plain = |arg: &&str| {
        !arg.is_empty()
            && arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '='))
    };
    if !fixed_args.iter().all(plain) {
        return None;
    }
    let mut line = format!("\"\"{path}\"");
    for arg in fixed_args {
        line.push(' ');
        line.push_str(arg);
    }
    line.push('"');
    Some(line)
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

/// The first `dir/name` that exists and can be run. On Windows, `name`
/// takes the launchable `PATHEXT` extensions, as the command line does.
fn search(name: &str, dirs: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        let pathext = env::var("PATHEXT").ok();
        windows_program_names(name, pathext.as_deref())
    } else {
        vec![name.to_string()]
    };
    dirs.into_iter()
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|candidate| is_executable(candidate))
}

/// Program extensions ReMa can start on Windows: executables and the
/// `.cmd`/`.bat` wrappers package managers install (through cmd.exe, see
/// [`launch`]). Scripts for other interpreters (`.ps1`, `.js`, `.vbs`) are
/// not programs to ReMa even when `PATHEXT` lists them.
const WINDOWS_LAUNCHABLE: [&str; 4] = [".exe", ".cmd", ".bat", ".com"];

/// The file names `name` may have on Windows, in `PATHEXT` order (the
/// order the command line resolves them in): `codex` → `codex.COM`,
/// `codex.EXE`, `codex.BAT`, `codex.CMD`. A name that already carries a
/// launchable extension is looked up as it is. Any launchable extension
/// `PATHEXT` leaves out still counts, after the listed ones, so an npm
/// `.cmd` wrapper is found under a trimmed `PATHEXT` too.
pub fn windows_program_names(name: &str, pathext: Option<&str>) -> Vec<String> {
    let lower = name.to_ascii_lowercase();
    if WINDOWS_LAUNCHABLE.iter().any(|ext| lower.ends_with(ext)) {
        return vec![name.to_string()];
    }
    let mut names: Vec<String> = pathext
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .map(str::trim)
        .filter(|ext| {
            let ext = ext.to_ascii_lowercase();
            WINDOWS_LAUNCHABLE.contains(&ext.as_str())
        })
        .map(|ext| format!("{name}{ext}"))
        .collect();
    for ext in WINDOWS_LAUNCHABLE {
        if !names.iter().any(|n| n.to_ascii_lowercase().ends_with(ext)) {
            names.push(format!("{name}{ext}"));
        }
    }
    names
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

/// Where the user's login shell would find `name`: its `PATH`, read once
/// per run (see [`login_environment`]) and searched by ReMa itself, so the
/// name never reaches a shell. Nothing on Windows, where no shell is asked.
async fn login_path_lookup(name: &str) -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let login = login_environment().await;
    let path = login.get(OsStr::new("PATH"))?;
    search(name, env::split_paths(path))
}

#[cfg(test)]
mod windows_tests {
    //! The Windows path logic is plain string work, tested everywhere.
    use super::*;

    #[test]
    fn windows_programs_take_the_launchable_pathext_extensions_in_order() {
        assert_eq!(
            windows_program_names("codex", Some(".COM;.EXE;.BAT;.CMD;.VBS;.JS;.PS1")),
            ["codex.COM", "codex.EXE", "codex.BAT", "codex.CMD"]
        );
        assert_eq!(
            windows_program_names("codex", None),
            ["codex.COM", "codex.EXE", "codex.BAT", "codex.CMD"]
        );
        // A trimmed PATHEXT still finds npm's .cmd wrappers, after its own.
        assert_eq!(
            windows_program_names("npx", Some(".EXE")),
            ["npx.EXE", "npx.cmd", "npx.bat", "npx.com"]
        );
        assert_eq!(windows_program_names("node.exe", None), ["node.exe"]);
        assert_eq!(windows_program_names("Codex.CMD", None), ["Codex.CMD"]);
        assert_eq!(
            windows_program_names("server.ps1", Some(".PS1;.EXE")),
            [
                "server.ps1.EXE",
                "server.ps1.cmd",
                "server.ps1.bat",
                "server.ps1.com"
            ],
            "an interpreter script is not a program"
        );
    }

    #[test]
    fn cmd_wrappers_run_through_cmd_exe_with_the_quoted_path_only() {
        let shim = Path::new(r"C:\Users\Ana Lopez\AppData\Roaming\npm\codex.cmd");
        assert_eq!(
            shim_command_line(shim, &["--version"]).as_deref(),
            Some(r#"""C:\Users\Ana Lopez\AppData\Roaming\npm\codex.cmd" --version""#)
        );
        assert_eq!(
            shim_command_line(Path::new(r"C:\tools\run.BAT"), &[]).as_deref(),
            Some(r#"""C:\tools\run.BAT"""#)
        );
        // Executables start directly, without cmd.exe.
        assert_eq!(
            shim_command_line(Path::new(r"C:\tools\codex.exe"), &["--version"]),
            None
        );
        assert_eq!(shim_command_line(Path::new(r"C:\tools\codex"), &[]), None);
        // What cmd.exe would interpret is refused rather than escaped.
        for path in [
            "C:\\odd\"name\\x.cmd",
            r"C:\100%\x.cmd",
            r"C:\bang!\x.cmd",
            r"C:\caret^\x.cmd",
            "C:\\new\nline\\x.cmd",
        ] {
            assert_eq!(shim_command_line(Path::new(path), &[]), None, "{path:?}");
        }
        for arg in ["--name=a b", "&& del", "\"", "--x|y", ""] {
            assert_eq!(shim_command_line(shim, &[arg]), None, "{arg:?}");
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::state::testing::temp_dir;

    fn executable(dir: &Path, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn reads_the_login_environment_between_its_markers() {
        let (b, e) = ("__REMA_ENV_BEGIN_ab__", "__REMA_ENV_END_ab__");
        // Chatter before (a message of the day, Powerlevel10k), NUL-separated
        // entries, chatter after (a logout hook): only the block counts.
        let stdout = format!(
            "Welcome to fish\n\x1b[1mp10k\x1b[0m\n{b}\nHOME=/Users/ana\0PATH=/opt/homebrew/bin:/usr/bin\0\
             OPENAI_API_KEY=sk-x=y\0PWD=/tmp\0SHLVL=2\0REMA_ENV_BEGIN={b}\0REMA_ENV_END={e}\0\n{e}\nbye\n"
        );
        let env = parse_env_block(stdout.as_bytes(), b, e);
        assert_eq!(
            env.get(OsStr::new("HOME")),
            Some(&OsString::from("/Users/ana"))
        );
        assert_eq!(
            env.get(OsStr::new("OPENAI_API_KEY")),
            Some(&OsString::from("sk-x=y"))
        );
        assert!(env.contains_key(OsStr::new("PATH")));
        for session_var in ["PWD", "SHLVL", "REMA_ENV_BEGIN", "REMA_ENV_END"] {
            assert!(!env.contains_key(OsStr::new(session_var)), "{session_var}");
        }
        assert!(!env
            .values()
            .any(|v| v.to_string_lossy().contains("Welcome")));
        assert!(parse_env_block(b"no markers", b, e).is_empty());
        assert!(
            parse_env_block(format!("{b}\nA=1\n").as_bytes(), b, e).is_empty(),
            "a shell stopped before the end marker gives nothing"
        );
    }

    #[test]
    fn without_env_0_only_name_value_lines_count() {
        let (b, e) = ("__REMA_ENV_BEGIN_cd__", "__REMA_ENV_END_cd__");
        // A profile that prints the marker text on a line of chatter cannot
        // fake the block: the marker variables themselves are lines too.
        let stdout = format!(
            "conda activate base\nREMA_ENV_BEGIN={b}\n{b}\nHOME=/home/ana\nMOTD=first line\n  second line\n\
             not a variable\nHTTPS_PROXY=http://proxy:3128\nREMA_ENV_END={e}\n{e}\nlogout chatter\n"
        );
        let env = parse_env_block(stdout.as_bytes(), b, e);
        assert_eq!(
            env.get(OsStr::new("HOME")),
            Some(&OsString::from("/home/ana"))
        );
        assert_eq!(
            env.get(OsStr::new("MOTD")),
            Some(&OsString::from("first line")),
            "a multi-line value keeps its first line"
        );
        assert_eq!(
            env.get(OsStr::new("HTTPS_PROXY")),
            Some(&OsString::from("http://proxy:3128"))
        );
        assert_eq!(env.len(), 3, "{env:?}");
    }

    #[tokio::test]
    async fn a_chatty_profile_does_not_get_into_the_environment() {
        // A "zsh" that prints before and after the command, exports a
        // variable in its profile and then runs the command with /bin/sh.
        let dir = temp_dir();
        let shell = executable(
            &dir,
            "zsh",
            "#!/bin/sh\necho 'Last login: today'\nprintf '\\033[1mfancy prompt\\033[0m\\n'\n\
             export REMA_TEST_FROM_PROFILE=works\nshift 3\n/bin/sh -c \"$1\"\necho bye\n",
        );
        let env = ask_shell(&shell, Duration::from_secs(10)).await.unwrap();
        assert_eq!(
            env.get(OsStr::new("REMA_TEST_FROM_PROFILE")),
            Some(&OsString::from("works"))
        );
        assert!(env.contains_key(OsStr::new("PATH")));
        assert!(!env.contains_key(OsStr::new("REMA_ENV_BEGIN")));
        assert!(!env.contains_key(OsStr::new("PWD")));
        assert!(
            !env.iter().any(|(k, v)| {
                let text = format!("{}{}", k.to_string_lossy(), v.to_string_lossy());
                text.contains("Last login") || text.contains("fancy") || text.contains("bye")
            }),
            "{env:?}"
        );
    }

    #[tokio::test]
    async fn a_shell_that_does_not_answer_is_stopped_and_ignored() {
        let dir = temp_dir();
        let shell = executable(&dir, "bash", "#!/bin/sh\nexec sleep 30\n");
        let started = std::time::Instant::now();
        assert_eq!(ask_shell(&shell, Duration::from_millis(300)).await, None);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        // The rest of ReMa goes on with its own environment.
        let dirs = shell_search_dirs(None);
        assert!(dirs.len() >= install_dirs().len());
    }

    #[test]
    fn a_program_is_found_where_the_users_shell_finds_it() {
        let nvm = temp_dir();
        let system = temp_dir();
        for dir in [&nvm, &system] {
            executable(dir, "node", "#!/bin/sh\n");
        }
        let login = env::join_paths([&nvm, &system]).unwrap();
        let dirs = shell_search_dirs(Some(&login));
        assert_eq!(&dirs[..2], [nvm.clone(), system.clone()]);
        assert_eq!(search("node", dirs), Some(nvm.join("node")));
        // The login PATH is searched by ReMa: the name never reaches a shell.
        assert_eq!(
            search("node", env::split_paths(&login)),
            Some(nvm.join("node"))
        );
        assert_eq!(search("nod e; rm -rf /", env::split_paths(&login)), None);
        // Without a login shell, ReMa's own PATH and install folders remain.
        assert!(shell_search_dirs(None).len() >= install_dirs().len());
    }

    #[test]
    fn finds_the_users_shell_without_a_shell_variable() {
        assert_eq!(
            shell_from_dscl("UserShell: /opt/homebrew/bin/fish\n"),
            Some(PathBuf::from("/opt/homebrew/bin/fish"))
        );
        assert_eq!(shell_from_dscl("No such key: UserShell"), None);
        assert_eq!(
            shell_from_passwd("ana:x:501:20:Ana:/home/ana:/usr/bin/zsh\n"),
            Some(PathBuf::from("/usr/bin/zsh"))
        );
        assert_eq!(shell_from_passwd("ana:x:501:20:Ana:/home/ana:"), None);
        // A relative or empty shell is not trusted.
        assert_eq!(shell_from_dscl("UserShell: zsh"), None);
    }

    #[test]
    fn each_shell_is_asked_in_its_own_syntax() {
        let zsh = shell_arguments(Path::new("/bin/zsh")).unwrap();
        assert_eq!(&zsh[..3], ["-i", "-l", "-c"]);
        assert_eq!(zsh[3], POSIX_ENV_COMMAND);
        assert_eq!(
            shell_arguments(Path::new("/usr/bin/fish")).unwrap()[3],
            POSIX_ENV_COMMAND
        );
        let tcsh = shell_arguments(Path::new("/bin/tcsh")).unwrap();
        assert_eq!(&tcsh[..2], ["-i", "-c"]);
        assert_eq!(tcsh[2], CSH_ENV_COMMAND);
        assert_eq!(shell_arguments(Path::new("/usr/bin/nu")), None);
        // The command is fixed text: the markers come from the environment.
        for command in [POSIX_ENV_COMMAND, CSH_ENV_COMMAND] {
            assert!(command.contains("\"$REMA_ENV_BEGIN\""));
            assert!(command.contains("\"$REMA_ENV_END\""));
            assert!(command.contains("env -0 ||"), "falls back to line output");
            assert!(!command.contains("__REMA"));
        }
        assert!(POSIX_ENV_COMMAND.contains("command env"));
        assert!(CSH_ENV_COMMAND.contains("/usr/bin/env"));
    }

    #[test]
    fn the_login_path_comes_right_after_the_programs_own_folder() {
        let first = [PathBuf::from("/opt/node/bin")];
        let joined = extended_path_with(&first, Some(OsStr::new("/login/bin:/opt/node/bin")));
        let dirs: Vec<PathBuf> = env::split_paths(&joined).collect();
        assert_eq!(dirs[0], PathBuf::from("/opt/node/bin"));
        assert_eq!(dirs[1], PathBuf::from("/login/bin"));
        assert_eq!(
            dirs.iter()
                .filter(|d| d.as_path() == Path::new("/opt/node/bin"))
                .count(),
            1,
            "no folder twice"
        );
    }

    #[test]
    fn searches_folders_in_order_and_skips_non_executables() {
        let (a, b) = (temp_dir(), temp_dir());
        std::fs::write(a.join("codex"), "not executable").unwrap();
        let wanted = executable(&b, "codex", "#!/bin/sh\n");
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
            executable(
                dir,
                "codex",
                &format!("#!/bin/sh\necho codex-cli {version}\n"),
            )
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
