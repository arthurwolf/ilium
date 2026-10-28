//! Release key page (SEC-728). The check reads the key ceremony record and the published
//! transcript; there is no code path to exercise, so the test inspects the repository files.

use sha2::{Digest, Sha256};

use crate::support::*;

/// Repository-relative folder that holds the key ceremony record and its transcript.
const CEREMONY_DIR: &str = "release/key-ceremony";

/// REQ: SEC-728
#[test]
fn sec_728_release_key_ceremony_record_shows_hardware_key_two_of_three_and_matching_transcript_hash(
) {
    let page = manual_page("reference-release-key-compromise");
    assert!(
        page.contains("hardware") && page.contains("2 of 3"),
        "the release key page must state the hardware key and the 2-of-3 signing rule"
    );
    let ceremony = repo_root().join(CEREMONY_DIR);
    let record_path = ceremony.join("ceremony-record.json");
    let record_text = std::fs::read_to_string(&record_path).unwrap_or_else(|error| {
        panic!(
            "the key ceremony record must exist at {}: {error}",
            record_path.display()
        )
    });
    let record: serde_json::Value =
        serde_json::from_str(&record_text).expect("the ceremony record must be valid JSON");
    assert_eq!(
        record["key_generation"]["location"], "hardware",
        "the release key must be generated in hardware (HSM or token)"
    );
    assert_eq!(
        record["signing_rule"], "2-of-3",
        "at least 2 of 3 custodians must be required to sign"
    );
    let transcript_path = ceremony.join("transcript.txt");
    let transcript = std::fs::read(&transcript_path).unwrap_or_else(|error| {
        panic!(
            "the published transcript must exist at {}: {error}",
            transcript_path.display()
        )
    });
    let digest: String = Sha256::digest(&transcript)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        record["transcript_sha256"], digest,
        "the transcript hash in the record must match the transcript file"
    );
}

/// REQ: SEC-728
#[test]
fn sec_728_ceremony_record_lists_at_least_three_custodians() {
    assert_manual_says("reference-release-key-compromise", "hardware");
    let record_path = repo_root().join(CEREMONY_DIR).join("ceremony-record.json");
    let record_text = std::fs::read_to_string(&record_path).unwrap_or_else(|error| {
        panic!(
            "the key ceremony record must exist at {}: {error}",
            record_path.display()
        )
    });
    let record: serde_json::Value =
        serde_json::from_str(&record_text).expect("the ceremony record must be valid JSON");
    let custodians = record["custodians"]
        .as_array()
        .map(|list| list.len())
        .unwrap_or(0);
    assert!(
        custodians >= 3,
        "a 2-of-3 rule needs at least three named custodians, got {custodians}"
    );
}
