//! Filesystem page (API-203): writes are all-or-nothing up to 4 MiB and report the new hash.

use ilium_plugin_host::testing::harness::{HarnessHandle, HostHarness};
use serde_json::json;

fn host() -> HarnessHandle {
    HostHarness::new()
        .grants(&["fs.write", "fs.read"])
        .build()
        .expect("build harness")
}

/// REQ: API-203
#[test]
fn api_203_four_mib_write_applies_completely() {
    let handle = host();
    let payload = "a".repeat(4 * 1024 * 1024);
    handle
        .call_import_value(
            "fs.write",
            json!({"path": "big.txt", "content": payload.clone()}),
        )
        .expect("a 4 MiB write must be accepted");
    let stored = handle
        .call_import_value("fs.read", json!({"path": "big.txt"}))
        .expect("the written file must be readable");
    assert_eq!(
        stored
            .get("content")
            .and_then(|value| value.as_str())
            .map(str::len),
        Some(payload.len()),
        "a completed write must hold the full payload"
    );
}

/// REQ: API-203
#[test]
fn api_203_interrupted_write_leaves_previous_content_intact() {
    let handle = host();
    handle
        .call_import_value("fs.write", json!({"path": "keep.txt", "content": "old"}))
        .expect("the first write must succeed");
    handle.kernel().interrupt_next_write();
    let _ = handle.call_import(
        "fs.write",
        json!({"path": "keep.txt", "content": "new-content"}),
    );
    let stored = handle
        .call_import_value("fs.read", json!({"path": "keep.txt"}))
        .expect("the file must still be readable");
    assert_eq!(
        stored.get("content").and_then(|value| value.as_str()),
        Some("old"),
        "an interrupted write must not replace the previous content"
    );
}

/// REQ: API-203
#[test]
fn api_203_successful_write_result_carries_the_new_content_hash() {
    let handle = host();
    let result = handle
        .call_import_value("fs.write", json!({"path": "hash.txt", "content": "hello"}))
        .expect("the write must succeed");
    let hash = result
        .get("content-hash")
        .and_then(|value| value.as_str())
        .expect("the write result must carry content-hash");
    assert!(
        hash.ends_with("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"),
        "the hash must be the SHA-256 of the new content, got {hash}"
    );
}
