//! Errors and limits page (API-200): per-plugin token bucket on effectful imports.

use std::time::Duration;

use ilium_plugin_host::testing::harness::{HarnessHandle, HostHarness};
use serde_json::json;

fn host() -> HarnessHandle {
    HostHarness::new()
        .grants(&["fs.write:data"])
        .build()
        .expect("build harness")
}

/// REQ: API-200
#[test]
fn api_200_empty_bucket_returns_busy_rate_limited_with_retry_after() {
    let handle = host();
    let mut last = None;
    for index in 0..1000 {
        let result = handle.call_import(
            "fs.write:data",
            json!({"path": "burst.txt", "content": format!("{index}")}),
        );
        if result.error_code().as_deref() == Some("busy") {
            last = Some(result);
            break;
        }
    }
    let limited = last.expect("a burst of effectful calls must exhaust the named token bucket");
    assert_eq!(limited.error_code().as_deref(), Some("busy"));
    let reason = limited
        .error_reason()
        .expect("a busy refusal must carry a reason");
    assert_eq!(reason, "rate-limited");
    let retry_after = limited
        .retry_after_ms()
        .expect("a rate-limited refusal must carry retry-after-ms");
    assert!(retry_after >= 1, "retry-after-ms must be at least 1");
}

/// REQ: API-200
#[test]
fn api_200_bucket_refills_after_retry_after_elapses() {
    let handle = host();
    let mut limited = None;
    for index in 0..1000 {
        let result = handle.call_import(
            "fs.write:data",
            json!({"path": "refill.txt", "content": format!("{index}")}),
        );
        if result.error_code().as_deref() == Some("busy") {
            limited = Some(result);
            break;
        }
    }
    let limited = limited.expect("the bucket must empty under a burst");
    let retry_after = limited
        .retry_after_ms()
        .expect("a rate-limited refusal must carry retry-after-ms");
    handle.advance_clock(Duration::from_millis(u64::from(retry_after)));
    let after = handle.call_import("fs.write:data", json!({"path": "refill.txt", "content": "ok"}));
    assert!(
        after.error_code().is_none(),
        "the call after retry-after-ms must be admitted by the refilled bucket"
    );
}

/// REQ: API-200
#[test]
fn api_200_each_plugin_owns_its_own_named_bucket() {
    let handle = host();
    for index in 0..1000 {
        let _ = handle.call_import(
            "fs.write:data",
            json!({"path": "own.txt", "content": format!("{index}")}),
        );
    }
    assert!(
        handle.shared_bucket_exists("fs.write:data"),
        "the effectful import must own a named token bucket"
    );
    let peer = handle.peer("com.example.other", &["fs.write:data"]);
    let admitted = peer.call_import("fs.write:data", json!({"path": "peer.txt", "content": "x"}));
    assert!(
        admitted.error_code().is_none(),
        "another plugin must not be refused by this plugin's exhausted bucket"
    );
}
