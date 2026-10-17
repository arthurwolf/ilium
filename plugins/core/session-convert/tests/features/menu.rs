//! F-077: the client side menu provider. The pane context menu offers one entry for the other agent;
//! an entry that cannot run stays visible with a short reason (page sample "Menu entries with disabled reasons").

use ilium_plugin_sdk::exports::{MenuContext, MenuEntry};
use ilium_plugin_sdk::primitives::SessionRef;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Intent, NodeId, PluginSide, ViewTarget};
use proptest::prelude::*;

use crate::common::*;

fn context(pane: Option<u64>) -> MenuContext {
    MenuContext {
        menu: "pane".to_string(),
        node: pane.map(NodeId),
        pane: pane.map(NodeId),
        view: None,
        key: None,
        buffer: None,
        line: None,
        selection: None,
    }
}

fn verified_session(scope: &str) -> SessionRef {
    SessionRef {
        scope: scope.to_string(),
        path: format!("projects/p/{SOURCE_SESSION}.jsonl"),
        session_id: SOURCE_SESSION.to_string(),
    }
}

/// A pane of the given signature with a verified session.
fn pane_with_session(host: &mut FakeHost, signature: &str) -> u64 {
    let pane = add_agent_pane(host, signature, signature);
    host.agents().set_session(
        pane,
        Some(verified_session(&format!("agent-sessions:{signature}"))),
    );
    pane
}

fn menu(host: &mut FakeHost, pane: Option<u64>) -> Vec<MenuEntry> {
    host.menu_items(context(pane))
}

fn only(entries: Vec<MenuEntry>) -> MenuEntry {
    assert_eq!(entries.len(), 1, "exactly one entry: {entries:?}");
    entries.into_iter().next().unwrap()
}

/// REQ: F-077
#[test]
fn f_077_a_claude_pane_is_offered_convert_to_codex() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "claude");
    let entry = only(menu(&mut host, Some(pane)));
    assert_eq!(entry.id, "to-codex");
    assert_eq!(entry.label, "Convert to Codex");
    assert_eq!(entry.icon.as_deref(), Some("arrows-exchange"));
    assert_eq!(entry.disabled_reason, None);
}

/// REQ: F-077
#[test]
fn f_077_a_codex_pane_is_offered_convert_to_claude_code() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "codex");
    let entry = only(menu(&mut host, Some(pane)));
    assert_eq!(entry.id, "to-claude");
    assert_eq!(entry.label, "Convert to Claude Code");
    assert_eq!(entry.icon.as_deref(), Some("arrows-exchange"));
    assert_eq!(entry.disabled_reason, None);
}

/// REQ: F-077
#[test]
fn f_077_an_antigravity_pane_gets_no_entry() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "antigravity");
    assert!(menu(&mut host, Some(pane)).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_custom_agent_pane_gets_no_entry() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "example-agent");
    assert!(menu(&mut host, Some(pane)).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_menu_without_a_pane_gets_no_entry() {
    let mut host = client_host();
    pane_with_session(&mut host, "claude");
    assert!(menu(&mut host, None).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_an_unknown_pane_gets_no_entry() {
    let mut host = client_host();
    assert!(menu(&mut host, Some(9999)).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_pane_without_a_verified_session_shows_the_entry_with_the_reason_no_verified_session() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude");
    let entry = only(menu(&mut host, Some(pane)));
    assert_eq!(entry.id, "to-codex");
    assert_eq!(
        entry.disabled_reason.as_deref(),
        Some("no verified session")
    );
}

/// REQ: F-077
#[test]
fn f_077_a_codex_pane_without_a_verified_session_shows_the_entry_with_the_reason_no_verified_session(
) {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "codex", "codex");
    let entry = only(menu(&mut host, Some(pane)));
    assert_eq!(entry.id, "to-claude");
    assert_eq!(
        entry.disabled_reason.as_deref(),
        Some("no verified session")
    );
}

/// REQ: F-077
#[test]
fn f_077_a_worktree_pane_shows_the_entry_with_the_reason_worktree_pane() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "claude");
    host.tree().set_worktree(pane, true);
    let entry = only(menu(&mut host, Some(pane)));
    assert_eq!(entry.disabled_reason.as_deref(), Some("worktree pane"));
}

/// REQ: F-077
#[test]
fn f_077_a_running_conversion_disables_every_entry_with_the_reason_another_conversion_runs() {
    let mut host = client_host();
    let first = pane_with_session(&mut host, "claude");
    let second = pane_with_session(&mut host, "codex");
    host.deliver_one(convert_click("to-codex", first));
    for pane in [first, second] {
        let entry = only(menu(&mut host, Some(pane)));
        assert_eq!(
            entry.disabled_reason.as_deref(),
            Some("another conversion runs"),
            "pane {pane}"
        );
    }
}

/// REQ: F-077
#[test]
fn f_077_the_entry_is_enabled_again_after_the_conversion_finished() {
    let mut host = client_host();
    let first = pane_with_session(&mut host, "claude");
    let second = pane_with_session(&mut host, "codex");
    host.deliver_one(convert_click("to-codex", first));
    host.deliver_one(message_from(
        PluginSide::Server,
        "finished",
        format!(r#"{{"pane":{first},"ok":true}}"#).as_bytes(),
    ));
    assert_eq!(only(menu(&mut host, Some(second))).disabled_reason, None);
}

/// REQ: F-077
#[test]
fn f_077_no_session_wins_over_a_running_conversion() {
    let mut host = client_host();
    let first = pane_with_session(&mut host, "claude");
    let bare = add_agent_pane(&mut host, "bare", "codex");
    host.deliver_one(convert_click("to-codex", first));
    assert_eq!(
        only(menu(&mut host, Some(bare))).disabled_reason.as_deref(),
        Some("no verified session")
    );
}

/// REQ: F-077
#[test]
fn f_077_a_running_conversion_wins_over_the_worktree_reason() {
    let mut host = client_host();
    let first = pane_with_session(&mut host, "claude");
    let tree_pane = pane_with_session(&mut host, "codex");
    host.tree().set_worktree(tree_pane, true);
    host.deliver_one(convert_click("to-codex", first));
    assert_eq!(
        only(menu(&mut host, Some(tree_pane)))
            .disabled_reason
            .as_deref(),
        Some("another conversion runs")
    );
}

/// REQ: F-077
#[test]
fn f_077_every_disabled_reason_fits_the_forty_character_menu_limit() {
    for reason in [
        "no verified session",
        "another conversion runs",
        "worktree pane",
    ] {
        assert!(reason.chars().count() <= 40, "{reason}");
    }
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude");
    let reason = only(menu(&mut host, Some(pane)))
        .disabled_reason
        .expect("reason");
    assert!(reason.chars().count() <= 40);
}

/// REQ: F-077
#[test]
fn f_077_clicking_to_codex_sends_the_start_message_to_the_server_side() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "claude");
    let submissions = host.deliver_one(convert_click("to-codex", pane));
    let sent = messages(&submissions);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].0, PLUGIN_ID);
    assert_eq!(sent[0].1, PluginSide::Server);
    assert_eq!(sent[0].2, "start");
    let payload: serde_json::Value = serde_json::from_str(&sent[0].3).expect("JSON payload");
    assert_eq!(
        payload,
        serde_json::json!({"pane": pane, "target": "codex"})
    );
}

