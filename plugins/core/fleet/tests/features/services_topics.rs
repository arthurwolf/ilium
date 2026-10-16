//! Services and topic of `ilium.core.fleet` (COR-011, COR-050): `broadcast` (send), `panes`
//! (list) and the `broadcast-sent` topic.

use ilium_plugin_sdk::types::AgentActivity;
use serde_json::Value;

use crate::common::*;

/// REQ: COR-011
#[test]
fn svc_011_the_broadcast_service_sends_text_to_the_agents() {
    let mut host = server_host();
    add_agent(&mut host, "a", "claude", AgentActivity::Idle);
    add_session(&mut host, "default", true, &[]);
    host.call_service("broadcast", "send", r#"{"message":"hello"}"#);
    host.settle();
    assert!(sent_inputs(&host).iter().all(|s| s.text == "hello"));
    assert!(!sent_inputs(&host).is_empty());
}

/// REQ: COR-011
#[test]
fn svc_011_the_panes_service_lists_the_panes() {
    let mut host = server_host();
    add_agent(&mut host, "a", "claude", AgentActivity::Idle);
    add_session(&mut host, "default", true, &[]);
    let reply = host.call_service("panes", "list", "{}").expect("reply");
    let value: Value = serde_json::from_str(&reply).expect("JSON");
    assert!(value.is_array() || value["panes"].is_array(), "{value}");
}

/// REQ: COR-011
#[test]
fn svc_011_the_broadcast_service_refuses_an_empty_message() {
    let mut host = server_host();
    add_agent(&mut host, "a", "claude", AgentActivity::Idle);
    add_session(&mut host, "default", true, &[]);
    host.call_service("broadcast", "send", r#"{"message":""}"#);
    host.settle();
    assert!(sent_inputs(&host).is_empty());
}

/// REQ: COR-050
#[test]
fn svc_050_the_page_declares_services_and_the_topic() {
    let text = page_text();
    assert!(text.contains("`ilium.core.fleet/broadcast`"));
    assert!(text.contains("`ilium.core.fleet/panes`"));
    assert!(text.contains("`ilium.core.fleet/broadcast-sent`"));
}
