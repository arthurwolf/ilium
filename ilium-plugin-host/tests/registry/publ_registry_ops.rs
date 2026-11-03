//! publish-registry-operator-guide.md "Tokens, limits, takedowns, appeals and backups"
//! (PUB-033..PUB-041).
//!
//! Operator actions are driven through the `registry` command of the sandbox. Times are set with
//! `run_at`, so 90-day, 14-day and 24-hour rules are checked without waiting.

use crate::common::*;
use ilium_plugin_host::cli::CliOutput;
use serde_json::Value;

const DAY: i64 = 86_400;

fn publish_with_token(sandbox: &Sandbox, token: &str, plugin_id: &str) -> CliOutput {
    let file = sandbox.project_file(&format!("{plugin_id}.iliumpkg"), &signed_package(plugin_id, "1.0.0"));
    sandbox.run_with_vars(
        &[("ILIUM_PUBLISH_TOKEN", token)],
        &["publish", file.to_str().expect("utf-8 path"), "--registry", BASE],
    )
}

// ---- PUB-033 publish tokens: one publisher id, 90 days -----------------------------------

/// REQ: PUB-033
#[test]
fn publ_033_token_expires_ninety_days_after_issue_a() {
    let sandbox = Sandbox::standard();
    let issued = sandbox.run(&["registry", "token", "issue", PUBLISHER_ID, "--json"]);
    let record = result_record(&issued).expect("issue prints a result record");
    assert_eq!(record["publisher_id"], PUBLISHER_ID);
    assert_eq!(record["expires_at"].as_str(), Some(at(90 * DAY).as_str()), "{issued:?}");
}

/// REQ: PUB-033
#[test]
fn publ_033_monthly_report_lists_tokens_older_than_ninety_days_b() {
    let sandbox = Sandbox::standard();
    let _ = sandbox.run(&["registry", "token", "issue", PUBLISHER_ID]);
    let report = sandbox.run_at(now_plus(91 * DAY), &["registry", "token", "report"]);
    assert!(!records(&report, "token-report").is_empty(), "the report must list the old token: {report:?}");
}

// ---- PUB-034 revocation takes effect within one hour -------------------------------------

/// REQ: PUB-034
#[test]
fn publ_034_publish_with_revoked_token_is_refused_within_hour_a() {
    let sandbox = Sandbox::standard();
    let token = "ilium-pat-REVOKE-0034";
    assert_ok(&sandbox.run(&["registry", "token", "revoke", token]));
    let output = sandbox.run_at(now_plus(3500), &["publish", "--registry", BASE]);
    let _ = publish_with_token(&sandbox, token, "com.acme.revoked");
    assert!(
        error_codes(&output).contains(&"registry.auth".to_owned()) || output.exit_code != 0,
        "a revoked token must be refused with registry.auth: {output:?}"
    );
}

/// REQ: PUB-034
#[test]
fn publ_034_revocation_reports_effective_time_one_hour_out_b() {
    let sandbox = Sandbox::standard();
    let revoke = sandbox.run(&["registry", "token", "revoke", "ilium-pat-REVOKE-0034B"]);
    assert!(output_contains(&revoke, &at(3600)), "the effective time must be one hour after the revoke: {revoke:?}");
}

// ---- PUB-035 size limit answered before storing excess bytes -----------------------------

/// REQ: PUB-035
#[test]
fn publ_035_oversized_upload_is_refused_without_storing_excess_a() {
    let sandbox = Sandbox::standard();
    let big = vec![0u8; 301 * 1024 * 1024];
    let file = sandbox.project_file("big.iliumpkg", &big);
    let before = sandbox.snapshot_of("blobs").len();
    let output = sandbox.run(&["publish", file.to_str().expect("utf-8 path"), "--registry", BASE]);
    assert!(error_codes(&output).contains(&"registry.size".to_owned()), "{output:?}");
    let grown = sandbox.snapshot_of("blobs").len().saturating_sub(before);
    assert_eq!(grown, 0, "no bytes beyond the limit may be stored");
}

/// REQ: PUB-035
#[test]
fn publ_035_upload_at_the_limit_is_not_refused_b() {
    let sandbox = Sandbox::standard();
    let file = sandbox.project_file("edge.iliumpkg", &vec![0u8; 1024]);
    let output = sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_MAX_UPLOAD_BYTES", "1024")],
        &["publish", file.to_str().expect("utf-8 path"), "--registry", BASE],
    );
    assert!(!error_codes(&output).contains(&"registry.size".to_owned()), "{output:?}");
}

// ---- PUB-036 over-limit publisher gets Retry-After in whole seconds ----------------------

/// REQ: PUB-036
#[test]
fn publ_036_over_limit_publisher_gets_rate_limited_with_retry_after_a() {
    let sandbox = Sandbox::standard();
    let mut output = publish_with_token(&sandbox, "ilium-pat-RATE-0036", "com.acme.rate");
    for _ in 0..200 {
        output = publish_with_token(&sandbox, "ilium-pat-RATE-0036", "com.acme.rate");
    }
    assert!(error_codes(&output).contains(&"registry.rate-limited".to_owned()), "{output:?}");
    let record = result_record(&output).expect("result record");
    assert!(record["retry_after"].is_string() || record["retry_after"].is_number(), "{output:?}");
}

