use ilium_plugin_host::audit::{AuditActor, AuditContext, AuditLog, AuditRecord, AuditRetention};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};

fn open(path: &Path, retention: AuditRetention) -> AuditLog {
    AuditLog::open_with_context(
        path.into(),
        Some(AuditContext {
            session: "actual-test-session".into(),
            actor: AuditActor {
                kind: "kernel".into(),
                pane: None,
                plugin: None,
            },
        }),
        retention,
    )
    .unwrap()
}
fn record(at: u64, detail: Value) -> AuditRecord {
    AuditRecord {
        at_unix_millis: at,
        plugin: String::new(),
        action: "safe-mode".into(),
        detail_json: detail.to_string(),
    }
}
fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn concurrent_handles_read_sequence_under_lock_and_hash_exact_line_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut first = open(&path, AuditRetention::default());
    let mut second = open(&path, AuditRetention::default());
    first
        .append_at(&record(0, json!({"result":"ok"})), 0)
        .unwrap();
    second
        .append_at(&record(1, json!({"result":"ok"})), 1)
        .unwrap();
    let bytes = std::fs::read_to_string(&path).unwrap();
    let raw: Vec<_> = bytes.lines().collect();
    let records = lines(&path);
    assert_eq!(records[0]["ts"], "1970-01-01T00:00:00.000Z");
    assert_eq!(records[0]["seq"], 1);
    assert_eq!(records[1]["seq"], 2);
    assert_eq!(records[0]["prev_hash"], "0".repeat(64));
    assert_eq!(
        records[1]["prev_hash"],
        format!("{:x}", Sha256::digest(raw[0].as_bytes()))
    );
    assert_eq!(records[1]["session"], "actual-test-session");
    assert!(records[1].get("detail_json").is_none());
    first.verify().unwrap();
}

#[test]
fn metadata_is_strict_and_missing_provenance_is_refused_without_append() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(&path, AuditRetention::default());
    for detail in [
        "[]",
        "\"text\"",
        "{\"result\":\"ok\",\"prompt\":\"secret\"}",
        "{\"result\":\"ok\",\"result\":\"failed\"}",
    ] {
        let mut request = record(0, json!({"result":"ok"}));
        request.detail_json = detail.into();
        assert!(log.append_at(&request, 0).is_err());
    }
    assert!(std::fs::read(&path).unwrap().is_empty());
    let mut no_context = AuditLog::open(path).unwrap();
    assert!(
        no_context
            .append_at(&record(0, json!({"result":"ok"})), 0)
            .is_err()
    );
}

#[test]
fn secret_variants_are_redacted_but_short_values_are_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(&path, AuditRetention::default());
    log.set_secrets(&["abcdef".into(), "tiny".into()]);
    log.append_at(&record(0, json!({"result":"ok", "arguments":["abcdef", "YWJjZGVm", "%61%62%63%64%65%66", "616263646566", "tiny"]})), 0).unwrap();
    assert_eq!(
        lines(&path)[0]["arguments"],
        json!([
            "<redacted>",
            "<redacted>",
            "<redacted>",
            "<redacted>",
            "tiny"
        ])
    );
    log.verify().unwrap();
}

#[test]
fn rotation_preserves_chain_and_retention_can_prune_an_expired_prefix() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(
        &path,
        AuditRetention {
            max_bytes: 1,
            keep_days: 1,
        },
    );
    log.append_at(&record(0, json!({"result":"ok"})), 0)
        .unwrap();
    log.append_at(&record(1, json!({"result":"ok"})), 1)
        .unwrap();
    assert!(directory.path().join("audit-1.jsonl").exists());
    assert_eq!(lines(&path)[0]["seq"], 2);
    log.verify().unwrap();
    log.append_at(&record(172_800_000, json!({"result":"ok"})), 172_800_000)
        .unwrap();
    assert!(!directory.path().join("audit-1.jsonl").exists());
    assert!(!directory.path().join("audit-2.jsonl").exists());
    assert_eq!(lines(&path)[0]["seq"], 3);
    log.verify().unwrap();
}

#[test]
fn live_deletion_has_explicit_reset_and_tampering_is_not_silently_repaired() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(&path, AuditRetention::default());
    log.append_at(&record(0, json!({"result":"ok"})), 0)
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    log.append_at(&record(1, json!({"result":"ok"})), 1)
        .unwrap();
    let records = lines(&path);
    assert_eq!(records[0]["event"], "audit-reset");
    assert_eq!(records[0]["seq"], 2);
    assert_eq!(records[0]["prev_hash"], "0".repeat(64));
    assert_eq!(records[1]["seq"], 3);
    assert!(log.status().reset_detected);
    log.verify().unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        raw.replacen("actual-test-session", "edited-test-session", 1),
    )
    .unwrap();
    assert!(log.verify().is_err());
    assert!(
        log.append_at(&record(2, json!({"result":"ok"})), 2)
            .is_err()
    );
}

