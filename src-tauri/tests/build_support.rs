//! Tests of the pure logic behind `build.rs`'s preflight (build scripts
//! cannot have tests of their own).

#[path = "../build_support.rs"]
mod build_support;

use build_support::*;

/// A little-endian ELF header with `machine` at offset 18.
fn elf(machine: u16) -> Vec<u8> {
    let mut head = vec![0u8; 64];
    head[..4].copy_from_slice(b"\x7FELF");
    head[5] = 1;
    head[18..20].copy_from_slice(&machine.to_le_bytes());
    head
}

/// A 64-bit Mach-O header with `cputype` at offset 4.
fn macho(cputype: u32) -> Vec<u8> {
    let mut head = vec![0u8; 32];
    head[..4].copy_from_slice(&[0xCF, 0xFA, 0xED, 0xFE]);
    head[4..8].copy_from_slice(&cputype.to_le_bytes());
    head
}

/// A DOS stub pointing at a PE header whose `Machine` is `machine`.
fn pe(machine: u16) -> Vec<u8> {
    let mut head = vec![0u8; 0x100];
    head[..2].copy_from_slice(b"MZ");
    head[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    head[0x80..0x84].copy_from_slice(b"PE\0\0");
    head[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
    head
}

#[test]
fn only_the_google_and_microsoft_client_ids_are_required_for_a_release() {
    let names = [
        "GOOGLE_DESKTOP_CLIENT_ID",
        "GOOGLE_DESKTOP_CLIENT_SECRET",
        "GOOGLE_PUBLISHING_STATUS",
        "MICROSOFT_PUBLIC_CLIENT_ID",
        "MICROSOFT_TENANT",
        "LINKEDIN_CLIENT_ID",
        "LINKEDIN_APPROVED_SCOPES",
    ];
    let missing = missing_keys(names, |name| name == "MICROSOFT_TENANT");
    assert_eq!(
        missing.required,
        ["GOOGLE_DESKTOP_CLIENT_ID", "MICROSOFT_PUBLIC_CLIENT_ID"]
    );
    assert_eq!(
        missing.optional,
        [
            "GOOGLE_DESKTOP_CLIENT_SECRET",
            "GOOGLE_PUBLISHING_STATUS",
            "LINKEDIN_CLIENT_ID",
            "LINKEDIN_APPROVED_SCOPES"
        ]
    );
    assert_eq!(need("GOOGLE_DESKTOP_CLIENT_SECRET"), Need::Optional);
    assert!(missing_keys(names, |_| true).is_empty());
}

#[test]
fn the_report_names_every_key_but_never_a_value() {
    let missing = missing_keys(
        ["GOOGLE_DESKTOP_CLIENT_ID", "GOOGLE_DESKTOP_CLIENT_SECRET"],
        |_| false,
    );
    let problems = [(
        "runtimes/ant-aarch64-apple-darwin".to_string(),
        "missing (run `pnpm runtimes`)".to_string(),
    )];
    let release = preflight_report(true, true, &missing, &problems);
    assert!(release.contains("GOOGLE_DESKTOP_CLIENT_ID: missing, required for a release build"));
    assert!(release.contains("GOOGLE_DESKTOP_CLIENT_SECRET: missing, optional"));
    assert!(release.contains("runtimes/ant-aarch64-apple-darwin: missing"));
    assert!(release.contains("required by this bundling build"));
    assert!(!release.contains("unavailable"));
    let debug = preflight_report(false, false, &missing, &[]);
    assert!(debug.contains("shows the connector as unavailable"));
    assert!(!debug.contains("Bundled runtimes"));
    assert_eq!(
        preflight_report(true, true, &MissingKeys::default(), &[]),
        ""
    );
    let dev = preflight_report(false, false, &MissingKeys::default(), &problems);
    assert!(dev.contains("not shipped by this build"));
}

#[test]
fn runtime_files_are_named_after_the_target() {
    assert_eq!(
        runtime_files("aarch64-apple-darwin"),
        [
            "runtimes/codex-aarch64-apple-darwin",
            "runtimes/ant-aarch64-apple-darwin"
        ]
    );
    assert_eq!(
        runtime_files("x86_64-pc-windows-msvc")[1],
        "runtimes/ant-x86_64-pc-windows-msvc.exe"
    );
}

#[test]
fn reads_the_architecture_from_executable_headers() {
    assert_eq!(
        inspect_binary(&elf(0x3E)),
        Some((Format::Elf, Arch::X86_64))
    );
    assert_eq!(
        inspect_binary(&elf(0xB7)),
        Some((Format::Elf, Arch::Aarch64))
    );
    assert_eq!(
        inspect_binary(&macho(0x0100_000C)),
        Some((Format::MachO, Arch::Aarch64))
    );
    assert_eq!(
        inspect_binary(&macho(0x0100_0007)),
        Some((Format::MachO, Arch::X86_64))
    );
    assert_eq!(
        inspect_binary(&[0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 2]),
        Some((Format::MachO, Arch::Universal))
    );
    assert_eq!(
        inspect_binary(&pe(0x8664)),
        Some((Format::Pe, Arch::X86_64))
    );
    assert_eq!(
        inspect_binary(&pe(0xAA64)),
        Some((Format::Pe, Arch::Aarch64))
    );
    assert_eq!(inspect_binary(b"#!/bin/sh\n"), None);
    assert_eq!(inspect_binary(b"<!doctype html>"), None);
    assert_eq!(
        inspect_binary(b"MZ"),
        None,
        "a truncated header is not a binary"
    );
    assert_eq!(inspect_binary(&[]), None);
}

#[test]
fn a_runtime_must_match_the_targets_system_and_processor() {
    let mac_arm = "aarch64-apple-darwin";
    assert_eq!(
        runtime_problem(mac_arm, Some(&macho(0x0100_000C)), true),
        None
    );
    assert_eq!(
        runtime_problem(mac_arm, Some(&[0xCA, 0xFE, 0xBA, 0xBE]), true),
        None,
        "a universal binary runs on either Mac"
    );
    assert!(runtime_problem(mac_arm, Some(&macho(0x0100_0007)), true)
        .unwrap()
        .contains("another processor"));
    assert!(runtime_problem(mac_arm, Some(&elf(0xB7)), true)
        .unwrap()
        .contains("another operating system"));
    assert!(runtime_problem(mac_arm, None, true)
        .unwrap()
        .contains("missing"));
    assert!(runtime_problem(mac_arm, Some(b"#!/bin/sh\n"), true)
        .unwrap()
        .contains("not an executable file"));
    assert!(
        runtime_problem("x86_64-unknown-linux-gnu", Some(&elf(0x3E)), false)
            .unwrap()
            .contains("not executable")
    );
    assert_eq!(
        runtime_problem("x86_64-pc-windows-msvc", Some(&pe(0x8664)), true),
        None
    );
    // An unknown target is left to the bundler.
    assert_eq!(
        runtime_problem("riscv64gc-unknown-linux-gnu", Some(&elf(0xF3)), true),
        None
    );
}
