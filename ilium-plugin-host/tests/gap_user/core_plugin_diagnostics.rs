//! Support bundles and `doctor` of the page `core-plugin-diagnostics` (USR-332, USR-334, USR-335,
//! USR-336, USR-337).

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::common::*;

/// Reads every entry of a support bundle zip into one string.
fn read_bundle_text(bundle: &Path) -> String {
    let file = File::open(bundle).expect("bundle exists");
    let mut archive = zip::ZipArchive::new(file).expect("bundle is a zip");
    let mut combined = String::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("zip entry");
        entry.read_to_string(&mut combined).expect("text entry");
    }
    combined
}

/// REQ: USR-332
#[test]
fn usr_332_dry_run_prints_bundle_files_and_redaction_counts_without_writing_the_zip() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_log_line(NOTES, "token=abc123");
    let dry = rig.cli(&["support-bundle", NOTES, "--dry-run"]);
    assert_eq!(dry.exit_code, 0, "{}", dry.stderr);
    assert!(dry.stdout_raw.contains("summary.txt"), "{}", dry.stdout_raw);
    assert!(dry.stdout_raw.contains("log.txt"), "{}", dry.stdout_raw);
    assert!(
        dry.stdout_raw.contains("redactions"),
        "redaction count missing: {}",
        dry.stdout_raw
    );
    assert!(
        rig.support_bundles().is_empty(),
        "a dry run must not write the zip"
    );

    let real = rig.cli(&["support-bundle", NOTES]);
    assert_eq!(real.exit_code, 0, "{}", real.stderr);
    let viewed = rig.viewer_documents();
    assert!(
        viewed.iter().any(|doc| doc.contains("summary.txt")),
        "summary.txt not shown"
    );
    assert!(
        viewed.iter().any(|doc| doc.contains("log.txt")),
        "log.txt not shown"
    );
    assert_eq!(
        rig.network_attempts(),
        0,
        "the host must not upload a bundle"
    );
}

/// REQ: USR-334
#[test]
fn usr_334_bundle_for_one_plugin_holds_no_other_plugins_lines_and_include_host_adds_the_tail() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    rig.plant_log_line(NOTES, "notes-line");
    rig.plant_log_line(OTHER, "other-only-line");
    assert_eq!(rig.cli(&["support-bundle", NOTES]).exit_code, 0);
    let bundle = rig.last_bundle_path();
    let text = read_bundle_text(&bundle);
    assert!(
        text.contains("notes-line"),
        "the plugin's own lines must be present"
    );
    assert!(
        !text.contains("other-only-line"),
        "another plugin's lines leaked into the bundle"
    );

    let with_host = rig.cli(&["support-bundle", NOTES, "--include-host"]);
    assert_eq!(with_host.exit_code, 0, "{}", with_host.stderr);
    let host_text = read_bundle_text(&rig.last_bundle_path());
    assert!(
        host_text.contains("host log tail"),
        "the host log tail is missing"
    );
    assert!(
        host_text.contains("include-host"),
        "summary.txt must name --include-host"
    );
}

/// REQ: USR-335
#[test]
fn usr_335_bundle_name_and_summary_keys_follow_the_documented_format() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(rig.cli(&["support-bundle", NOTES]).exit_code, 0);
    let name = rig
        .last_bundle_path()
        .file_name()
        .expect("bundle file name")
        .to_string_lossy()
        .into_owned();
    let prefix = format!("ilium-support-{NOTES}-");
    assert!(name.starts_with(&prefix), "bad bundle name: {name}");
    let stamp = &name[prefix.len()..name.len() - ".zip".len()];
    assert_eq!(stamp.len(), 15, "YYYYMMDD-HHmmss expected: {stamp}");
    assert_eq!(stamp.as_bytes()[8], b'-');
    assert!(stamp
        .chars()
        .enumerate()
        .all(|(index, c)| index == 8 || c.is_ascii_digit()));

    let text = read_bundle_text(&rig.last_bundle_path());
    let summary = text
        .split("summary.txt")
        .nth(1)
        .expect("summary.txt entry")
        .to_string();
    for key in [
        "ilium version",
        "plugin id",
        "plugin version",
        "tier",
        "state",
        "platform",
        "host api range",
        "failure count",
        "redaction counts",
        "dropped log lines",
        "sha256",
    ] {
        assert!(
            summary
                .lines()
                .any(|line| line.to_lowercase().starts_with(&format!("{key}:"))),
            "summary lacks the key line {key}"
        );
    }
}

