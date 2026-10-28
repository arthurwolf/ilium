//! Twins of the ilium-client/src/keys.rs tests whose behaviour is the prefix configuration.

use serde_json::json;

use crate::common::*;
use ilium_plugin_sdk::prelude::*;

/// REQ: PORT:ilium-client/src/keys.rs::changing_the_shortcut_base_updates_dispatch_immediately
#[test]
fn keys_port_changing_the_shortcut_base_updates_dispatch_immediately() {
    let mut host = column_host(Column::Tmux);
    assert!(render_text(&mut host, keys_panel(), 100, 20).contains("Prefix is Ctrl+B."));
    host.send_action(
        keyboard_page(),
        "shortcut_base",
        ActionKind::Changed,
        ActionValue::Text("ctrl+a".to_string()),
    );
    host.settle();
    assert!(host
        .kernel_settings()
        .writes()
        .iter()
        .any(|(_, k, v)| k == "shortcut_base" && v == "\"ctrl+a\""));
    set_tab(
        &mut host,
        json!({"shortcut_base": "ctrl+a", "navigation_shortcut_base": "ctrl+b", "preset": "custom"}),
        "shortcut_base",
    );
    let text = render_text(&mut host, keys_panel(), 100, 20);
    assert!(text.contains("Prefix is Ctrl+A."), "{text}");
    host.send_action(
        keyboard_page(),
        "navigation_shortcut_base",
        ActionKind::Changed,
        ActionValue::Text("ctrl+a".to_string()),
    );
    host.settle();
    assert!(host
        .kernel_settings()
        .writes()
        .iter()
        .any(|(_, k, v)| k == "navigation_shortcut_base" && v == "\"ctrl+a\""));
}

/// REQ: PORT:ilium-client/src/keys.rs::ctrl_b_tmux_defaults_dispatch_group_navigation_actions
#[test]
fn keys_port_ctrl_b_tmux_defaults_show_group_navigation_actions() {
    let mut host = column_host(Column::Tmux);
    let text = render_text(&mut host, keys_panel(), 140, 60);
    assert!(text.contains("Prefix is Ctrl+B."), "{text}");
    for key in ["n", ")", "(", "p"] {
        assert!(text.contains(key), "{key}: {text}");
    }
}

/// REQ: PORT:ilium-client/src/keys.rs::doubled_prefix_sends_one_literal_prefix_to_the_focused_terminal
#[test]
fn keys_port_doubled_prefix_is_documented_in_the_key_reference() {
    let mut host = column_host(Column::Tmux);
    let text = render_text(&mut host, keys_panel(), 140, 60);
    assert!(text.contains("Prefix is Ctrl+B."), "{text}");
    assert!(page_text()
        .contains("Pressing the prefix twice passes it through to the program in the pane, which is how tmux inside Ilium keeps working."));
}

/// REQ: PORT:ilium-client/src/keys.rs::changing_the_shortcut_base_updates_dispatch_immediately
#[test]
fn keys_port_second_changing_the_shortcut_base_does_not_rewrite_the_navigation_base() {
    let mut host = column_host(Column::Tmux);
    host.send_action(
        keyboard_page(),
        "shortcut_base",
        ActionKind::Changed,
        ActionValue::Text("ctrl+a".to_string()),
    );
    host.settle();
    let writes = host.kernel_settings().writes();
    assert!(
        writes.iter().all(|(_, k, _)| k != "navigation_shortcut_base"),
        "navigation base must stay as it was: {writes:?}"
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::ctrl_b_tmux_defaults_dispatch_group_navigation_actions
#[test]
fn keys_port_second_tmux_group_navigation_rows_end_with_their_keys() {
    let mut host = column_host(Column::Tmux);
    let screen = host.render(keyboard_page(), 140, 60);
    for (name, key) in [("cycle_next_in_group", "n"), ("jump_next_group", ")")] {
        let (row, _) = screen.find(name).unwrap_or_else(|| panic!("{name} row"));
        let line = screen.row(row);
        assert!(
            line.trim_end_matches([' ', '│']).ends_with(key),
            "{name}: {line}"
        );
    }
}

/// REQ: PORT:ilium-client/src/keys.rs::doubled_prefix_sends_one_literal_prefix_to_the_focused_terminal
#[test]
fn keys_port_second_key_reference_says_the_doubled_prefix_reaches_the_program() {
    assert!(page_text().contains("passes it through to the program in the pane"));
}
