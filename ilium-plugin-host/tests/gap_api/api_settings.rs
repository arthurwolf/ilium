//! Settings page (API-213): validate before store, JSON pointer on failure, event only after persist.

use ilium_plugin_host::testing::harness::{HarnessHandle, HostHarness};
use serde_json::json;

fn host() -> HarnessHandle {
    HostHarness::new()
        .grants(&["settings.write-all"])
        .build()
        .expect("build harness")
}

/// REQ: API-213
#[test]
fn api_213_invalid_write_returns_invalid_argument_with_first_failing_pointer() {
    let handle = host();
    let result = handle.call_import(
        "settings.write-all",
        json!({"values": {"width": "not-a-number", "height": "also-bad"}}),
    );
    assert_eq!(
        result.error_code().as_deref(),
        Some("invalid-argument"),
        "a value that fails the declared schema must return invalid-argument"
    );
    assert_eq!(
        result.error_pointer().as_deref(),
        Some("/width"),
        "the error must name the JSON pointer of the first failing field"
    );
}

/// REQ: API-213
#[test]
fn api_213_invalid_write_stores_nothing_and_emits_no_settings_changed() {
    let handle = host();
    let _ = handle.call_import(
        "settings.write-all",
        json!({"values": {"width": "not-a-number"}}),
    );
    assert_eq!(
        handle.stored_setting("width"),
        None,
        "a rejected write must not be stored"
    );
    assert_eq!(
        handle.emitted_events("settings-changed").len(),
        0,
        "a rejected write must not emit settings-changed"
    );
}

/// REQ: API-213
#[test]
fn api_213_settings_changed_is_emitted_only_after_the_value_is_persisted() {
    let handle = host();
    let result = handle.call_import("settings.write-all", json!({"values": {"width": 80}}));
    assert!(result.error_code().is_none(), "a valid write must succeed");
    assert_eq!(
        handle.emitted_events("settings-changed").len(),
        1,
        "one settings-changed event must be emitted for the write"
    );
    assert!(
        handle.event_was_after_persist("settings-changed"),
        "settings-changed must be emitted only after the value is persisted"
    );
}