/// REQ: USR-336
#[test]
fn usr_336_bundle_builds_for_failed_plugins_within_5_seconds_for_a_4_mib_log_and_rejects_unknown_ids(
) {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.put_plugin_in_state(NOTES, "failed");
    rig.plant_log_bytes(NOTES, 4 * 1024 * 1024);
    let started = Instant::now();
    let output = rig.cli(&["support-bundle", NOTES]);
    assert_eq!(
        output.exit_code, 0,
        "a failed plugin must still bundle: {}",
        output.stderr
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "bundle took too long"
    );

    let unknown = rig.cli(&["support-bundle", "com.example.missing"]);
    assert_ne!(unknown.exit_code, 0);
    assert!(
        unknown.stderr.contains("bundle.plugin-unknown"),
        "{}",
        unknown.stderr
    );
}

/// REQ: USR-337
#[test]
fn usr_337_doctor_prints_one_ok_warn_or_fail_line_per_check_and_exits_0_only_without_fail() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["doctor"]);
    let lines: Vec<&str> = output.stdout_raw.lines().collect();
    for check in [
        "state folders",
        "policy file",
        "trust store",
        "installed.json",
        "disk space",
        "host API range",
        "language",
    ] {
        let line = lines
            .iter()
            .find(|line| line.contains(check))
            .unwrap_or_else(|| panic!("no doctor line for {check}"));
        let status = line.split_whitespace().next().unwrap_or_default();
        assert!(
            matches!(status, "ok" | "warn" | "fail"),
            "bad status in: {line}"
        );
    }
    let any_fail = lines.iter().any(|line| line.starts_with("fail"));
    assert_eq!(
        output.exit_code == 0,
        !any_fail,
        "exit 0 only when no line is fail"
    );

    rig.set_free_bytes(50 * 1024 * 1024);
    let low_disk = rig.cli(&["doctor"]);
    assert!(
        low_disk
            .stdout_raw
            .lines()
            .any(|line| line.starts_with("warn") && line.contains("disk space")),
        "free space below 100 MiB must warn"
    );
}

/// REQ: USR-332
#[test]
fn usr_332_bundle_shown_in_the_viewer_has_the_token_redacted() {
    assert_manual_says(
        "core-plugin-diagnostics",
        "the host MUST show the redacted `summary.txt` and `log.txt` in a viewer",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_log_line(NOTES, "token=abc123");
    assert_eq!(rig.cli(&["support-bundle", NOTES]).exit_code, 0);
    let viewed = rig.viewer_documents();
    assert!(
        viewed.iter().all(|doc| !doc.contains("abc123")),
        "the secret value must be redacted in every viewed document"
    );
}

/// REQ: USR-334
#[test]
fn usr_334_default_bundle_has_no_host_log_tail() {
    assert_manual_says(
        "core-plugin-diagnostics",
        "`--include-host` MUST add the host log tail (last 500 lines, redacted)",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(rig.cli(&["support-bundle", NOTES]).exit_code, 0);
    let text = read_bundle_text(&rig.last_bundle_path());
    assert!(
        !text.contains("host log tail"),
        "the host log tail is added only with --include-host"
    );
}

/// REQ: USR-335
#[test]
fn usr_335_summary_sha256_is_a_64_digit_hex_digest_of_the_other_files() {
    assert_manual_says(
        "core-plugin-diagnostics",
        "and a SHA-256 of the other three files",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(rig.cli(&["support-bundle", NOTES]).exit_code, 0);
    let text = read_bundle_text(&rig.last_bundle_path());
    let digest_line = text
        .lines()
        .find(|line| line.to_lowercase().starts_with("sha256:"))
        .expect("summary.txt has a sha256 line");
    let digest = digest_line["sha256:".len()..].trim();
    assert_eq!(digest.len(), 64, "a SHA-256 is 64 hex digits: {digest}");
    assert!(
        digest.chars().all(|c| c.is_ascii_hexdigit()),
        "the digest is hexadecimal: {digest}"
    );
}

/// REQ: USR-336
#[test]
fn usr_336_bundle_builds_for_a_blocked_plugin() {
    assert_manual_says(
        "core-plugin-diagnostics",
        "MUST work for a plugin in state `failed`, `blocked`, `incompatible` or missing helper",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.put_plugin_in_state(NOTES, "blocked");
    let output = rig.cli(&["support-bundle", NOTES]);
    assert_eq!(
        output.exit_code, 0,
        "a blocked plugin still gets a bundle: {}",
        output.stderr
    );
}

/// REQ: USR-337
#[test]
fn usr_337_unreadable_installed_json_is_reported_as_a_fail_line_and_exits_nonzero() {
    assert_manual_says("core-plugin-diagnostics", "`installed.json` readability");
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.corrupt_file(&rig.state_dir().join("installed.json"), b"{not json");
    let output = rig.cli(&["doctor"]);
    assert_ne!(
        output.exit_code, 0,
        "a fail line makes doctor exit non-zero"
    );
    assert!(
        output
            .stdout_raw
            .lines()
            .any(|line| line.starts_with("fail") && line.contains("installed.json")),
        "doctor reports installed.json as fail: {}",
        output.stdout_raw
    );
}
