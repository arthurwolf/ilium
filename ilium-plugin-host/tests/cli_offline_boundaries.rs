//! Offline inspection must use package bytes without network or session effects.
use ilium_plugin_host::cli::{run, CliEnv};
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use std::{sync::Arc, time::SystemTime};

struct NoNetwork;
impl Fetcher for NoNetwork {
    fn get(&self, _: &str) -> Result<Vec<u8>, FetchError> {
        panic!("offline commands must not fetch")
    }
}
fn environment(root: &std::path::Path) -> CliEnv {
    CliEnv::new(
        root.join("data"),
        root.join("state"),
        root.join("config"),
        root.into(),
        SystemTime::now(),
        true,
        Arc::new(NoNetwork),
    )
}
#[test]
fn offline_inspection_reports_real_bytes_without_creating_state() {
    let root = tempfile::tempdir().unwrap();
    let hex = include_str!("package/golden/minimal_unsigned.hex");
    let compact: String = hex
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let bytes: Vec<u8> = compact
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    std::fs::write(root.path().join("example.iliumpkg"), &bytes).unwrap();
    let output = run(
        &["inspect".into(), "example.iliumpkg".into(), "--json".into()],
        &environment(root.path()),
    );
    assert_eq!(output.exit_code, 0, "{}", output.stdout_raw);
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|record| record["section"] == "manifest"));
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|record| record["section"] == "file" && record["data"]["bytes"].as_u64().is_some()));
    assert_eq!(
        output
            .stdout_jsonl
            .iter()
            .filter(|record| record["type"] == "summary")
            .count(),
        1
    );
    assert_eq!(output.stdout_jsonl.last().unwrap()["type"], "result");
    assert!(output
        .stdout_raw
        .lines()
        .all(|line| line.starts_with("{\"type\":")));
    assert!(!root.path().join("state").exists());
    assert!(!root.path().join("data").exists());
}
#[test]
fn unknown_flag_fails_before_file_or_network_access() {
    let root = tempfile::tempdir().unwrap();
    let output = run(
        &[
            "inspect".into(),
            "missing.iliumpkg".into(),
            "--invented".into(),
        ],
        &environment(root.path()),
    );
    assert_eq!(output.exit_code, 2);
    assert_eq!(output.stdout_jsonl[0]["code"], "usage.unknown-flag");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn inspection_verification_detects_changed_payload_with_valid_zip_checksum() {
    use ilium_plugin_host::package::{read_package, write_package};
    let root = tempfile::tempdir().unwrap();
    let compact: String = include_str!("package/golden/minimal_unsigned.hex")
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let bytes: Vec<u8> = compact
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    let archive = read_package(&bytes).unwrap();
    let payload = archive.manifest.files.first().unwrap().path.clone();
    let files: Vec<_> = archive
        .entries
        .into_iter()
        .map(|entry| {
            let mut bytes = entry.bytes;
            if entry.path == payload {
                bytes.push(0);
            }
            (entry.path, bytes)
        })
        .collect();
    let changed = write_package(&files).unwrap();
    // Repacking updates ZIP checksums; the unchanged manifest inventory must
    // independently catch the payload alteration.
    std::fs::write(root.path().join("changed.iliumpkg"), changed).unwrap();
    let output = run(
        &[
            "inspect".into(),
            "changed.iliumpkg".into(),
            "--verify".into(),
            "--json".into(),
        ],
        &environment(root.path()),
    );
    assert_eq!(output.exit_code, 1, "{}", output.stdout_raw);
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|record| record["type"] == "diff" && record["subject"] == payload));
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|record| record["code"] == "inspect.hash-mismatch"));
    assert!(!root.path().join("state").exists());
}

#[test]
fn debouncer_waits_for_quiet_and_retains_separated_bursts() {
    use ilium_plugin_host::cli::dev::{Debouncer, DEBOUNCE};
    let mut debounce = Debouncer::new(DEBOUNCE);
    debounce.change(0);
    debounce.change(200);
    assert_eq!(debounce.due(499), 0);
    assert_eq!(debounce.due(500), 1);
    assert_eq!(debounce.due(500), 0);
    debounce.change(1000);
    debounce.change(1500);
    assert_eq!(debounce.due(1600), 1);
    assert_eq!(debounce.due(1800), 1);
    assert_eq!(debounce.due(2000), 0);
}

#[test]
fn grants_command_reads_real_session_state_without_mutation() {
    use ilium_plugin_host::cli::MemorySession;
    let root = tempfile::tempdir().unwrap();
    let session = MemorySession::new();
    session.add_plugin(
        "example.notes",
        "1.0.0",
        "T2",
        "disabled",
        false,
        &[
            ("tree.read", "low", "granted"),
            ("process.spawn", "high", "declined"),
        ],
    );
    let env = environment(root.path()).with_session(session.clone());
    let output = run(
        &["grants".into(), "example.notes".into(), "--json".into()],
        &env,
    );
    assert_eq!(output.exit_code, 0, "{}", output.stdout_raw);
    assert_eq!(
        output
            .stdout_jsonl
            .iter()
            .filter(|record| record["type"] == "grant")
            .count(),
        2
    );
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|record| record["grant"] == "process.spawn" && record["state"] == "declined"));
    assert_eq!(session.connect_attempts(), 1);
    assert!(session.journal().is_empty());
    assert_eq!(session.sessions_started(), 0);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
