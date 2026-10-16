//! Twins of ilium-client/src/recap/pointers.rs: only real, allowed, evidenced files survive.

use serde_json::json;

use crate::support::*;

/// REQ: PORT:ilium-client/src/recap/pointers.rs::accepts_existing_file_named_in_evidence
#[test]
fn port_pointers_accept_an_existing_file_named_in_evidence() {
    let short = short_after_answer(&["edited src/lib.rs"], &[("src/lib.rs", "fn main() {}")], pointer_answer("src/lib.rs", "edited"));
    assert_eq!(short["pointers"].as_array().map(Vec::len), Some(1));
    assert_eq!(short["pointers"][0]["path"], "src/lib.rs");
    assert_eq!(short["rejected_pointers"], 0);
}

/// REQ: PORT:ilium-client/src/recap/pointers.rs::rejects_pointer_not_mentioned_in_evidence
#[test]
fn port_pointers_reject_a_pointer_not_mentioned_in_evidence() {
    let short = short_after_answer(&["$ cargo test"], &[("src/other.rs", "x")], pointer_answer("src/other.rs", "invented"));
    assert_eq!(short["pointers"], json!([]));
    assert_eq!(short["rejected_pointers"], 1);
}

/// REQ: PORT:ilium-client/src/recap/pointers.rs::rejects_missing_file_and_outside_root
#[test]
fn port_pointers_reject_a_missing_file_and_a_path_outside_the_root() {
    let missing = short_after_answer(&["edited src/ghost.rs"], &[], pointer_answer("src/ghost.rs", "gone"));
    assert_eq!(missing["pointers"], json!([]));
    assert_eq!(missing["rejected_pointers"], 1);
    let outside = short_after_answer(&["see /etc/shadow and ../escape.rs"], &[], pointer_answer("/etc/shadow", "outside"));
    assert_eq!(outside["pointers"], json!([]));
    assert_eq!(outside["rejected_pointers"], 1);
    let parent = short_after_answer(&["see ../escape.rs"], &[], pointer_answer("../escape.rs", "outside"));
    assert_eq!(parent["rejected_pointers"], 1);
}

/// REQ: PORT:ilium-client/src/recap/pointers.rs::rejects_denied_paths_even_when_mentioned_and_present
#[test]
fn port_pointers_reject_denied_paths_even_when_mentioned_and_present() {
    let short = short_after_answer(&["opened .env"], &[(".env", "SECRET=1")], pointer_answer(".env", "config"));
    assert_eq!(short["pointers"], json!([]));
    assert_eq!(short["rejected_pointers"], 1);
}

/// REQ: PORT:ilium-client/src/recap/pointers.rs::redacts_secrets_in_pointer_notes
#[test]
fn port_pointers_redact_secrets_in_pointer_notes() {
    let short = short_after_answer(
        &["edited src/lib.rs"],
        &[("src/lib.rs", "x")],
        pointer_answer("src/lib.rs", "uses api_key=abcd1234efgh5678 here"),
    );
    assert!(!short.to_string().contains("abcd1234efgh5678"), "{short}");
    assert!(short["pointers"][0]["note"].as_str().unwrap().contains("[redacted]"));
}
