//! Gap tests for the plugin diagnostics page: the `ilium plugin bundle` archive, its 5 MiB cap
//! and the failure-offer rules (LIF-312..314).
//!
//! Assumptions: `bundle_entries` lists the archive member names, `bundle_text` returns the text
//! of the archive members, `bundle_index` returns the index file text; `answer_bundle_offer`
//! answers the offer shown after a plugin enters `failed`; `bundle_files` lists written bundles.

use std::path::PathBuf;

use crate::support::*;

const FIVE_MIB: u64 = 5 * 1024 * 1024;

/// Builds a bundle for `NOTES` into the rig's scratch directory and returns its path.
fn build_bundle(rig: &OpsRig) -> PathBuf {
    let path = rig.scratch_dir().join("bundle.tar");
    let path_text = path.to_string_lossy().into_owned();
    let output = rig.cli(&["plugin", "bundle", NOTES, "--out", &path_text]);
    assert_eq!(
        output.exit_code, 0,
        "bundle must succeed: {}",
        output.stderr
    );
    path
}

/// REQ: LIF-312
#[test]
fn lif_312_bundle_holds_report_logs_manifest_states_and_version_without_secrets() {
    assert_manual_says(
        "core-plugin-diagnostics",
        "`ilium plugin bundle` MUST write one archive that holds the report",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_setting(NOTES, "api_token", "SECRET-TOKEN-VALUE");
    rig.write_storage(NOTES, "global", "draft", b"STORAGE-BUCKET-VALUE");
    let path = build_bundle(&rig);
    let entries = rig.bundle_entries(&path);
    for required in ["report", "manifest", "states", "version"] {
        assert!(
            entries.iter().any(|entry| entry.contains(required)),
            "bundle holds a {required} member: {entries:?}"
        );
    }
    assert!(
        entries.iter().any(|entry| entry.contains("logs")),
        "bundle holds the log records"
    );
    assert!(
        !rig.bundle_text(&path).contains("SECRET-TOKEN-VALUE"),
        "no secrets"
    );
    assert!(
        !rig.bundle_text(&path).contains("STORAGE-BUCKET-VALUE"),
        "no storage buckets"
    );
}

/// REQ: LIF-313
#[test]
fn lif_313_bundle_is_capped_at_5_mib_drops_oldest_logs_and_names_the_cut() {
    assert_manual_says(
        "core-plugin-diagnostics",
        "The bundle MUST be capped at 5 MiB",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    for index in 0..60_000u32 {
        rig.log_line(NOTES, &format!("record-{index:06} {}", "x".repeat(120)));
    }
    let path = build_bundle(&rig);
    let size = std::fs::metadata(&path).expect("bundle exists").len();
    assert!(size <= FIVE_MIB, "bundle is {size} bytes, above 5 MiB");
    let text = rig.bundle_text(&path);
    assert!(
        !text.contains("record-000000"),
        "the oldest record is dropped first"
    );
    assert!(text.contains("record-059999"), "the newest record is kept");
    assert!(
        rig.bundle_index(&path).contains("cut"),
        "the index names the cut"
    );
}

/// REQ: LIF-313
#[test]
fn lif_313_bundle_replaces_home_directory_with_tilde_in_paths() {
    assert_manual_says(
        "core-plugin-diagnostics",
        "the home directory MUST be replaced by `~`",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let home = rig.home_dir();
    rig.log_line(NOTES, &format!("opened {}/notes.md", home.display()));
    let path = build_bundle(&rig);
    let text = rig.bundle_text(&path);
    assert!(
        text.contains("~/notes.md"),
        "the log path uses ~ for the home directory"
    );
    assert!(
        !text.contains(&home.display().to_string()),
        "the real home directory is absent from the bundle"
    );
}

/// REQ: LIF-314
#[test]
fn lif_314_bundle_is_written_only_after_consent_and_old_bundles_are_deleted_at_start() {
    assert_manual_says(
        "core-plugin-diagnostics",
        "The host MUST offer a bundle after a plugin enters `failed`",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    for _ in 0..5 {
        server.fail(NOTES, Side::Server);
    }
    rig.answer_bundle_offer("decline");
    assert!(rig.bundle_files().is_empty(), "no bundle without consent");
    server.fail(NOTES, Side::Server);
    rig.answer_bundle_offer("accept");
    assert_eq!(rig.bundle_files().len(), 1, "one bundle after consent");
    rig.age_bundles(days(15));
    server.shutdown();
    rig.block_network();
    let _restarted = rig.start_server("default");
    assert!(
        rig.bundle_files().is_empty(),
        "bundles older than 14 days are deleted"
    );
    assert_eq!(rig.network_attempts(), 0, "no bundle is sent anywhere");
}
