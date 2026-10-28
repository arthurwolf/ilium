//! Requirements SEC-953 .. SEC-957: audit and crash-record retention (reference-admin-policy.md and
//! reference-audit-log.md), the torn final audit line, and `ilium plugin audit --verify` exit codes.

use ilium_plugin_host::audit::{AuditActor, AuditContext, AuditLog, AuditRecord, AuditRetention};
use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};
use serde_json::{json, Value};

use crate::support::*;

const DAY_MS: u64 = 86_400_000;

/// An audit log with kernel provenance and the given retention.
fn open_log(path: &std::path::Path, retention: AuditRetention) -> AuditLog {
    AuditLog::open_with_context(
        path.to_path_buf(),
        Some(AuditContext {
            session: "default".to_owned(),
            actor: AuditActor { kind: "kernel".to_owned(), pane: None, plugin: None },
        }),
        retention,
    )
    .expect("audit log opens")
}

fn safe_mode_record(at_unix_millis: u64) -> AuditRecord {
    AuditRecord {
        at_unix_millis,
        plugin: String::new(),
        action: "safe-mode".to_owned(),
        detail_json: json!({ "result": "ok" }).to_string(),
    }
}

/// Every parsed line of every audit file (active and rotated) in `dir`, with its file name.
fn all_audit_lines(dir: &std::path::Path) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).expect("audit directory").flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("audit") && name.ends_with(".jsonl")) {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
        for line in text.lines() {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                found.push((name.clone(), value));
            }
        }
    }
    found
}

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
    let root = scratch();
    let path = root.path().join("audit").join("audit.jsonl");
    let mut log = open_log(&path, AuditRetention { max_bytes: 600, keep_days: 1 });
    // Old records, written on day 0 and spread over several files by the 600-byte ceiling.
    for index in 0..6 {
        log.append_at(&safe_mode_record(index), index).expect("old record");
    }
    // Young records on day 3 hit the ceiling again; the old files are past keep_days.
    let young_start = 3 * DAY_MS;
    for index in 0..6 {
        log.append_at(&safe_mode_record(young_start + index), young_start + index)
            .expect("young record");
    }
    let lines = all_audit_lines(path.parent().expect("audit directory"));
    let young_kept = lines
        .iter()
        .filter(|(_, line)| line["ts"].as_str().is_some_and(|ts| ts.starts_with("1970-01-04")))
        .count();
    assert_eq!(young_kept, 6, "no file younger than keep_days may be deleted at the ceiling");
    assert!(
        lines.iter().any(|(name, _)| name != "audit.jsonl"),
        "the ceiling must have rotated at least one younger file"
    );
    assert!(
        lines
            .iter()
            .all(|(_, line)| !line["ts"].as_str().is_some_and(|ts| ts.starts_with("1970-01-01"))),
        "rotated files older than keep_days are the ones deleted"
    );
    log.verify().expect("the retained chain verifies");
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
    let root = scratch();
    let path = root.path().join("audit").join("audit.jsonl");
    let mut log = open_log(&path, AuditRetention { max_bytes: 600, keep_days: 1 });
    for index in 0..6 {
        log.append_at(&safe_mode_record(index), index).expect("old record");
    }
    let young_start = 3 * DAY_MS;
    for index in 0..6 {
        log.append_at(&safe_mode_record(young_start + index), young_start + index)
            .expect("young record");
    }
    let lines = all_audit_lines(path.parent().expect("audit directory"));
    let deletion_records: Vec<&Value> = lines
        .iter()
        .map(|(_, line)| line)
        .filter(|line| line["event"] == "audit-retention-deleted")
        .collect();
    assert!(
        !deletion_records.is_empty(),
        "deleting an old audit file must leave an audit-retention-deleted record"
    );
    for record in deletion_records {
        let rule = record["reason"].as_str().unwrap_or_default();
        assert!(
            rule == "keep-days" || rule == "storage-ceiling",
            "the record names the rule that applied, got {record}"
        );
    }
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
    for purge in [false, true] {
        let ops = OpsRig::new();
        let notes = "com.example.notes";
        let other = "com.example.other";
        for id in [notes, other] {
            let package = ops.write_package(&PackageSpec::new(id, "1.0.0"));
            let package_text = package.to_string_lossy().into_owned();
            let installed = ops.cli(&["install", &package_text, "--yes", "--allow-unsigned"]);
            assert_eq!(installed.exit_code, 0, "{:?}", installed.stderr);
        }
        ops.plant_crash_record(notes, 3);
        ops.plant_crash_record(other, 2);
        let records_file = ilium_plugin_host::crash_records::records_path(&ops.state_dir());
        let before = std::fs::read_to_string(&records_file).expect("records planted");
        assert_eq!(before.matches(&format!("plugin={notes} ")).count(), 3);
        let mut command = vec!["plugin", "uninstall", notes];
        if purge {
            command.push("--purge");
        }
        let output = ops.cli(&command);
        assert_eq!(output.exit_code, 0, "{:?}", output.stderr);
        let after = std::fs::read_to_string(&records_file).unwrap_or_default();
        assert!(
            !after.contains(&format!("plugin={notes} ")),
            "uninstall (purge = {purge}) must delete every crash record of the plugin: {after}"
        );
        assert_eq!(
            after.matches(&format!("plugin={other} ")).count(),
            2,
            "the crash records of another plugin stay"
        );
    }
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
    let root = scratch();
    let audit_dir = root.path().join("audit");
    let path = audit_dir.join("audit.jsonl");
    {
        let mut log = open_log(&path, AuditRetention::default());
        log.append_at(&safe_mode_record(1_000), 1_000).expect("first record");
        log.append_at(&safe_mode_record(2_000), 2_000).expect("second record");
    }
    let complete = std::fs::read(&path).expect("complete file");
    let mut damaged = complete.clone();
    damaged.extend_from_slice(b"{\"ts\":\"1970-01-01T00:00:03.000Z\",\"seq\":3,\"prev_h");
    std::fs::write(&path, &damaged).expect("damaged file");
    let mut log = open_log(&path, AuditRetention::default());
    let torn_file = audit_dir.join("audit-torn-3.jsonl");
    assert_eq!(
        std::fs::read(&torn_file).expect("the partial bytes are moved to audit-torn-<seq>.jsonl"),
        &damaged[complete.len()..],
        "the moved bytes are exactly the partial final line"
    );
    assert_eq!(
        std::fs::read(&path).expect("truncated file").get(..complete.len()),
        Some(complete.as_slice()),
        "audit.jsonl keeps every complete record"
    );
    log.append_at(&safe_mode_record(4_000), 4_000).expect("append after repair");
    log.verify().expect("the chain resumes and verifies after the repair");
    let lines = read_jsonl(&path);
    assert_eq!(lines[2]["event"], "audit-torn", "an audit-torn record follows the last complete record");
    assert_eq!(
        lines[2]["prev_hash"],
        sha256_hex(String::from_utf8_lossy(&complete).lines().last().expect("last line").as_bytes()),
        "the audit-torn record chains from the last complete record"
    );
}

