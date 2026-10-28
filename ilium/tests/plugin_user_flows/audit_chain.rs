//! Audit record hash chain and storage ceiling (USR-257, USR-258).

use crate::support::*;

fn report_text(output: &CliOutput) -> String {
    serde_json::to_string(&output.stdout_jsonl).unwrap_or_default()
}

/// REQ: USR-257
#[test]
fn usr_257_verify_names_the_record_whose_hash_no_longer_chains() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.a", "1.0.0", &[]));
    install_ok(&mut rig, &spec("com.example.b", "1.0.0", &[]));
    let audit_file = rig.audit_dir().join("audit.jsonl");
    let text = std::fs::read_to_string(&audit_file).expect("audit file");
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    assert!(lines.len() >= 2, "two audit records are needed");
    let mut record: serde_json::Value = serde_json::from_str(&lines[1]).expect("record json");
    let key = record
        .as_object()
        .expect("record object")
        .iter()
        .find(|(_, value)| value.is_string())
        .map(|(key, _)| key.clone())
        .expect("a string field");
    record[key.as_str()] = serde_json::Value::String("tampered".to_string());
    lines[1] = record.to_string();
    std::fs::write(&audit_file, lines.join("\n") + "\n").expect("write tampered audit");
    let output = rig.cli(&["audit", "verify"]);
    assert_ne!(result_exit_code(&output), 0);
    assert!(
        report_text(&output).contains('2'),
        "{}",
        report_text(&output)
    );
}

/// REQ: USR-257
#[test]
fn usr_257_verify_reports_a_break_after_truncation() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.a", "1.0.0", &[]));
    install_ok(&mut rig, &spec("com.example.b", "1.0.0", &[]));
    let audit_file = rig.audit_dir().join("audit.jsonl");
    let text = std::fs::read_to_string(&audit_file).expect("audit file");
    std::fs::write(&audit_file, &text[..text.len() - 10]).expect("truncate audit");
    let output = rig.cli(&["audit", "verify"]);
    assert_ne!(result_exit_code(&output), 0);
}

/// REQ: USR-258
#[test]
fn usr_258_the_storage_ceiling_deletes_the_oldest_rotated_files_first_and_records_it() {
    let mut rig = UserRig::new();
    let dir = rig.audit_dir();
    std::fs::create_dir_all(&dir).expect("audit folder");
    for index in 1..=3 {
        let file =
            std::fs::File::create(dir.join(format!("audit-{index}.jsonl"))).expect("rotated file");
        file.set_len(100 * 1024 * 1024).expect("set size");
    }
    rig.start_server();
    assert!(!dir.join("audit-1.jsonl").exists());
    assert!(dir.join("audit-3.jsonl").exists());
    let live = std::fs::read_to_string(dir.join("audit.jsonl")).unwrap_or_default();
    assert!(live.contains("deleted"), "{live}");
}

/// REQ: USR-258
#[test]
fn usr_258_deletion_stops_once_the_total_is_back_under_the_ceiling() {
    let mut rig = UserRig::new();
    let dir = rig.audit_dir();
    std::fs::create_dir_all(&dir).expect("audit folder");
    for index in 1..=3 {
        let file =
            std::fs::File::create(dir.join(format!("audit-{index}.jsonl"))).expect("rotated file");
        file.set_len(100 * 1024 * 1024).expect("set size");
    }
    rig.start_server();
    assert!(!dir.join("audit-1.jsonl").exists());
    assert!(dir.join("audit-2.jsonl").exists());
    assert!(dir.join("audit-3.jsonl").exists());
}
