//! Events page (API-201): subscribe is idempotent per (plugin, topic, filter).

use ilium_plugin_host::testing::harness::{HarnessHandle, HostHarness};
use serde_json::json;

fn host() -> HarnessHandle {
    HostHarness::new()
        .grants(&["events.subscribe"])
        .build()
        .expect("build harness")
}

/// REQ: API-201
#[test]
fn api_201_second_identical_subscribe_returns_the_same_subscription_id() {
    let handle = host();
    let args = json!({"topic": "pane-lifecycle", "filter": {"kind": "terminal"}});
    let first = handle
        .call_import_value("events.subscribe", args.clone())
        .expect("first subscribe must succeed");
    let second = handle
        .call_import_value("events.subscribe", args)
        .expect("second identical subscribe must succeed");
    let first_id = first
        .get("subscription-id")
        .expect("subscribe must return a subscription-id");
    let second_id = second
        .get("subscription-id")
        .expect("subscribe must return a subscription-id");
    assert_eq!(
        first_id, second_id,
        "identical subscribes must share one id"
    );
}

/// REQ: API-201
#[test]
fn api_201_identical_subscribe_does_not_duplicate_delivery() {
    let handle = host();
    let args = json!({"topic": "pane-lifecycle", "filter": {"kind": "terminal"}});
    handle
        .call_import_value("events.subscribe", args.clone())
        .expect("first subscribe must succeed");
    handle
        .call_import_value("events.subscribe", args)
        .expect("second identical subscribe must succeed");
    let delivered = handle.delivered_events_for("pane-lifecycle");
    assert_eq!(
        delivered, 1,
        "one published event must be delivered exactly once to the plugin"
    );
}

/// REQ: API-201
#[test]
fn api_201_unsubscribe_of_a_stale_id_returns_ok() {
    let handle = host();
    let result = handle.call_import(
        "events.unsubscribe",
        json!({"subscription-id": "sub-does-not-exist"}),
    );
    assert!(
        result.error_code().is_none(),
        "unsubscribing a stale subscription id must return ok"
    );
}