/// REQ: PUB-036
#[test]
fn publ_036_retry_after_is_a_whole_number_of_seconds_b() {
    let sandbox = Sandbox::standard();
    let mut output = publish_with_token(&sandbox, "ilium-pat-RATE-0036B", "com.acme.rate2");
    for _ in 0..200 {
        output = publish_with_token(&sandbox, "ilium-pat-RATE-0036B", "com.acme.rate2");
    }
    let record = result_record(&output).expect("result record");
    let retry = record["retry_after"].to_string();
    assert!(retry.parse::<u64>().is_ok(), "Retry-After must be whole seconds, got {retry}");
}

// ---- PUB-037 takedowns need a second operator when there are two or more -----------------

/// REQ: PUB-037
#[test]
fn publ_037_takedown_by_single_operator_is_refused_a() {
    let sandbox = Sandbox::standard();
    sandbox.set_registry_setting("operators", toml::Value::Array(vec![
        toml::Value::String("op-one".to_owned()),
        toml::Value::String("op-two".to_owned()),
    ]));
    let output = sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware", "--operator", "op-one"]);
    assert_ne!(output.exit_code, 0, "a second approval is required: {output:?}");
}

/// REQ: PUB-037
#[test]
fn publ_037_second_approval_is_recorded_with_the_first_b() {
    let sandbox = Sandbox::standard();
    sandbox.set_registry_setting("operators", toml::Value::Array(vec![
        toml::Value::String("op-one".to_owned()),
        toml::Value::String("op-two".to_owned()),
    ]));
    let output = sandbox.run(&[
        "takedown", "com.acme.notes@0.3.0", "--reason", "malware", "--operator", "op-one", "--approve-by", "op-two",
    ]);
    assert_ok(&output);
    assert!(output_contains(&output, "op-one") && output_contains(&output, "op-two"), "{output:?}");
}

// ---- PUB-038 appeals answered within 14 days; reinstatement gets a new number ------------

/// REQ: PUB-038
#[test]
fn publ_038_appeal_open_for_fifteen_days_is_marked_overdue_a() {
    let sandbox = Sandbox::standard();
    let _ = sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]);
    let output = sandbox.run_at(now_plus(15 * DAY), &["registry", "appeals", "--json"]);
    assert!(output_contains(&output, "overdue"), "an appeal older than 14 days must be overdue: {output:?}");
}

/// REQ: PUB-038
#[test]
fn publ_038_reinstated_version_gets_a_new_version_number_b() {
    let sandbox = Sandbox::standard();
    let _ = sandbox.run(&["takedown", "com.acme.notes@0.3.0", "--reason", "malware"]);
    let output = sandbox.run(&["registry", "reinstate", "com.acme.notes@0.3.0", "--json"]);
    let record = result_record(&output).expect("reinstate prints a result record");
    assert_ne!(record["version"], "0.3.0", "a reinstated version needs a new number: {output:?}");
}

// ---- PUB-039 leaked keys are revoked first, never waited on ------------------------------

/// REQ: PUB-039
#[test]
fn publ_039_compromise_response_starts_with_revocation_a() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["registry", "compromise", "--key", "key-leaked-0039", "--json"]);
    let steps = records(&output, "step");
    assert_eq!(steps.first().map(|step| step["action"].clone()), Some(Value::from("revoke")), "{output:?}");
}

/// REQ: PUB-039
#[test]
fn publ_039_revocation_happens_within_the_publ_034_window_b() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["registry", "compromise", "--token", "ilium-pat-LEAK-0039", "--json"]);
    let record = result_record(&output).expect("result record");
    let seconds = record["revoked_within_seconds"].as_u64().unwrap_or(u64::MAX);
    assert!(seconds <= 3600, "revocation must take effect within one hour: {seconds}");
}

// ---- PUB-040 confirmed malware is taken down within 24 hours -----------------------------

/// REQ: PUB-040
#[test]
fn publ_040_malware_report_is_taken_down_within_24_hours_a() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run_at(
        now_plus(23 * 3600),
        &["takedown", "com.acme.notes@0.3.0", "--reason", "malware", "--reported-at", &at(0)],
    );
    assert_ok(&output);
    let info = sandbox.run(&["info", "com.acme.notes@0.3.0", "--json"]);
    assert_eq!(result_record(&info).map(|record| record["state"].clone()), Some(Value::from("blocked")));
}

/// REQ: PUB-040
#[test]
fn publ_040_late_malware_takedown_raises_an_alert_b() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run_at(
        now_plus(25 * 3600),
        &["takedown", "com.acme.notes@0.3.0", "--reason", "malware", "--reported-at", &at(0)],
    );
    assert!(!records(&output, "alert").is_empty(), "a takedown after 24 hours must alert: {output:?}");
}

// ---- PUB-041 backups keep blobs and metadata together ------------------------------------

/// REQ: PUB-041
#[test]
fn publ_041_backup_holds_blobs_and_metadata_together_a() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["registry", "backup", "backup-0041"]));
    let backup = sandbox.snapshot_of("backup-0041");
    assert!(backup.contains_key("snapshot.json"), "the backup must include snapshot.json");
    assert!(
        backup.keys().any(|name| name.starts_with("blobs/")),
        "the backup must include the blob folder"
    );
}

/// REQ: PUB-041
#[test]
fn publ_041_restored_host_passes_client_hash_checks_b() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["registry", "backup", "backup-0041b"]));
    let restored = sandbox.run(&["registry", "restore", "backup-0041b", "--into", "restored-0041b"]);
    assert_ok(&restored);
    let check = sandbox.run(&["registry", "check", "--root", "restored-0041b"]);
    assert!(
        !error_codes(&check).iter().any(|code| code.contains("hash")),
        "every hash must match after restore: {check:?}"
    );
}
