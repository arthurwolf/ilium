//! F-035 model and context facts in the frame header: server `agent-model-changed` to a client `model` message.

use ilium_plugin_sdk::types::{AgentActivity, PluginSide};

use crate::common::*;

/// Server host with an agent pane whose transcript reports `used_tokens` of context (see API_GAPS.md).
fn server_with_used_tokens(used_tokens: u32) -> (ilium_plugin_sdk::testing::FakeHost, u64) {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    host.agents().set_context_used_tokens(pane, used_tokens);
    (host, pane)
}

fn model_payload(used_tokens: u32, window: Option<u32>, model: &str) -> (u64, String) {
    let (mut host, pane) = server_with_used_tokens(used_tokens);
    let submissions = host.deliver_one(model_changed(pane, model, window));
    let sent = messages(&submissions);
    assert_eq!(sent.len(), 1, "{sent:?}");
    (pane, String::from_utf8(sent[0].3.clone()).expect("utf8 payload"))
}

/// REQ: F-035
#[test]
fn f_035_model_change_sends_one_message_to_the_client_side_of_this_plugin() {
    let (mut host, pane) = server_with_used_tokens(82_000);
    let sent = messages(&host.deliver_one(model_changed(pane, "opus", Some(200_000))));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, PLUGIN_ID);
    assert_eq!(sent[0].1, PluginSide::Client);
    assert_eq!(sent[0].2, "model");
}

/// REQ: F-035
#[test]
fn f_035_payload_is_pane_bar_model_dot_percent_context() {
    let (pane, payload) = model_payload(82_000, Some(200_000), "opus");
    assert_eq!(payload, format!("{pane}|opus · 41% context"));
}

/// REQ: F-035
#[test]
fn f_035_percent_is_rounded_down() {
    let (pane, payload) = model_payload(1_999, Some(200_000), "opus");
    assert_eq!(payload, format!("{pane}|opus · 0% context"));
    let (pane, payload) = model_payload(2_000, Some(200_000), "opus");
    assert_eq!(payload, format!("{pane}|opus · 1% context"));
}

/// REQ: F-035
#[test]
fn f_035_percent_at_the_full_window_is_100() {
    let (pane, payload) = model_payload(200_000, Some(200_000), "opus");
    assert_eq!(payload, format!("{pane}|opus · 100% context"));
}

/// REQ: F-035
#[test]
fn f_035_percent_one_token_below_the_next_whole_percent_stays() {
    let (pane, payload) = model_payload(199_999, Some(200_000), "opus");
    assert_eq!(payload, format!("{pane}|opus · 99% context"));
}

/// REQ: F-035
#[test]
fn f_035_unknown_window_shows_the_model_alone() {
    let (pane, payload) = model_payload(50_000, None, "opus");
    assert_eq!(payload, format!("{pane}|opus"));
}

/// REQ: F-035
#[test]
fn f_035_zero_window_shows_the_model_alone() {
    let (pane, payload) = model_payload(50_000, Some(0), "opus");
    assert_eq!(payload, format!("{pane}|opus"));
}

/// REQ: F-035
#[test]
fn f_035_one_token_window_does_not_divide_by_zero() {
    let (pane, payload) = model_payload(0, Some(1), "opus");
    assert_eq!(payload, format!("{pane}|opus · 0% context"));
}

/// REQ: F-035
#[test]
fn f_035_model_name_is_passed_through_unchanged() {
    let (pane, payload) = model_payload(0, Some(1_000_000), "gpt-5.6-terra");
    assert_eq!(payload, format!("{pane}|gpt-5.6-terra · 0% context"));
}

/// REQ: F-035
#[test]
fn f_035_message_arrives_on_the_client_and_fills_the_model_slot() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    let payload = format!("{pane}|opus · 41% context");
    let submissions = host.deliver_one(message_from(PluginSide::Server, "model", payload.as_bytes()));
    assert_eq!(set_slots(&submissions), vec![(pane, "model".to_string(), "opus · 41% context".to_string())]);
}

/// REQ: F-035
#[test]
fn f_035_model_message_without_a_pipe_is_ignored() {
    let mut host = client_host();
    let submissions = host.deliver_one(message_from(PluginSide::Server, "model", b"garbage"));
    assert!(set_slots(&submissions).is_empty());
}

/// REQ: F-035
#[test]
fn f_035_model_message_with_a_non_numeric_pane_is_ignored() {
    let mut host = client_host();
    let submissions = host.deliver_one(message_from(PluginSide::Server, "model", b"abc|opus"));
    assert!(set_slots(&submissions).is_empty());
}

/// REQ: F-035
#[test]
fn f_035_model_text_may_itself_contain_a_pipe() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Working);
    let payload = format!("{pane}|a|b");
    let submissions = host.deliver_one(message_from(PluginSide::Server, "model", payload.as_bytes()));
    assert_eq!(set_slots(&submissions)[0].2, "a|b");
}
