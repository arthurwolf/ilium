//! The `ilium.core.cost/usage` service and the `usage-changed` topic.

use serde_json::json;

use crate::common::*;

/// REQ: F-038
#[test]
fn usage_038_answers_at_once_with_done() {
    let reply = call_usage("usage", "{}");
    assert!(matches!(
        reply,
        ilium_plugin_sdk::exports::ServiceReply::Done(_)
    ));
}

/// REQ: F-038
#[test]
fn usage_038_result_has_a_panes_object() {
    let value = done_json(call_usage("usage", "{}"));
    assert!(value["panes"].is_object());
}

/// REQ: F-038
#[test]
fn usage_038_window_parameter_is_accepted() {
    let value = done_json(call_usage("usage", &json!({"window": "day"}).to_string()));
    assert!(value["panes"].is_object());
}

/// REQ: F-038
#[test]
fn usage_038_unknown_method_is_not_found() {
    let mut server = ilium_plugin_core_cost::CostServer;
    use ilium_plugin_sdk::ServiceProvider;
    let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        server.handle(ilium_plugin_sdk::exports::ServiceCall {
            call: 2,
            service: USAGE_SERVICE.to_string(),
            method: "nope".to_string(),
            payload_json: "{}".to_string(),
            caller: ilium_plugin_sdk::types::PluginId("ilium.core.fleet".to_string()),
        })
    }));
    match reply {
        Ok(ilium_plugin_sdk::exports::ServiceReply::Done(text)) => {
            assert!(text.contains("not-found"), "{text}");
        }
        other => panic!("expected a not-found reply, got {other:?}"),
    }
}

/// REQ: F-038, F-040
#[test]
fn usage_038_a_fed_pane_appears_under_panes() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "claude", "claude");
    feed_pane(&mut host, pane, KNOWN_MODEL, 10_000);
    let value = done_json(call_usage("usage", "{}"));
    assert!(value["panes"].get(pane.to_string()).is_some(), "{value}");
}

/// REQ: F-038
#[test]
fn usage_038_total_change_publishes_usage_changed_with_pane_and_total() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "claude", "claude");
    feed_pane(&mut host, pane, KNOWN_MODEL, 10_000);
    let topics = published_topics(&host);
    let (_, payload) = topics
        .iter()
        .find(|(topic, _)| topic == "ilium.core.cost/usage-changed")
        .expect("published");
    let value: serde_json::Value = serde_json::from_str(payload).expect("JSON");
    assert_eq!(value["pane"], json!(pane));
    assert!(value.get("total").is_some());
}

/// REQ: F-038
#[test]
fn usage_038_no_topic_before_any_usage() {
    let host = activated_server();
    assert!(published_topics(&host).is_empty());
}
