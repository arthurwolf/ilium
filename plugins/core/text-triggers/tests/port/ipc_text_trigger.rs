//! Twins of ilium-ipc text_trigger.rs: the rule model as seen through the settings rows (delays, ids, targets).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, Event, Outcome};
use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-ipc/src/text_trigger.rs::explicit_delay_round_trips_through_toml_including_zero
#[test]
fn port_001_explicit_delay_round_trips_through_toml_including_zero() {
    let mut host = server_host();
    for delay in [0u64, 1, 90, MAX_DELAY_SECONDS] {
        set_rules(&mut host, &["a"], &["r"]);
        host.settings()
            .set("delays", json!([delay]))
            .expect("delay accepted");
        host.settle();
        assert_eq!(host.settings().get("delays"), Some(json!([delay])));
        assert_eq!(
            host.kernel_settings().tab("text_triggers").expect("table")["delays"],
            json!([delay])
        );
    }
}

/// REQ: PORT:ilium-ipc/src/text_trigger.rs::identity_validation_preserves_authored_non_uuid_ids
#[test]
fn port_002_identity_validation_preserves_authored_non_uuid_ids() {
    let mut host = server_host();
    set_rules(&mut host, &["a"], &["r"]);
    host.settings()
        .set("ids", json!(["authored-rule"]))
        .expect("authored id accepted");
    host.settle();
    assert_eq!(host.settings().get("ids"), Some(json!(["authored-rule"])));
}

/// REQ: PORT:ilium-ipc/src/text_trigger.rs::identity_validation_rejects_missing_blank_control_and_duplicate_ids
#[test]
fn port_003_identity_validation_rejects_missing_blank_control_and_duplicate_ids() {
    let mut host = server_host();
    set_rules(&mut host, &["a", "b"], &["r", "s"]);
    for bad in [
        json!(["", "x"]),
        json!(["   ", "x"]),
        json!(["rule\nname", "x"]),
        json!(["\t", "x"]),
        json!(["x", "x"]),
    ] {
        assert_eq!(
            settings_error(&mut host, "ids", bad.clone()),
            ErrorCode::InvalidArgument,
            "{bad}"
        );
    }
    assert_eq!(host.settings().get("ids"), Some(json!([])));
}

/// REQ: PORT:ilium-ipc/src/text_trigger.rs::rules_stored_before_the_delay_existed_load_with_the_default
#[test]
fn port_004_rules_stored_before_the_delay_existed_load_with_the_default() {
    let mut host = host_with_table(json!({"patterns": ["a"], "replies": ["r"]}));
    host.settle();
    assert_eq!(
        host.settings().get("delays"),
        Some(json!([DEFAULT_DELAY_SECONDS]))
    );
}

/// REQ: PORT:ilium-ipc/src/text_trigger.rs::settings_round_trip_preserves_rule_order_and_target
#[test]
fn port_005_settings_round_trip_preserves_rule_order_and_target() {
    let mut host = server_host();
    let agent = add_agent_pane(&mut host, "claude");
    let shell = add_plain_pane(&mut host);
    set_rules(&mut host, &["a", "b", "c"], &["1", "2", "3"]);
    host.settings()
        .set("targets", json!(["agents", "terminals", "both"]))
        .expect("targets accepted");
    host.settle();
    assert_eq!(
        host.settings().get("patterns"),
        Some(json!(["a", "b", "c"]))
    );
    assert_eq!(
        host.settings().get("targets"),
        Some(json!(["agents", "terminals", "both"]))
    );
    assert_eq!(filter_ids(&host, agent), [rule_id(0), rule_id(2)]);
    assert_eq!(filter_ids(&host, shell), [rule_id(1), rule_id(2)]);
}

/// REQ: PORT:ilium-ipc/src/text_trigger.rs::validation_rejects_delays_beyond_the_maximum
#[test]
fn port_006_validation_rejects_delays_beyond_the_maximum() {
    let mut host = server_host();
    set_rules(&mut host, &["a"], &["r"]);
    assert_eq!(
        settings_error(&mut host, "delays", json!([MAX_DELAY_SECONDS + 1])),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        host.settings().get("delays"),
        Some(json!([DEFAULT_DELAY_SECONDS]))
    );
}
