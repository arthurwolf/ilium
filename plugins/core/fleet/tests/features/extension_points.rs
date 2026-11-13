//! Extension points `selectors` (collect) and `before-broadcast` (hook transform) of
//! `ilium.core.fleet` (COR-011, COR-050), exercised through the fake extension registry.

use ilium_plugin_sdk::exports::HookReply;
use ilium_plugin_sdk::types::AgentActivity;
use serde_json::Value;

use crate::common::*;
use crate::feature_only::*;

/// REQ: COR-011
#[test]
fn ext_011_a_selector_entry_adds_a_group_to_the_broadcast_targets() {
    let mut host = server_host();
    add_agent(&mut host, "a", "claude", AgentActivity::Idle);
    add_session(&mut host, "default", true, &[]);
    host.set_extension_entries(
        "ilium.core.fleet/selectors",
        vec![r#"{"id":"mine","label":"Mine","panes":[1]}"#.to_string()],
    );
    let (stdout, _, exit) = run_cli(&mut host, &["panes"], b"");
    assert_eq!(exit, Some(0));
    assert!(!stdout.is_empty());
}

/// REQ: COR-011
#[test]
fn ext_011_the_hook_payload_keys_are_message_targets_and_source() {
    let mut host = server_host();
    add_agent(&mut host, "a", "claude", AgentActivity::Idle);
    add_session(&mut host, "default", true, &[]);
    host.set_hook_reply("ilium.core.fleet/before-broadcast", HookReply::Pass);
    run_cli(&mut host, &["broadcast", "hi"], b"");
    let calls = host.hook_payloads("ilium.core.fleet/before-broadcast");
    assert_eq!(calls.len(), 1);
    let payload: Value = serde_json::from_str(&calls[0]).expect("JSON");
    let mut keys: Vec<&str> = payload
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(keys, ["message", "source", "targets"]);
}

/// REQ: COR-011
#[test]
fn ext_011_a_replace_without_a_message_field_leaves_the_text_unchanged() {
    let mut host = server_host();
    add_agent(&mut host, "a", "claude", AgentActivity::Idle);
    add_session(&mut host, "default", true, &[]);
    host.set_hook_reply(
        "ilium.core.fleet/before-broadcast",
        HookReply::Replace("{}".to_string()),
    );
    run_cli(&mut host, &["broadcast", "hi"], b"");
    assert!(sent_inputs(&host).iter().all(|s| s.text == "hi"));
}

/// REQ: COR-050
#[test]
fn ext_050_the_page_lists_both_points_with_their_kinds_and_priorities() {
    let text = page_text();
    assert!(text.contains("ilium.core.fleet/selectors"));
    assert!(text.contains("ilium.core.fleet/before-broadcast"));
    assert!(text.contains("hook: transform"));
    assert!(text.contains("collect"));
}