#[test]
fn write_failure_buffers_until_ten_second_retry_and_reports_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(&path, AuditRetention::default());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    log.append_at(&record(0, json!({"result":"ok"})), 0)
        .unwrap();
    assert_eq!(log.status().queued, 1);
    assert!(log.status().write_failed);
    std::fs::remove_dir(&path).unwrap();
    log.tick(Duration::from_secs(9), 9000).unwrap();
    assert_eq!(log.status().queued, 1);
    log.tick(Duration::from_secs(1), 10000).unwrap();
    assert_eq!(log.status().queued, 0);
    assert!(!log.status().write_failed);
    assert_eq!(lines(&path).last().unwrap()["event"], "safe-mode");
}

#[test]
fn refusal_attempts_are_counted_once_per_plugin_kind_minute() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(&path, AuditRetention::default());
    let mut request = record(
        0,
        json!({"result":"refused", "version":"1.0.0", "tier":"t1", "grant":"fs.read"}),
    );
    request.plugin = "example.audit".into();
    request.action = "h-grant-refused".into();
    for _ in 0..5 {
        log.append_at(&request, 0).unwrap();
    }
    assert_eq!(log.status().queued, 1);
    assert!(lines(&path).is_empty());
    log.tick(Duration::from_secs(59), 59_000).unwrap();
    assert!(lines(&path).is_empty());
    log.tick(Duration::from_secs(1), 60_000).unwrap();
    assert_eq!(lines(&path)[0]["count"], 5);
    assert_eq!(log.status().queued, 0);
}

#[test]
fn pending_failure_and_summary_storage_share_the_thousand_record_bound() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(&path, AuditRetention::default());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    for _ in 0..1002 {
        log.append_at(&record(0, json!({"result":"ok"})), 0)
            .unwrap();
    }
    assert_eq!(log.status().queued, 1000);
    assert_eq!(log.status().dropped, 2);
}

#[test]
fn leap_day_timestamp_and_backwards_wall_clock_do_not_change_sequence_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(&path, AuditRetention::default());
    log.append_at(
        &record(1_709_164_800_000, json!({"result":"ok"})),
        1_709_164_800_000,
    )
    .unwrap();
    log.append_at(&record(0, json!({"result":"ok"})), 0)
        .unwrap();
    let records = lines(&path);
    assert_eq!(records[0]["ts"], "2024-02-29T00:00:00.000Z");
    assert_eq!(records[1]["ts"], "1970-01-01T00:00:00.000Z");
    assert_eq!(records[1]["seq"], 2);
    log.verify().unwrap();
}

#[test]
fn explicit_host_call_kinds_do_not_merge_when_they_share_a_grant() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(&path, AuditRetention::default());
    let mut request = record(
        0,
        json!({"result":"refused", "version":"1.0.0", "tier":"t1", "grant":"fs.read"}),
    );
    request.plugin = "example.audit".into();
    request.action = "h-grant-refused".into();
    log.append_refusal_at(&request, "file-open", 0).unwrap();
    log.append_refusal_at(&request, "file-open", 0).unwrap();
    log.append_refusal_at(&request, "directory-list", 0)
        .unwrap();
    log.tick(Duration::from_secs(60), 60_000).unwrap();
    let records = lines(&path);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["count"], 2);
    assert_eq!(records[1]["count"], 1);
}

#[test]
fn refusal_counts_survive_shutdown_and_reopen_without_early_delivery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let request = refusal_record();
    {
        let mut log = open(&path, AuditRetention::default());
        log.append_refusal_at(&request, "file-open", 0).unwrap();
        log.append_refusal_at(&request, "file-open", 10_000)
            .unwrap();
    }
    let mut reopened = open(&path, AuditRetention::default());
    assert_eq!(reopened.status().queued, 1);
    reopened.tick(Duration::from_secs(59), 59_000).unwrap();
    assert!(lines(&path).is_empty());
    reopened.tick(Duration::from_secs(1), 60_000).unwrap();
    assert_eq!(lines(&path)[0]["count"], 2);
    assert_eq!(reopened.status().queued, 0);
}

fn refusal_record() -> AuditRecord {
    let mut request = record(
        0,
        json!({"result":"refused", "version":"1.0.0", "tier":"t1", "grant":"fs.read"}),
    );
    request.plugin = "example.audit".into();
    request.action = "h-grant-refused".into();
    request
}