/// REQ: SEC-957
#[test]
fn sec_957_a_torn_tail_alone_is_reported_as_torn_tail_and_exits_zero() {
    let lines = chain_lines(&["plugin.installed", "plugin.started"]);
    let root = scratch();
    let env = env_with_audit(root.path(), &lines);
    let audit_path = env.state_dir.join("audit").join("audit.jsonl");
    let complete_len = std::fs::metadata(&audit_path).expect("audit file").len();
    let mut file = std::fs::OpenOptions::new().append(true).open(&audit_path).expect("append");
    std::io::Write::write_all(&mut file, b"{\"ts\":\"2026-10-12T08:14:").expect("torn bytes");
    drop(file);
    let output = ilium_plugin_host::cli::run(&args(&["plugin", "audit", "--verify"]), &env);
    assert_eq!(output.exit_code, 0, "a torn tail alone exits 0: {}", output.stderr);
    let combined = format!("{:?} {}", output.stdout_jsonl, output.stderr);
    assert!(combined.contains("torn-tail"), "the finding is named torn-tail: {combined}");
    assert!(
        combined.contains(&complete_len.to_string()),
        "the finding carries the byte offset {complete_len}: {combined}"
    );
    assert!(!combined.contains("broken"), "a torn tail is not a broken chain: {combined}");
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
