//! Services and topic of `ilium.core.fleet` (COR-011, COR-050): `broadcast` (send), `panes`
//! (list) and the `broadcast-sent` topic.

use ilium_plugin_sdk::types::AgentActivity;
use serde_json::Value;

use crate::common::*;
use crate::feature_only::*;

/// REQ: COR-011
#[test]
fn svc_011_the_broadcast_service_sends_text_to_the_agents() {
    let mut host = server_host();
    let id = add_agent(&mut host, "a", "claude", AgentActivity::Idle);
    add_session(&mut host, "default", true, &[]);
    let reply = host.call_service("broadcast", "send", r#"{"message":"hello"}"#);
    host.settle();
    assert!(reply.is_ok(), "{reply:?}");
    let inputs = sent_inputs(&host);
    assert_eq!(inputs.len(), 1, "{inputs:?}");
    assert_eq!(inputs[0].pane.0, id);
    assert_eq!(inputs[0].text, "hello");
    assert_eq!(
        inputs[0].delivery,
        ilium_plugin_sdk::types::Delivery::Submitted
    );
}

/// REQ: COR-011
#[test]
fn svc_011_the_panes_service_lists_the_panes() {
    let mut host = server_host();
    add_agent(&mut host, "a", "claude", AgentActivity::Idle);
    add_session(&mut host, "default", true, &[]);
    let reply = host.call_service("panes", "list", "{}").expect("reply");
    let value: Value = serde_json::from_str(&reply).expect("JSON");
    // manual does not pin the reply shape (bare array or a "panes" member): accept either
    // spelling but require the list to hold the one agent pane.
    let list = if value.is_array() {
        &value
    } else {
        &value["panes"]
    };
    let items = list
        .as_array()
        .unwrap_or_else(|| panic!("no pane list: {value}"));
    assert_eq!(items.len(), 1, "{value}");
    assert!(items[0].to_string().contains("\"a\""), "{value}");
}

/// REQ: COR-011
#[test]
fn svc_011_the_broadcast_service_refuses_an_empty_message() {
    let mut host = server_host();
    add_agent(&mut host, "a", "claude", AgentActivity::Idle);
    add_session(&mut host, "default", true, &[]);
    let reply = host.call_service("broadcast", "send", r#"{"message":""}"#);
    host.settle();
    assert!(reply.is_err(), "empty message must be refused: {reply:?}");
    // manual does not pin the error code of an empty message
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
