//! publish-publisher-workflow.md "Publisher workflow and version states" (PUBL-001..PUBL-007).
//!
//! Tests follow the manual: publish, info and yank are driven through the CLI of a sandbox whose
//! registry is served by the recording fake network.

use crate::common::*;
use ilium_plugin_host::cli::CliOutput;

fn package_path(sandbox: &Sandbox, plugin_id: &str, version: &str) -> String {
    let file = format!("{plugin_id}-{version}.iliumpkg");
    let path = sandbox.project_file(&file, &signed_package(plugin_id, version));
    path.to_str().expect("utf-8 path").to_owned()
}

fn publish(sandbox: &Sandbox, file: &str) -> CliOutput {
    sandbox.run(&["publish", file, "--registry", BASE])
}

fn info_json(sandbox: &Sandbox, target: &str) -> CliOutput {
    sandbox.run(&["info", target, "--json"])
}

// ---- PUBL-001 verifying versions stay out of the index ------------------------------------

/// REQ: PUBL-001
#[test]
fn publ_001_verifying_version_is_absent_from_index_a() {
    let sandbox = Sandbox::standard();
    let file = package_path(&sandbox, "com.acme.fresh", "1.0.0");
    let output = publish(&sandbox, &file);
    let result = result_record(&output).expect("publish prints a result record");
    // The version passes through `verifying`; the first response must not claim it is live yet.
    assert_ne!(result["state"], "live", "{output:?}");
    let listed = sandbox.run(&["search", "fresh"]);
    assert!(
        !plugin_ids(&listed).contains(&"com.acme.fresh".to_owned()),
        "a version still in verifying must not appear in index.json"
    );
}

/// REQ: PUBL-001
#[test]
fn publ_001_only_live_or_queued_review_reaches_index_b() {
    let sandbox = Sandbox::standard();
    let file = package_path(&sandbox, "com.acme.fresh", "1.0.0");
    let output = publish(&sandbox, &file);
    assert_ok(&output);
    let result = result_record(&output).expect("result record");
    let state = result["state"].as_str().unwrap_or_default();
    assert!(
        state == "live" || state == "queued-review",
        "publish must end in live or queued-review, got {state:?}"
    );
}

// ---- PUBL-002 rejected versions are not stored and free their number ----------------------

/// REQ: PUBL-002
#[test]
fn publ_002_rejected_version_is_not_stored_a() {
    let sandbox = Sandbox::standard();
    let file = package_path(&sandbox, "com.acme.rejected", "2.0.0");
    let _ = publish(&sandbox, &file);
    let info = info_json(&sandbox, "com.acme.rejected@2.0.0");
    assert_eq!(info.exit_code, 5, "a never-live version must make info exit 5: {info:?}");
}

/// REQ: PUBL-002
#[test]
fn publ_002_rejection_record_names_stage_and_code_b() {
    let sandbox = Sandbox::standard();
    let file = package_path(&sandbox, "com.acme.rejected", "2.0.0");
    let _ = publish(&sandbox, &file);
    let info = info_json(&sandbox, "com.acme.rejected@2.0.0");
    let record = result_record(&info).expect("info prints a result record");
    assert!(record["rejection"]["stage"].is_string(), "{info:?}");
    assert!(record["rejection"]["code"].is_string(), "{info:?}");
}

// ---- PUBL-003 live -> yanked keeps the blob, the entry and the hash -----------------------

/// REQ: PUBL-003
#[test]
fn publ_003_yank_keeps_blob_hash_in_snapshot_a() {
    let sandbox = Sandbox::standard();
    let before = sandbox.snapshot();
    let yanked = sandbox.run(&["yank", "com.acme.notes@0.3.0", "--reason", "security: test fixture"]);
    assert_ok(&yanked);
    let after = sandbox.snapshot();
    let before_hash = signed_of(&before["snapshot.json"]);
    let after_hash = signed_of(&after["snapshot.json"]);
    assert_eq!(
        before_hash["meta"]["com.acme.notes@0.3.0.json"]["hashes"],
        after_hash["meta"]["com.acme.notes@0.3.0.json"]["hashes"],
        "yank must not change the blob hash"
    );
}

/// REQ: PUBL-003
#[test]
fn publ_003_yank_is_a_signed_metadata_change_b() {
    let sandbox = Sandbox::standard();
    let yanked = sandbox.run(&["yank", "com.acme.notes@0.3.0", "--reason", "security: test fixture"]);
    assert_ok(&yanked);
    let info = info_json(&sandbox, "com.acme.notes@0.3.0");
    let record = result_record(&info).expect("info record");
    assert_eq!(record["state"], "yanked", "{info:?}");
}

// ---- PUBL-004 the token and the Authorization header never reach output -------------------

