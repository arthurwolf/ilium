//! Audit log page (SEC-730).

use sha2::{Digest, Sha256};

use crate::support::*;

/// REQ: SEC-730
#[test]
fn sec_730_every_audit_record_carries_the_sha256_of_the_previous_record() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.cli(&["grant", NOTES, "fs.read"]);
    let records = rig.audit_log_lines();
    assert!(
        records.len() >= 2,
        "the test needs at least two audit records"
    );
    for pair in records.windows(2) {
        let previous_line = pair[0]["raw"]
            .as_str()
            .expect("each record keeps its raw line");
        let claimed = pair[1]["prev_sha256"]
            .as_str()
            .expect("each record carries prev_sha256");
        let actual: String = Sha256::digest(previous_line.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(claimed, actual, "prev_sha256 must hash the previous record");
    }
}

/// REQ: SEC-730
#[test]
fn sec_730_verify_reports_the_first_broken_index_with_exit_code_3() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.cli(&["grant", NOTES, "fs.read"]);
    rig.cli(&["disable", NOTES]);
    rig.corrupt_audit_record(1);
    let output = rig.cli(&["plugin", "audit", "--verify"]);
    assert_eq!(output.exit_code, 3, "a broken chain must exit with code 3");
    assert!(
        output
            .stdout_jsonl
            .iter()
            .any(|line| line["first_broken_index"] == 1),
        "the first broken index must be reported, got {:?}",
        output.stdout_jsonl
    );
}

/// REQ: SEC-730
#[test]
fn sec_730_rotation_writes_the_last_hash_as_the_first_record_of_the_next_file() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let last_hash_before = rig.audit_last_hash();
    rig.rotate_audit_log();
    let next_file = rig.audit_log_lines();
    let first = next_file
        .first()
        .expect("the new file must start with a chain record");
    assert_eq!(
        first["prev_sha256"].as_str(),
        Some(last_hash_before.as_str()),
        "the rotated file must open with the last hash of the previous file"
    );
}