#[test]
fn concurrent_handles_share_accounting_and_only_one_delivers_a_window() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut log = open(&path, AuditRetention::default());
                barrier.wait();
                for _ in 0..5 {
                    log.append_refusal_at(&refusal_record(), "file-open", 0)
                        .unwrap();
                }
                log
            })
        })
        .collect();
    let mut handles: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    for handle in &mut handles {
        handle.tick(Duration::from_secs(60), 60_000).unwrap();
    }
    let records = lines(&path);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["count"], 10);
    assert_eq!(records[0]["refusal_kind"], "file-open");
    handles[0].verify().unwrap();
}

#[test]
fn committed_summary_is_not_replayed_if_sidecar_acknowledgement_was_interrupted() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let state_path = path.with_extension("refusals.json");
    let mut log = open(&path, AuditRetention::default());
    log.append_refusal_at(&refusal_record(), "file-open", 0)
        .unwrap();
    let unacknowledged = std::fs::read(&state_path).unwrap();
    log.tick(Duration::from_secs(60), 60_000).unwrap();
    drop(log);
    // Recreate the exact durable sidecar state from before the successful append:
    // this is the crash boundary between audit commit and sidecar acknowledgement.
    std::fs::write(&state_path, unacknowledged).unwrap();
    let mut reopened = open(&path, AuditRetention::default());
    reopened.tick(Duration::from_secs(60), 60_000).unwrap();
    assert_eq!(lines(&path).len(), 1);
    reopened
        .append_refusal_at(&refusal_record(), "file-open", 60_000)
        .unwrap();
    reopened.tick(Duration::from_secs(59), 119_000).unwrap();
    assert_eq!(lines(&path).len(), 1);
    reopened.tick(Duration::from_secs(1), 120_000).unwrap();
    assert_eq!(lines(&path).len(), 2);
    assert_eq!(lines(&path)[1]["count"], 1);
    reopened.verify().unwrap();
}

#[test]
fn backward_wall_clock_cannot_reopen_a_completed_rate_window() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(&path, AuditRetention::default());
    log.append_refusal_at(&refusal_record(), "file-open", 0)
        .unwrap();
    log.tick(Duration::from_secs(60), 60_000).unwrap();
    drop(log);
    let mut reopened = open(&path, AuditRetention::default());
    reopened
        .append_refusal_at(&refusal_record(), "file-open", 0)
        .unwrap();
    reopened.tick(Duration::from_secs(60), 60_000).unwrap();
    assert_eq!(lines(&path).len(), 1);
    reopened.tick(Duration::from_secs(60), 120_000).unwrap();
    assert_eq!(lines(&path).len(), 2);
}

#[test]
fn malformed_durable_state_is_preserved_and_refused() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let mut log = open(&path, AuditRetention::default());
    log.append_refusal_at(&refusal_record(), "file-open", 0)
        .unwrap();
    let state_path = path.with_extension("refusals.json");
    std::fs::write(&state_path, b"not a state document").unwrap();
    assert!(
        log.append_refusal_at(&refusal_record(), "file-open", 1)
            .is_err()
    );
    assert!(AuditLog::open(path).is_err());
    assert_eq!(std::fs::read(state_path).unwrap(), b"not a state document");
}

#[test]
fn refusal_state_write_failure_preserves_retry_custody_then_becomes_reopen_safe() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let temporary = path.with_extension("refusals.tmp");
    let mut log = open(&path, AuditRetention::default());
    std::fs::create_dir(&temporary).unwrap();
    log.append_refusal_at(&refusal_record(), "file-open", 0)
        .unwrap();
    assert!(log.status().write_failed);
    assert_eq!(log.status().queued, 1);
    std::fs::remove_dir(&temporary).unwrap();
    log.tick(Duration::from_secs(10), 10_000).unwrap();
    assert!(!log.status().write_failed);
    assert_eq!(log.status().queued, 1);
    drop(log);
    let mut reopened = open(&path, AuditRetention::default());
    reopened.tick(Duration::from_secs(60), 70_000).unwrap();
    assert_eq!(lines(&path).len(), 1);
    assert_eq!(lines(&path)[0]["count"], 1);
}

#[test]
fn a_full_retry_queue_can_transfer_into_one_durable_summary_without_dropping_counts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("audit.jsonl");
    let temporary = path.with_extension("refusals.tmp");
    let mut log = open(&path, AuditRetention::default());
    std::fs::create_dir(&temporary).unwrap();
    for _ in 0..1000 {
        log.append_refusal_at(&refusal_record(), "file-open", 0)
            .unwrap();
    }
    assert_eq!(log.status().queued, 1000);
    assert_eq!(log.status().dropped, 0);
    std::fs::remove_dir(&temporary).unwrap();
    log.tick(Duration::from_secs(10), 10_000).unwrap();
    assert_eq!(log.status().queued, 1);
    assert_eq!(log.status().dropped, 0);
    log.tick(Duration::from_secs(60), 70_000).unwrap();
    assert_eq!(lines(&path).len(), 1);
    assert_eq!(lines(&path)[0]["count"], 1000);
}