/// REQ: F-077
#[test]
fn f_077_clicking_to_claude_sends_the_start_message_with_target_claude() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "codex");
    let submissions = host.deliver_one(convert_click("to-claude", pane));
    let sent = messages(&submissions);
    assert_eq!(sent.len(), 1, "{sent:?}");
    let payload: serde_json::Value = serde_json::from_str(&sent[0].3).expect("JSON payload");
    assert_eq!(
        payload,
        serde_json::json!({"pane": pane, "target": "claude"})
    );
}

/// REQ: F-077
#[test]
fn f_077_clicking_opens_the_modal_progress_panel_and_takes_focus() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "claude");
    let submissions = host.deliver_one(convert_click("to-codex", pane));
    let opened: Vec<_> = intents(&submissions)
        .into_iter()
        .filter_map(|intent| match intent {
            Intent::OpenView(spec) => Some((spec.view, spec.focus)),
            _ => None,
        })
        .collect();
    assert_eq!(opened, [(ViewTarget::Panel("progress".to_string()), true)]);
}

/// REQ: F-077
#[test]
fn f_077_a_click_on_a_pane_without_a_session_submits_nothing() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude");
    assert!(host.deliver_one(convert_click("to-codex", pane)).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_click_on_a_worktree_pane_submits_nothing() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "claude");
    host.tree().set_worktree(pane, true);
    assert!(host.deliver_one(convert_click("to-codex", pane)).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_second_click_while_a_conversion_runs_submits_nothing() {
    let mut host = client_host();
    let first = pane_with_session(&mut host, "claude");
    let second = pane_with_session(&mut host, "codex");
    host.deliver_one(convert_click("to-codex", first));
    assert!(host
        .deliver_one(convert_click("to-claude", second))
        .is_empty());
}

/// REQ: F-077
#[test]
fn f_077_the_entry_for_the_wrong_agent_is_refused() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "claude");
    assert!(host
        .deliver_one(convert_click("to-claude", pane))
        .is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_click_with_an_unknown_item_id_submits_nothing() {
    let mut host = client_host();
    let pane = pane_with_session(&mut host, "claude");
    assert!(host
        .deliver_one(convert_click("to-gemini", pane))
        .is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // REQ: F-077
    #[test]
    fn f_077_prop_only_claude_and_codex_signatures_get_an_entry(signature in "[a-z][a-z0-9-]{0,15}") {
        let mut host = client_host();
        let pane = pane_with_session(&mut host, &signature);
        let entries = menu(&mut host, Some(pane));
        match signature.as_str() {
            "claude" => prop_assert_eq!(entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["to-codex"]),
            "codex" => prop_assert_eq!(entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["to-claude"]),
            _ => prop_assert!(entries.is_empty()),
        }
    }
}
