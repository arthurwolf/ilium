//! The `ilium.core.cost/usage` service and the `usage-changed` topic.

use serde_json::json;

use crate::common::*;

/// REQ: F-038
#[test]
fn usage_038_answers_at_once_with_done() {
    let mut host = activated_server();
    assert!(host.call_service(USAGE_SERVICE, "usage", "{}").is_ok());
}

/// REQ: F-038
#[test]
fn usage_038_result_has_a_panes_object() {
    let mut host = activated_server();
    let reply = host.call_service(USAGE_SERVICE, "usage", "{}").expect("usage service");
    let value: serde_json::Value = serde_json::from_str(&reply).expect("usage reply is JSON");
    assert!(value["panes"].is_object());
}

/// REQ: F-038
#[test]
fn usage_038_window_parameter_is_accepted() {
    let mut host = activated_server();
    let reply = host.call_service(USAGE_SERVICE, "usage", &json!({"window": "day"}).to_string()).expect("usage service");
    let value: serde_json::Value = serde_json::from_str(&reply).expect("usage reply is JSON");
    assert!(value["panes"].is_object());
}

/// REQ: F-038
#[test]
fn usage_038_unknown_method_is_not_found() {
    let mut host = activated_server();
    let reply = host.call_service(USAGE_SERVICE, "nope", "{}").expect("service reply");
    assert!(reply.contains("not-found"), "{reply}");
}

/// REQ: F-038, F-040
#[test]
fn usage_038_a_fed_pane_appears_under_panes() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "claude", "claude");
    feed_pane(&mut host, pane, KNOWN_MODEL, 10_000);
    let reply = host.call_service(USAGE_SERVICE, "usage", "{}").expect("usage service");
    let value: serde_json::Value = serde_json::from_str(&reply).expect("usage reply is JSON");
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
