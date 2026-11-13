//! The topic `ilium.core.notifications/raised`: `{ event, pane, urgency }`.

use ilium_plugin_sdk::types::AgentActivity;
use serde_json::Value;

use crate::common::*;

const RAISED: &str = "ilium.core.notifications/raised";

fn raised_payloads(host: &ilium_plugin_sdk::testing::FakeHost) -> Vec<Value> {
    host.published_topics()
        .into_iter()
        .filter(|(topic, _)| topic == RAISED)
        .map(|(_, payload)| serde_json::from_str(&payload).expect("payload is JSON"))
        .collect()
}

/// REQ: F-048
#[test]
fn topic_048_a_raised_notice_is_published_with_event_pane_and_urgency() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    finish_turn(&mut host, node, 1);
    let payloads = raised_payloads(&host);
    assert_eq!(payloads.len(), 1);
    let payload = payloads[0].as_object().expect("object");
    let mut keys: Vec<&str> = payload.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["event", "pane", "urgency"]);
    assert_eq!(payload["pane"], node.0);
    assert!(
        ["low", "normal", "high"].contains(&payload["urgency"].as_str().expect("urgency string"))
    );
}

/// REQ: F-048
#[test]
fn topic_048_nothing_is_published_when_no_notice_is_raised() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    transition(
        &mut host,
        node,
        AgentActivity::Idle,
        AgentActivity::Working,
        1,
    );
    assert!(raised_payloads(&host).is_empty());
}

/// REQ: F-048
#[test]
fn topic_048_a_notice_silenced_by_the_master_switch_is_not_published() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.enabled", serde_json::json!(false));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    finish_turn(&mut host, node, 1);
    assert!(raised_payloads(&host).is_empty());
}
