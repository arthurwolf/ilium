//! Services page (API-209): call chains deeper than 4 hops or cyclic chains are refused before any callee runs.

use ilium_plugin_host::testing::harness::{HarnessHandle, HostHarness};
use serde_json::json;

fn host() -> HarnessHandle {
    HostHarness::new()
        .two_plugins()
        .build()
        .expect("build harness")
}

/// REQ: API-209
#[test]
fn api_209_chain_deeper_than_four_hops_is_refused_reentrant() {
    let handle = host();
    let result = handle.call_import(
        "services.call",
        json!({
            "service": "svc.notes",
            "method": "ping",
            "chain": ["a", "b", "c", "d", "e"]
        }),
    );
    assert_eq!(
        result.error_code().as_deref(),
        Some("reentrant"),
        "a fifth hop must be refused with reentrant"
    );
}

/// REQ: API-209
#[test]
fn api_209_cycle_back_to_a_plugin_on_the_chain_is_refused_before_callee_runs() {
    let handle = host();
    let result = handle.call_import(
        "services.call",
        json!({
            "service": "svc.notes",
            "method": "ping",
            "chain": ["com.example.notes", "com.example.other"]
        }),
    );
    assert_eq!(
        result.error_code().as_deref(),
        Some("reentrant"),
        "a plugin already on the chain must be refused with reentrant"
    );
    assert_eq!(
        handle.callee_invocations("svc.notes"),
        0,
        "the refused cycle must not run any callee"
    );
}