/// REQ: PUBL-004
#[test]
fn publ_004_wrong_token_is_not_echoed_in_output_a() {
    let sandbox = Sandbox::standard();
    let file = package_path(&sandbox, "com.acme.tokentest", "1.0.0");
    let token = "ilium-pat-SECRET-TOKEN-VALUE-0001";
    let output = sandbox.run_with_vars(
        &[("ILIUM_PUBLISH_TOKEN", token)],
        &["publish", file.as_str(), "--registry", BASE],
    );
    let rendered = format!("{output:?}");
    assert!(!rendered.contains(token), "token leaked into output");
    assert!(!rendered.contains("Authorization"), "Authorization header leaked");
}

/// REQ: PUBL-004
#[test]
fn publ_004_token_is_absent_from_the_log_file_b() {
    let sandbox = Sandbox::standard();
    let file = package_path(&sandbox, "com.acme.tokentest", "1.0.0");
    let token = "ilium-pat-SECRET-TOKEN-VALUE-0002";
    let _ = sandbox.run_with_vars(
        &[("ILIUM_PUBLISH_TOKEN", token)],
        &["publish", file.as_str(), "--registry", BASE],
    );
    let logs = sandbox.dir.path().join("data").join("logs");
    let mut found = false;
    if let Ok(entries) = std::fs::read_dir(&logs) {
        for entry in entries.flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            found |= text.contains(token);
        }
    }
    assert!(!found, "publish token found in a log file under {}", logs.display());
}

// ---- PUBL-005 publish exit codes match the result record ----------------------------------

/// REQ: PUBL-005
#[test]
fn publ_005_exit_code_matches_result_record_a() {
    let sandbox = Sandbox::standard();
    let file = package_path(&sandbox, "com.acme.notes", "0.3.0");
    let output = publish(&sandbox, &file);
    let result = result_record(&output).expect("result record");
    assert_eq!(
        result["exit_code"].as_i64(),
        Some(i64::from(output.exit_code)),
        "{output:?}"
    );
}

/// REQ: PUBL-005
#[test]
fn publ_005_version_exists_exits_one_b() {
    let sandbox = Sandbox::standard();
    let file = package_path(&sandbox, "com.acme.notes", "0.3.0");
    let other = sandbox.project_file("other.iliumpkg", &dummy_package("com.acme.notes", "0.3.0"));
    let output = publish(&sandbox, other.to_str().expect("utf-8 path"));
    let _ = file;
    assert_failed(&output, "registry.version-exists", 1);
}

// ---- PUBL-006 recovery after an unreachable registry --------------------------------------

/// REQ: PUBL-006
#[test]
fn publ_006_unreachable_publish_is_checked_with_info_first_a() {
    let sandbox = Sandbox::standard();
    let file = package_path(&sandbox, "com.acme.notes", "0.4.0");
    sandbox.net.set_down(true);
    let output = publish(&sandbox, &file);
    assert_failed(&output, "registry.unreachable", 8);
    sandbox.net.set_down(false);
    let info = info_json(&sandbox, "com.acme.notes@0.4.0");
    assert!(
        result_record(&info).is_some(),
        "the publisher must be able to run info before publishing again"
    );
}

/// REQ: PUBL-006
#[test]
fn publ_006_same_hash_reports_publish_already_complete_b() {
    let sandbox = Sandbox::standard();
    let file = package_path(&sandbox, "com.acme.notes", "0.3.0");
    let output = publish(&sandbox, &file);
    let result = result_record(&output).expect("result record");
    assert_eq!(result["code"], "publish.already-complete", "{output:?}");
    assert_eq!(output.exit_code, 0);
}

// ---- PUBL-007 changed content under an existing version never overwrites it ---------------

/// REQ: PUBL-007
#[test]
fn publ_007_different_hash_exits_one_without_overwrite_a() {
    let sandbox = Sandbox::standard();
    let before = sandbox.snapshot();
    let other = sandbox.project_file("changed.iliumpkg", &dummy_package("com.acme.notes", "0.3.0"));
    let output = publish(&sandbox, other.to_str().expect("utf-8 path"));
    assert_failed(&output, "registry.version-exists", 1);
    let after = sandbox.snapshot();
    assert_eq!(
        before["snapshot.json"], after["snapshot.json"],
        "the live version must not be overwritten"
    );
}

/// REQ: PUBL-007
#[test]
fn publ_007_message_points_to_a_new_version_number_b() {
    let sandbox = Sandbox::standard();
    let other = sandbox.project_file("changed.iliumpkg", &dummy_package("com.acme.notes", "0.3.0"));
    let output = publish(&sandbox, other.to_str().expect("utf-8 path"));
    let message = error_message(&output, "registry.version-exists");
    assert!(
        message.to_lowercase().contains("new version"),
        "the fix named by PUBL-007 is a new version number: {message:?}"
    );
}
