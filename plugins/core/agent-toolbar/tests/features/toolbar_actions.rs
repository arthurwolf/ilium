//! F-035 toolbar actions: the client forwards a command with the pane id, the server runs the agent action once per pane and press.

use ilium_plugin_sdk::types::{AgentActivity, CommandOrigin, Outcome, PluginSide};

use crate::common::*;

fn forward(
    command: &str,
    origin: CommandOrigin,
    pane: Option<u64>,
) -> Vec<(String, PluginSide, String, Vec<u8>)> {
    let mut host = client_host();
    messages(&host.deliver_one(command_on(command, origin, pane)))
}

/// REQ: F-035
#[test]
fn f_035_toolbar_click_on_compact_is_forwarded_to_the_server_side() {
    let sent = forward("compact", CommandOrigin::Toolbar, Some(7));
    assert_eq!(
        sent,
        vec![(
            PLUGIN_ID.to_string(),
            PluginSide::Server,
            "compact".to_string(),
            b"7".to_vec()
        )]
    );
}

/// REQ: F-035
#[test]
fn f_035_the_channel_is_the_command_id() {
    let sent = forward("history", CommandOrigin::RowAction, Some(9));
    assert_eq!(sent[0].2, "history");
    assert_eq!(sent[0].3, b"9".to_vec());
}

/// REQ: F-035
#[test]
fn f_035_the_forward_targets_this_plugin_with_no_client_filter() {
    let mut host = client_host();
    let submissions = host.deliver_one(command_on("compact", CommandOrigin::Toolbar, Some(7)));
    match &submissions[0].intent {
        ilium_plugin_sdk::types::Intent::Message(spec) => {
            assert_eq!(spec.to.plugin.0, PLUGIN_ID);
            assert_eq!(spec.to.side, PluginSide::Server);
            assert_eq!(spec.to.client, None);
        }
        other => panic!("expected a message intent, got {other:?}"),
    }
}

/// REQ: F-035
#[test]
fn f_035_a_command_without_a_pane_context_forwards_nothing() {
    assert!(forward("compact", CommandOrigin::Toolbar, None).is_empty());
}

/// REQ: F-035
#[test]
fn f_035_the_palette_origin_is_forwarded_like_a_toolbar_click() {
    let sent = forward("compact", CommandOrigin::Palette, Some(3));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].3, b"3".to_vec());
}

/// REQ: F-035
#[test]
fn f_035_keybinding_and_menu_origins_are_forwarded_too() {
    assert_eq!(
        forward("compact", CommandOrigin::Keybinding, Some(3)).len(),
        1
    );
    assert_eq!(forward("compact", CommandOrigin::Menu, Some(3)).len(), 1);
}

/// REQ: F-035
#[test]
fn f_035_two_clicks_forward_two_messages_in_order() {
    let mut host = client_host();
    let submissions = host.deliver(vec![
        command_on("compact", CommandOrigin::Toolbar, Some(1)),
        command_on("compact", CommandOrigin::Toolbar, Some(2)),
    ]);
    let payloads: Vec<Vec<u8>> = messages(&submissions)
        .into_iter()
        .map(|message| message.3)
        .collect();
    assert_eq!(payloads, vec![b"1".to_vec(), b"2".to_vec()]);
}

/// REQ: F-035
#[test]
fn f_035_server_runs_compact_as_an_agent_action_on_the_pane() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    let submissions = host.deliver_one(message_from(
        PluginSide::Client,
        "compact",
        pane.to_string().as_bytes(),
    ));
    assert_eq!(
        agent_actions(&submissions),
        vec![(pane, "compact".to_string())]
    );
}

/// REQ: F-035
#[test]
fn f_035_the_action_carries_the_dedup_key_compact_colon_pane() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    let submissions = host.deliver_one(message_from(
        PluginSide::Client,
        "compact",
        pane.to_string().as_bytes(),
    ));
    assert_eq!(submissions[0].dedup_key, Some(format!("compact:{pane}")));
}

