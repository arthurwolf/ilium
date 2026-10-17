//! Twins of tree_ui.rs: the tree toolbar.
//! Request/reply shape: see API_GAPS_ports.md.
use crate::common::{ask, Place};
use serde_json::Value;

fn check(place: Place, op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(place, op, request);
    let fields = expected.as_object().expect("expected is an object");
    for (name, value) in fields {
        assert_eq!(
            &reply[name.as_str()],
            value,
            "field {name} of reply {reply}"
        );
    }
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::toolbar_and_row_controls_use_their_configured_icons
#[test]
fn port_tree_ui_toolbar_and_row_controls_use_their_configured_icons() {
    check(
        Place::Client,
        "tree-toolbar.icons",
        r####"{}"####,
        r####"{"follows_configured_icons": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::toolbar_is_visible_for_tree_focus_or_footer_hover
#[test]
fn port_tree_ui_toolbar_is_visible_for_tree_focus_or_footer_hover() {
    check(
        Place::Client,
        "tree-toolbar.visible",
        r####"{"cases": [[true, false], [false, true], [true, true], [false, false]]}"####,
        r####"{"visible": [true, true, true, false]}"####,
    );
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::toolbar_keeps_settings_right_aligned_without_overlapping_left_actions
#[test]
fn port_tree_ui_toolbar_keeps_settings_right_aligned_without_overlapping_left_actions() {
    check(
        Place::Client,
        "tree-toolbar.layout",
        r####"{"width": 16}"####,
        r####"{"toolbar_width": 14, "project_x": 4, "restructure_x": 8, "settings_x": [12, 13], "overlap": false}"####,
    );
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::stopped_agent_tree_row_keeps_recovery_indicator_in_normal_and_attention_modes
#[test]
fn port_tree_ui_stopped_agent_row_keeps_its_recovery_indicator_in_normal_and_attention_modes() {
    for agent in ["codex", "claude"] {
        for mode in ["normal", "attention"] {
            for stable_glyphs in [false, true] {
                let request = serde_json::json!({
                    "agent": agent, "display_mode": mode, "stable_glyphs": stable_glyphs,
                    "row": {"kind": "agent-unavailable", "title": "Recover authored work"},
                });
                let reply = ask(Place::Client, "tree-row.render", request.clone());
                assert_eq!(
                    reply["indicator"], "agent-unavailable",
                    "{request} -> {reply}"
                );
                assert_eq!(
                    reply["text"], "Recover authored work [unavailable]",
                    "{request} -> {reply}"
                );
                assert_eq!(reply["agent_state"], Value::Null, "{request} -> {reply}");
            }
        }
    }
}

/// REQ: PORT:ilium-client/src/keys.rs::agent_context_menu_enter_enter_creates_here_without_worktree
#[test]
fn port_tree_ui_agent_context_menu_enter_enter_creates_here_without_worktree() {
    // Assumed op `tree-toolbar.context-menu-keys`: replays chords on the tree context menu of a group.
    check(
        Place::Client,
        "tree-toolbar.context-menu-keys",
        r####"{"select": "new-agent:claude", "chords": ["enter", "enter"]}"####,
        r####"{"submenu_after_first_enter": 0, "menu_open": false, "created": [{"kind": "command", "command": "claude", "worktree": false}]}"####,
    );
}
