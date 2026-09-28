//! The pure logic behind `build.rs`'s preflight: which OAuth settings a
//! release needs, which runtime files a target bundles, and whether a
//! runtime file is an executable for that target.
//!
//! Build scripts cannot have tests, so this file has no dependencies and is
//! included by `build.rs` and by `tests/build_support.rs`, which tests it.
//! Nothing here reads the environment or the file system: `build.rs` passes
//! in what it found and prints what comes back.

#![allow(dead_code)]

/// OAuth settings (their environment-variable names; `connectors.toml` names
/// the same settings) a release build cannot ship without. Client IDs are
/// public identifiers, never secrets.
pub const REQUIRED_FOR_RELEASE: [&str; 2] =
    ["GOOGLE_DESKTOP_CLIENT_ID", "MICROSOFT_PUBLIC_CLIENT_ID"];

/// Whether a release build needs a setting or may go without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Required,
    Optional,
}

/// The Google Desktop client secret is optional (Google does not treat it
/// as confidential for installed apps, and a build can work without it),
/// as are the LinkedIn keys, the Google publishing status and the
/// Microsoft tenant, which has a default.
pub fn need(env_name: &str) -> Need {
    if REQUIRED_FOR_RELEASE.contains(&env_name) {
        Need::Required
    } else {
        Need::Optional
    }
}

/// The OAuth settings a build is missing, by name only, never by value.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MissingKeys {
    pub required: Vec<String>,
    pub optional: Vec<String>,
}

impl MissingKeys {
    pub fn is_empty(&self) -> bool {
        self.required.is_empty() && self.optional.is_empty()
    }
}

/// Sorts the settings `names` that `present` does not find into required
/// and optional, keeping the given order.
pub fn missing_keys<'a>(
    names: impl IntoIterator<Item = &'a str>,
    present: impl Fn(&str) -> bool,
) -> MissingKeys {
    let mut missing = MissingKeys::default();
    for name in names {
        if present(name) {
            continue;
        }
        match need(name) {
            Need::Required => missing.required.push(name.to_string()),
            Need::Optional => missing.optional.push(name.to_string()),
        }
    }
    missing
}

/// The files `pnpm build:app` (scripts/runtimes/fetch.mjs) places in
/// `src-tauri/runtimes/` for `target`, named as Tauri expects sidecars
/// (`<name>-<target triple>[.exe]`).
pub fn runtime_files(target: &str) -> [String; 2] {
    let exe = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    [
        format!("runtimes/codex-{target}{exe}"),
        format!("runtimes/ant-{target}{exe}"),
    ]
}

/// A processor architecture an executable file is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    /// A macOS fat binary holding several architectures.
    Universal,
    Other,
}

/// An executable file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Elf,
    MachO,
    Pe,
}

/// What a Rust target triple runs: its executable format and architecture.
pub fn target_binary(target: &str) -> Option<(Format, Arch)> {
    let arch = if target.starts_with("x86_64-") {
        Arch::X86_64
    } else if target.starts_with("aarch64-") {
        Arch::Aarch64
    } else if target.starts_with("universal-") {
        Arch::Universal
    } else {
        return None;
    };
    let format = if target.contains("-apple-") {
        Format::MachO
    } else if target.contains("-windows-") {
        Format::Pe
    } else if target.contains("-linux-") {
        Format::Elf
    } else {
        return None;
    };
    Some((format, arch))
}

