//! Requirements SEC-953 .. SEC-957: audit and crash-record retention (reference-admin-policy.md and
//! reference-audit-log.md), the torn final audit line, and `ilium plugin audit --verify` exit codes.

use serde_json::json;

use crate::support::*;

/// A chained audit file: each record carries the SHA-256 of the previous line, as the host writes it.
fn chain_lines(events: &[&str]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for (index, event) in events.iter().enumerate() {
        let prev_hash = match lines.last() {
            Some(last) => sha256_hex(last.as_bytes()),
            None => zero_hash(),
        };
        let record = json!({
            "ts": format!("2026-10-12T08:14:{:02}.114Z", index % 60),
            "seq": index + 1,
            "prev_hash": prev_hash,
            "session": "default",
            "actor": { "kind": "kernel" },
            "event": event,
            "plugin_id": "example.demo",
            "result": "ok",
        });
        lines.push(record.to_string());
    }
    lines
}

/// The CLI environment with `lines` written as its audit file.
fn env_with_audit(root: &std::path::Path, lines: &[String]) -> ilium_plugin_host::cli::CliEnv {
    let env = cli_env(root);
    let dir = env.state_dir.join("audit");
    std::fs::create_dir_all(&dir).expect("audit directory");
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(dir.join("audit.jsonl"), text).expect("audit file");
    env
}

/// REQ: SEC-953
#[test]
fn sec_953_the_policy_page_makes_audit_keep_days_the_minimum_retention_for_rotated_files() {
    let page = manual_page("reference-admin-policy.md");
    assert!(
        page.contains("`audit.keep_days` is the minimum retention for audit files"),
        "the page must name audit.keep_days as the minimum retention"
    );
    assert!(
        page.contains("MUST NOT delete a rotated audit file younger than `audit.keep_days`"),
        "the page must forbid deleting a younger rotated file even at the ceiling"
    );
}

/// REQ: SEC-953
#[test]
fn sec_953_at_the_storage_ceiling_only_the_file_older_than_keep_days_is_deleted() {
    unimplemented!("ilium_plugin_host::audit::storage_ceiling_deletes_only_files_older_than_keep_days")
}

/// REQ: SEC-954
#[test]
fn sec_954_the_admin_policy_page_requires_an_audit_retention_record_before_each_deletion() {
    let page = manual_page("reference-admin-policy.md");
    assert!(
        page.contains("MUST write one `audit-retention-deleted` record first"),
        "the page must require the audit-retention-deleted record first"
    );
    assert!(
        page.contains("the rule that applied (`keep-days` or `storage-ceiling`)"),
        "the record must name the rule that applied"
    );
}

/// REQ: SEC-954
#[test]
fn sec_954_each_retention_deletion_writes_its_record_before_the_file_is_gone() {
    unimplemented!("ilium_plugin_host::audit::retention_deletion_writes_audit_retention_deleted_first")
}

/// REQ: SEC-955
#[test]
fn sec_955_the_audit_log_page_sets_thirty_days_and_two_hundred_crash_records() {
    let page = manual_page("reference-audit-log.md");
    assert!(
        page.contains("a crash record MUST be deleted once it is more than 30 days old"),
        "the page must state the 30-day crash record period"
    );
    assert!(
        page.contains("the oldest MUST be deleted first until 200 remain"),
        "the page must state the 200-record count with oldest removed first"
    );
    assert!(
        page.contains("The 14-day period in [SEC-447] is superseded"),
        "the page must state that the 14-day period is superseded"
    );
}

/// REQ: SEC-955
#[test]
fn sec_955_uninstall_with_or_without_purge_deletes_every_crash_record_of_the_plugin() {
    unimplemented!("ilium_plugin_host::crash_records::uninstall_deletes_all_crash_records")
}

/// REQ: SEC-955
#[test]
fn sec_955_keep_data_does_not_keep_crash_records() {
    let page = manual_page("reference-audit-log.md");
    assert!(
        page.contains("Crash records are not deleted by `--keep-data`, which keeps data and storage only."),
        "the page must state that --keep-data does not keep crash records"
    );
}

/// REQ: SEC-956
#[test]
fn sec_956_the_audit_log_page_moves_a_torn_final_line_aside_and_writes_audit_torn() {
    let page = manual_page("reference-audit-log.md");
    assert!(
        page.contains("move those partial bytes to `audit-torn-<seq>.jsonl`"),
        "the page must name the torn-file move"
    );
    assert!(
        page.contains("The partial bytes are not part of the chain and MUST NOT be hashed."),
        "the page must exclude the partial bytes from the chain"
    );
    assert!(
        page.contains("MUST NOT drop the partial bytes without moving them aside"),
        "the page must forbid dropping the partial bytes"
    );
}

/// REQ: SEC-956
#[test]
fn sec_956_a_torn_final_line_is_moved_aside_and_the_chain_resumes_at_the_last_record() {
    unimplemented!("ilium_plugin_host::audit::open moves torn final line to audit-torn-<seq>.jsonl")
}

/// REQ: SEC-957
#[test]
fn sec_957_a_torn_tail_alone_is_reported_as_torn_tail_and_exits_zero() {
    unimplemented!("ilium_plugin_host::cli::run plugin audit --verify reports torn-tail with byte offset")
}

/// REQ: SEC-957
#[test]
fn sec_957_a_wrong_prev_hash_in_a_complete_record_is_a_broken_chain_with_exit_code_three() {
    let mut lines = chain_lines(&["plugin.installed", "plugin.started", "plugin.stopped"]);
    let previous = sha256_hex(lines[0].as_bytes());
    lines[1] = lines[1].replace(previous.as_str(), zero_hash().as_str());
    let root = scratch();
    let env = env_with_audit(root.path(), &lines);
    let output = ilium_plugin_host::cli::run(&args(&["plugin", "audit", "--verify"]), &env);
    assert_eq!(output.exit_code, 3, "a broken chain must exit 3: {}", output.stderr);
}
