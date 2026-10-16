//! The topic `ilium.core.tree/changed` and the service `ilium.core.tree/bookmarks`.
//!
//! Observing published topics needs `FakeHost::published_topics` (see API_GAPS.md).

use ilium_plugin_sdk::types::{Event, NodeClass, NodeId};
use serde_json::Value;

use crate::common::{add_node, page_text, server_host};

const TOPIC: &str = "ilium.core.tree/changed";

fn changed_payloads(host: &ilium_plugin_sdk::testing::FakeHost) -> Vec<Value> {
    host.published_topics()
        .into_iter()
        .filter(|(topic, _)| topic == TOPIC)
        .map(|(_, payload)| serde_json::from_str(&payload).expect("topic payload is JSON"))
        .collect()
}

/// REQ: F-001
#[test]
fn feature_001_adding_a_node_publishes_changed_with_node_and_change() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "Deploy", NodeClass::Container);
    host.settle();
    let payloads = changed_payloads(&host);
    assert!(payloads.iter().any(|payload| payload["node"] == node && payload["change"].is_string()), "{payloads:?}");
}

/// REQ: F-001
#[test]
fn feature_001_removing_a_node_publishes_changed_for_that_node() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "Deploy", NodeClass::Container);
    host.settle();
    host.tree().remove(node);
    host.settle();
    let payloads = changed_payloads(&host);
    assert!(payloads.iter().filter(|payload| payload["node"] == node).count() >= 2, "{payloads:?}");
}

/// REQ: F-001
#[test]
fn feature_001_the_add_and_remove_changes_are_named_differently() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "Deploy", NodeClass::Container);
    host.settle();
    host.tree().remove(node);
    host.settle();
    let changes: Vec<String> = changed_payloads(&host)
        .into_iter()
        .filter(|payload| payload["node"] == node)
        .map(|payload| payload["change"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(changes.len() >= 2);
    assert_ne!(changes.first(), changes.last());
}

/// REQ: F-001
#[test]
fn feature_001_events_that_are_not_structural_publish_nothing() {
    let mut host = server_host();
    host.deliver(vec![Event::ThemeChanged, Event::PaneFocused(Some(NodeId(1)))]);
    host.settle();
    assert!(changed_payloads(&host).is_empty());
}

/// REQ: F-001
#[test]
fn feature_001_the_page_documents_the_topic_payload_fields() {
    assert!(page_text().contains("`ilium.core.tree/changed`: { node, change } after any structural change"));
}

/// REQ: F-006
#[test]
fn feature_006_the_page_documents_the_bookmarks_service_operations() {
    assert!(page_text().contains("`ilium.core.tree/bookmarks`: list, add, remove: read and edit the project bookmarks"));
}