/// Reads the format and architecture from the first bytes of an executable
/// file (an ELF, Mach-O or PE header). `None` when it is none of these (a
/// script, a text file, an HTML error page saved as a download).
pub fn inspect_binary(head: &[u8]) -> Option<(Format, Arch)> {
    let u16_at = |at: usize, little: bool| -> Option<u16> {
        let bytes = [*head.get(at)?, *head.get(at + 1)?];
        Some(if little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    };
    let u32_at = |at: usize, little: bool| -> Option<u32> {
        let bytes = [
            *head.get(at)?,
            *head.get(at + 1)?,
            *head.get(at + 2)?,
            *head.get(at + 3)?,
        ];
        Some(if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    match head.get(..4)? {
        [0x7F, b'E', b'L', b'F'] => {
            // Byte 5 says the header's byte order; e_machine is at 18.
            let little = *head.get(5)? == 1;
            let arch = match u16_at(18, little)? {
                0x3E => Arch::X86_64,
                0xB7 => Arch::Aarch64,
                _ => Arch::Other,
            };
            Some((Format::Elf, arch))
        }
        // MH_MAGIC_64 as stored by a little-endian file, and its reverse.
        [0xCF, 0xFA, 0xED, 0xFE] | [0xFE, 0xED, 0xFA, 0xCF] => {
            let little = head[0] == 0xCF;
            let arch = match u32_at(4, little)? {
                0x0100_0007 => Arch::X86_64,
                0x0100_000C => Arch::Aarch64,
                _ => Arch::Other,
            };
            Some((Format::MachO, arch))
        }
        // FAT_MAGIC: a universal binary (its slices are not inspected).
        [0xCA, 0xFE, 0xBA, 0xBE] => Some((Format::MachO, Arch::Universal)),
        [b'M', b'Z', ..] => {
            // The PE header starts where e_lfanew (at 0x3C) points; the
            // machine type follows the "PE\0\0" signature.
            let pe = u32_at(0x3C, true)? as usize;
            if head.get(pe..pe + 4)? != b"PE\0\0" {
                return None;
            }
            let arch = match u16_at(pe + 4, true)? {
                0x8664 => Arch::X86_64,
                0xAA64 => Arch::Aarch64,
                _ => Arch::Other,
            };
            Some((Format::Pe, arch))
        }
        _ => None,
    }
}

/// Why a runtime file cannot be bundled for `target`, or `None` when it
/// can. `head` is the file's first bytes (`None` when the file is missing),
/// `executable` whether it has an execute bit (always true on Windows).
pub fn runtime_problem(target: &str, head: Option<&[u8]>, executable: bool) -> Option<String> {
    let Some(head) = head else {
        return Some("missing (run `pnpm runtimes`)".into());
    };
    let Some((format, arch)) = inspect_binary(head) else {
        return Some("not an executable file (the download may be incomplete)".into());
    };
    let Some((wanted_format, wanted_arch)) = target_binary(target) else {
        // An unknown target cannot be checked further; the bundler decides.
        return None;
    };
    if format != wanted_format {
        return Some(format!(
            "built for another operating system ({format:?}, not {wanted_format:?})"
        ));
    }
    let fits = arch == wanted_arch
        || (format == Format::MachO && arch == Arch::Universal)
        || (wanted_arch == Arch::Universal && format == Format::MachO);
    if !fits {
        return Some(format!(
            "built for another processor ({arch:?}, not {wanted_arch:?})"
        ));
    }
    if !executable {
        return Some("not executable (chmod +x)".into());
    }
    None
}

/// The preflight report: every missing OAuth setting by name, sorted into
/// required and optional, and every runtime file that cannot be bundled
/// (`bundling` says whether this build ships them or only looks for them).
/// Empty when nothing is missing.
pub fn preflight_report(
    release: bool,
    bundling: bool,
    missing: &MissingKeys,
    runtime_problems: &[(String, String)],
) -> String {
    let mut lines = Vec::new();
    if !missing.is_empty() {
        lines.push(
            "OAuth configuration (src-tauri/connectors.toml or the environment):".to_string(),
        );
        for name in &missing.required {
            lines.push(format!(
                "  - {name}: missing, required for a release build{}",
                if release {
                    ""
                } else {
                    " (this development build shows the connector as unavailable)"
                }
            ));
        }
        for name in &missing.optional {
            lines.push(format!("  - {name}: missing, optional"));
        }
    }
    if !runtime_problems.is_empty() {
        lines.push(format!(
            "Bundled runtimes (src-tauri/runtimes/, fetched by `pnpm runtimes` for the target){}:",
            if bundling {
                ", required by this bundling build"
            } else {
                ", not shipped by this build (a development build uses an installed copy)"
            }
        ));
        for (path, problem) in runtime_problems {
            lines.push(format!("  - {path}: {problem}"));
        }
    }
    lines.join("\n")
}
