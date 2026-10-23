use std::path::Path;
use ilium_plugin_host::audit::{AuditActor, AuditContext, AuditLog, AuditRecord, AuditRetention};
use serde_json::json;

fn writer(path: &Path, max_bytes: u64) -> AuditLog {
    AuditLog::open_with_context(path.into(), Some(AuditContext {
        session:"readonly-audit-test".into(), actor:AuditActor {kind:"cli".into(),pane:None,plugin:None},
    }), AuditRetention {max_bytes,keep_days:365}).unwrap()
}
fn append(log: &mut AuditLog, at: u64, reason: &str) {
    log.append_at(&AuditRecord {at_unix_millis:at,plugin:String::new(),action:"safe-mode".into(),
        detail_json:json!({"result":"ok","reason":reason}).to_string()},at).unwrap();
}
#[test]
fn missing_path_returns_empty_without_creating_root_or_lock() {
    let root=tempfile::tempdir().unwrap();
    let path=root.path().join("missing/audit/audit.jsonl");
    assert!(AuditLog::read_verified(&path).unwrap().is_empty());
    assert!(!root.path().join("missing").exists());
    let path=root.path().join("audit.jsonl");
    assert!(AuditLog::read_verified(&path).unwrap().is_empty());
    assert!(!path.with_extension("lock").exists());
}
#[test]
fn genuine_redacted_records_read_without_rewriting_any_bytes() {
    let root=tempfile::tempdir().unwrap(); let path=root.path().join("audit.jsonl");
    let mut log=writer(&path,65536); log.set_secrets(&["actual-private-secret".into()]);
    append(&mut log,0,"contains actual-private-secret");
    append(&mut log,1,"second event");
    let before=std::fs::read(&path).unwrap();
    let permissions=std::fs::metadata(&path).unwrap().permissions();
    let records=AuditLog::read_verified(&path).unwrap();
    assert_eq!(records.len(),2); assert_eq!(records[1]["seq"],2);
    assert!(!serde_json::to_string(&records).unwrap().contains("actual-private-secret"));
    assert_eq!(std::fs::read(&path).unwrap(),before);
    assert_eq!(std::fs::metadata(&path).unwrap().permissions(),permissions);
}
#[test]
fn retained_rotations_are_all_checked_and_returned_in_chain_order() {
    let root=tempfile::tempdir().unwrap(); let path=root.path().join("audit.jsonl");
    let mut log=writer(&path,256);
    for sequence in 0..5 { append(&mut log,sequence,"rotation boundary event with sufficient payload"); }
    assert!(std::fs::read_dir(root.path()).unwrap().any(|entry|entry.unwrap().file_name().to_string_lossy().starts_with("audit-")));
    let records=AuditLog::read_verified(&path).unwrap();
    assert_eq!(records.len(),5);
    // Each reader enumerates through an independent held-directory stream.
    assert_eq!(AuditLog::read_verified(&path).unwrap(),records);
    for (index,record) in records.iter().enumerate() { assert_eq!(record["seq"],index+1); }
    let rotation=std::fs::read_dir(root.path()).unwrap().map(|entry|entry.unwrap().path())
        .find(|path|path.file_name().unwrap().to_string_lossy().starts_with("audit-")).unwrap();
    let original=std::fs::read_to_string(&rotation).unwrap();
    let changed=original.replace("rotation boundary","tampered boundary");
    assert_ne!(changed,original); std::fs::write(&rotation,&changed).unwrap();
    assert!(AuditLog::read_verified(&path).is_err());
    assert_eq!(std::fs::read_to_string(rotation).unwrap(),changed);
}
#[test]
fn malformed_tail_and_missing_writer_lock_are_not_repaired() {
    let root=tempfile::tempdir().unwrap(); let path=root.path().join("audit.jsonl");
    let mut log=writer(&path,65536); append(&mut log,0,"event");
    let mut bytes=std::fs::read(&path).unwrap(); bytes.pop(); std::fs::write(&path,&bytes).unwrap();
    assert!(AuditLog::read_verified(&path).is_err()); assert_eq!(std::fs::read(&path).unwrap(),bytes);
    bytes.push(b'\n'); std::fs::write(&path,&bytes).unwrap();
    std::fs::remove_file(path.with_extension("lock")).unwrap();
    assert!(AuditLog::read_verified(&path).is_err());
    assert!(!path.with_extension("lock").exists());
}
#[cfg(unix)]
#[test]
fn unsafe_permissions_are_refused_without_chmod() {
    let root=tempfile::tempdir().unwrap(); let path=root.path().join("audit.jsonl");
    let mut log=writer(&path,65536); append(&mut log,0,"event");
    assert!(std::process::Command::new("chmod").arg("644").arg(&path).status().unwrap().success());
    let before=std::fs::metadata(&path).unwrap().permissions();
    assert!(AuditLog::read_verified(&path).is_err());
    assert_eq!(std::fs::metadata(&path).unwrap().permissions(),before);
}
#[cfg(unix)]
#[test]
fn symlinked_log_or_rotation_is_refused_without_following_target() {
    let root=tempfile::tempdir().unwrap(); let path=root.path().join("audit.jsonl");
    let mut log=writer(&path,65536); append(&mut log,0,"event");
    let target=root.path().join("target"); std::fs::rename(&path,&target).unwrap();
    let bytes=std::fs::read(&target).unwrap();
    assert!(std::process::Command::new("ln").arg("-s").arg(&target).arg(&path).status().unwrap().success());
    assert!(AuditLog::read_verified(&path).is_err());
    assert_eq!(std::fs::read(&target).unwrap(),bytes);
    std::fs::remove_file(&path).unwrap(); std::fs::rename(&target,&path).unwrap();
    let rotation=root.path().join("audit-1.jsonl");
    assert!(std::process::Command::new("ln").arg("-s").arg(&path).arg(&rotation).status().unwrap().success());
    assert!(AuditLog::read_verified(&path).is_err());
    assert!(std::fs::symlink_metadata(rotation).unwrap().file_type().is_symlink());
}

#[test]
fn first_and_middle_record_tampering_is_rejected_without_repair() {
    let root=tempfile::tempdir().unwrap(); let path=root.path().join("audit.jsonl");
    let mut log=writer(&path,65536);
    for (at,reason) in [(0,"first record"),(1,"middle record"),(2,"last anchor")] { append(&mut log,at,reason); }
    let original=std::fs::read_to_string(&path).unwrap();
    for reason in ["first record","middle record"] {
        let changed=original.replace(reason,"altered record");
        std::fs::write(&path,&changed).unwrap();
        assert!(AuditLog::read_verified(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(),changed);
    }
}