/// REQ: F-035
#[test]
fn f_035_an_unknown_channel_runs_no_action() {
    let mut host = server_host();
    let submissions = host.deliver_one(message_from(PluginSide::Client, "format-disk", b"1"));
    assert!(agent_actions(&submissions).is_empty());
}

/// REQ: F-035
#[test]
fn f_035_a_payload_that_is_not_a_pane_id_runs_no_action() {
    let mut host = server_host();
    for payload in [&b""[..], b"abc", b"-1", b"1.5", b"7 "] {
        let submissions = host.deliver_one(message_from(PluginSide::Client, "compact", payload));
        assert!(
            agent_actions(&submissions).is_empty(),
            "payload {payload:?}"
        );
    }
}

/// REQ: F-035
#[test]
fn f_035_a_second_press_inside_the_dedup_window_is_a_duplicate() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    let press = message_from(PluginSide::Client, "compact", pane.to_string().as_bytes());
    host.deliver_one(press.clone());
    host.deliver_one(press);
    host.settle();
    let outcomes = host.outcomes();
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes[0].is_ok(), "{outcomes:?}");
    assert!(matches!(outcomes[1], Outcome::Duplicate(_)), "{outcomes:?}");
}

/// REQ: F-035
#[test]
fn f_035_presses_on_different_panes_are_not_duplicates() {
    let mut host = server_host();
    let first = add_agent_pane(&mut host, "one", "claude", AgentActivity::Idle);
    let second = add_agent_pane(&mut host, "two", "codex", AgentActivity::Idle);
    host.deliver_one(message_from(
        PluginSide::Client,
        "compact",
        first.to_string().as_bytes(),
    ));
    host.deliver_one(message_from(
        PluginSide::Client,
        "compact",
        second.to_string().as_bytes(),
    ));
    host.settle();
    assert!(
        host.outcomes().iter().all(|outcome| outcome.is_ok()),
        "{:?}",
        host.outcomes()
    );
}

/// REQ: F-035
#[test]
fn f_035_a_press_one_second_before_the_ten_minute_window_ends_is_a_duplicate() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    let press = message_from(PluginSide::Client, "compact", pane.to_string().as_bytes());
    host.deliver_one(press.clone());
    host.advance_clock(599_000);
    host.deliver_one(press);
    host.settle();
    assert!(
        matches!(host.outcomes()[1], Outcome::Duplicate(_)),
        "{:?}",
        host.outcomes()
    );
}

/// REQ: F-035
#[test]
fn f_035_a_press_one_second_after_the_ten_minute_window_runs_again() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    let press = message_from(PluginSide::Client, "compact", pane.to_string().as_bytes());
    host.deliver_one(press.clone());
    host.advance_clock(601_000);
    host.deliver_one(press);
    host.settle();
    assert!(
        host.outcomes().iter().all(|outcome| outcome.is_ok()),
        "{:?}",
        host.outcomes()
    );
}

/// REQ: F-035
#[test]
fn f_035_the_dedup_key_charset_and_length_are_valid() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    let submissions = host.deliver_one(message_from(
        PluginSide::Client,
        "compact",
        pane.to_string().as_bytes(),
    ));
    let key = submissions[0].dedup_key.clone().expect("keyed submission");
    assert!((1..=128).contains(&key.len()));
    assert!(
        key.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c)),
        "{key}"
    );
}

/// REQ: F-035
#[test]
fn f_035_action_submission_is_accepted_by_the_host() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    host.deliver_one(message_from(
        PluginSide::Client,
        "compact",
        pane.to_string().as_bytes(),
    ));
    host.settle();
    assert_eq!(host.outcomes().len(), 1);
    assert!(host.outcomes()[0].is_ok());
}

/// REQ: F-035
#[test]
fn f_035_compact_and_history_are_the_only_commands_of_the_manifest() {
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("manifest"))
            .expect("json");
    let ids: Vec<&str> = manifest["contributes"]["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["compact", "history"]);
}
