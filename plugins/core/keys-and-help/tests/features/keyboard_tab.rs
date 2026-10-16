//! The Keyboard tab (settings page `keyboard`): preset, the two prefixes, the 35 action rows, the
//! third figure of the page as a golden, and the writes through `settings.write-kernel:keyboard`.

use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::testing::assert_golden;
use serde_json::json;

use crate::common::*;

fn keyboard_page() -> ViewTarget {
    ViewTarget::SettingsPage("keyboard".to_string())
}

/// A host whose keymap holds the 35 actions of the table with their tmux keys.
fn full_keymap_host() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = seeded_host();
    let bindings: Vec<_> = action_rows().iter().map(|row| action_binding(row, &row.tmux)).collect();
    for row in action_rows() {
        host.keymap().set_label(&format!("prefix+{}", row.tmux), &row.tmux);
    }
    host.keymap().set_bindings(bindings);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    host
}

/// REQ: F-007
#[test]
fn keyboard_007_the_tab_title_is_keyboard() {
    let mut host = full_keymap_host();
    assert!(host.render(keyboard_page(), 80, 60).text().contains("Keyboard"));
}

/// REQ: F-007
#[test]
fn keyboard_007_the_tab_lists_all_35_actions_by_name() {
    let mut host = full_keymap_host();
    let text = host.render(keyboard_page(), 80, 60).text();
    for row in action_rows() {
        assert!(text.contains(&row.name), "{} missing from the Keyboard tab", row.name);
    }
}

/// REQ: F-007
#[test]
fn keyboard_007_each_action_row_has_its_key_after_a_leader() {
    let mut host = full_keymap_host();
    let screen = host.render(keyboard_page(), 80, 60);
    let (row, _) = screen.find("new_terminal").expect("new_terminal row");
    let line = screen.row(row);
    let after_name = &line[line.find("new_terminal").unwrap() + "new_terminal".len()..];
    assert!(after_name.contains('…'));
    assert!(after_name.trim_end_matches(|c: char| c == ' ' || c == '│').ends_with('c'), "{line}");
}

/// REQ: F-007
#[test]
fn keyboard_007_the_tab_shows_the_preset_and_the_general_prefix() {
    let mut host = full_keymap_host();
    let text = host.render(keyboard_page(), 80, 60).text();
    assert!(text.contains("Preset"));
    assert!(text.contains("tmux"));
    assert!(text.contains("Prefix"));
    assert!(text.contains("Ctrl+B"));
}

/// REQ: F-007
#[test]
fn keyboard_007_the_tab_shows_the_navigation_prefix_row() {
    let mut host = full_keymap_host();
    assert!(host.render(keyboard_page(), 80, 60).text().contains("Navigation prefix"));
}

/// REQ: F-007
#[test]
fn keyboard_007_moving_the_general_prefix_keeps_the_navigation_prefix_label() {
    let mut host = full_keymap_host();
    host.kernel_settings().set_tab(
        "keyboard",
        json!({"shortcut_base": "ctrl+a", "navigation_shortcut_base": "ctrl+b", "preset": "custom"}),
    );
    host.deliver_one(Event::KernelSettingsChanged(ilium_plugin_sdk::types::events::KernelSettingsEvent {
        tab: "keyboard".to_string(),
        keys: vec!["shortcut_base".to_string()],
    }));
    host.settle();
    let screen = host.render(keyboard_page(), 80, 60);
    let (prefix_row, _) = screen.find("Prefix").expect("Prefix row");
    let (navigation_row, _) = screen.find("Navigation prefix").expect("Navigation prefix row");
    assert!(screen.row(prefix_row).contains("Ctrl+A"));
    assert!(screen.row(navigation_row).contains("Ctrl+B"));
}

/// REQ: F-007
#[test]
fn keyboard_007_the_tab_follows_a_kernel_settings_change_of_the_preset() {
    let mut host = full_keymap_host();
    host.kernel_settings().set_tab(
        "keyboard",
        json!({"shortcut_base": "ctrl+a", "navigation_shortcut_base": "ctrl+b", "preset": "screen"}),
    );
    host.deliver_one(Event::KernelSettingsChanged(ilium_plugin_sdk::types::events::KernelSettingsEvent {
        tab: "keyboard".to_string(),
        keys: vec!["preset".to_string()],
    }));
    host.settle();
    assert!(host.render(keyboard_page(), 80, 60).text().contains("GNU Screen"));
}

/// REQ: F-007
#[test]
fn keyboard_007_a_change_of_another_tab_does_not_invalidate_the_keyboard_page() {
    let mut host = full_keymap_host();
    let before = host.invalidated().len();
    host.deliver_one(Event::KernelSettingsChanged(ilium_plugin_sdk::types::events::KernelSettingsEvent {
        tab: "terminal".to_string(),
        keys: vec!["scrollback".to_string()],
    }));
    host.settle();
    assert_eq!(host.invalidated().len(), before);
}

/// REQ: F-007
#[test]
fn keyboard_007_a_change_of_the_keyboard_tab_invalidates_the_keyboard_page() {
    let mut host = full_keymap_host();
    host.deliver_one(Event::KernelSettingsChanged(ilium_plugin_sdk::types::events::KernelSettingsEvent {
        tab: "keyboard".to_string(),
        keys: vec!["shortcut_base".to_string()],
    }));
    host.settle();
    assert!(host.invalidated().contains(&keyboard_page()));
}

/// REQ: F-007, COR-011
#[test]
fn keyboard_007_figure_three_at_48_columns_equals_the_golden() {
    let mut host = seeded_host();
    let rows = action_rows();
    let shown = ["new_terminal", "close_pane", "rename", "focus_pane_left", "quit"];
    let bindings: Vec<_> = rows
        .iter()
        .filter(|row| shown.contains(&row.name.as_str()))
        .map(|row| action_binding(row, &row.tmux))
        .collect();
    for row in rows.iter().filter(|row| shown.contains(&row.name.as_str())) {
        host.keymap().set_label(&format!("prefix+{}", row.tmux), &row.tmux);
    }
    host.keymap().set_bindings(bindings);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    let screen = host.render(keyboard_page(), 48, 11);
    assert_golden(&golden_path("keyboard_tab_48.txt"), &normalized(&screen.text()));
}

/// REQ: F-007, COR-011
#[test]
fn keyboard_007_the_golden_is_the_figure_of_the_page() {
    let figure = page_figures().remove(2);
    let golden = std::fs::read_to_string(golden_path("keyboard_tab_48.txt")).expect("golden file");
    assert_eq!(normalized(&golden), normalized(&figure));
}

/// REQ: F-007
#[test]
fn keyboard_007_the_plugin_section_adds_the_show_keys_row_at_the_end_of_the_tab() {
    let mut host = full_keymap_host();
    let screen = host.render(keyboard_page(), 80, 80);
    let (section_row, _) = screen.find("Plugin keys").expect("section title");
    let (last_action_row, _) = screen.find("settings").expect("last action");
    assert!(section_row > last_action_row);
    let (key_row, _) = screen.find("Key reference").expect("keybinding row");
    assert!(key_row > section_row);
    assert!(screen.row(key_row).contains('?'));
}

/// REQ: F-007
#[test]
fn keyboard_007_the_show_keys_row_default_is_the_question_mark() {
    let mut host = full_keymap_host();
    assert_eq!(host.settings().get("show_keys"), Some(json!("?")));
}

/// REQ: F-007
#[test]
fn keyboard_007_choosing_a_prefix_writes_shortcut_base_to_the_keyboard_tab() {
    let mut host = full_keymap_host();
    host.send_action(keyboard_page(), "shortcut_base", ActionKind::Changed, ActionValue::Text("ctrl+a".to_string()));
    host.settle();
    assert_eq!(
        host.kernel_settings().writes(),
        vec![("keyboard".to_string(), "shortcut_base".to_string(), "\"ctrl+a\"".to_string())]
    );
}

/// REQ: F-007
#[test]
fn keyboard_007_choosing_the_navigation_prefix_writes_navigation_shortcut_base() {
    let mut host = full_keymap_host();
    host.send_action(
        keyboard_page(),
        "navigation_shortcut_base",
        ActionKind::Changed,
        ActionValue::Text("ctrl+c".to_string()),
    );
    host.settle();
    assert_eq!(
        host.kernel_settings().writes(),
        vec![("keyboard".to_string(), "navigation_shortcut_base".to_string(), "\"ctrl+c\"".to_string())]
    );
}

/// REQ: F-007
#[test]
fn keyboard_007_a_prefix_that_is_not_one_ascii_letter_is_refused_with_invalid_argument() {
    let mut host = full_keymap_host();
    host.send_action(keyboard_page(), "shortcut_base", ActionKind::Changed, ActionValue::Text("ctrl+ab".to_string()));
    host.settle();
    assert!(host.kernel_settings().writes().is_empty());
    assert!(host.failures().iter().any(|failure| failure.code == "invalid-argument"));
    assert!(host.render(keyboard_page(), 80, 60).text().contains("one letter"));
}

/// REQ: F-007
#[test]
fn keyboard_007_a_digit_is_not_a_valid_prefix() {
    let mut host = full_keymap_host();
    host.send_action(keyboard_page(), "shortcut_base", ActionKind::Changed, ActionValue::Text("ctrl+1".to_string()));
    host.settle();
    assert!(host.kernel_settings().writes().is_empty());
}

/// REQ: F-007
#[test]
fn keyboard_007_the_prefix_boundary_letters_a_and_z_are_accepted() {
    for letter in ["ctrl+a", "ctrl+z"] {
        let mut host = full_keymap_host();
        host.send_action(keyboard_page(), "shortcut_base", ActionKind::Changed, ActionValue::Text(letter.to_string()));
        host.settle();
        assert_eq!(host.kernel_settings().writes().len(), 1, "{letter}");
    }
}

/// REQ: F-007
#[test]
fn keyboard_007_a_failed_write_keeps_the_stored_values_and_shows_the_error() {
    let mut host = full_keymap_host();
    host.inject_failure(
        "kernel-settings.write",
        PluginError { code: ErrorCode::NotPermitted, message: "revoked".to_string(), retry_after_ms: None },
    );
    host.send_action(keyboard_page(), "shortcut_base", ActionKind::Changed, ActionValue::Text("ctrl+a".to_string()));
    host.settle();
    assert_eq!(host.kernel_settings().tab("keyboard").expect("tab")["shortcut_base"], "ctrl+b");
    assert!(host.render(keyboard_page(), 80, 60).text().contains("revoked"));
}

/// REQ: F-007
#[test]
fn keyboard_007_the_plugin_without_the_write_grant_cannot_write_and_reports_not_permitted() {
    let mut host = full_keymap_host();
    host.revoke("settings.write-kernel:keyboard");
    host.send_action(keyboard_page(), "shortcut_base", ActionKind::Changed, ActionValue::Text("ctrl+a".to_string()));
    host.settle();
    assert!(host.kernel_settings().writes().is_empty());
    assert!(host.failures().iter().any(|failure| failure.code == "not-permitted"));
}

/// REQ: F-007
#[test]
fn keyboard_007_choosing_a_preset_writes_the_preset_key() {
    let mut host = full_keymap_host();
    host.send_action(keyboard_page(), "preset", ActionKind::Changed, ActionValue::Text("screen".to_string()));
    host.settle();
    assert_eq!(
        host.kernel_settings().writes(),
        vec![("keyboard".to_string(), "preset".to_string(), "\"screen\"".to_string())]
    );
}

/// REQ: F-007, COR-033
#[test]
fn keyboard_033_one_edit_writes_exactly_one_key_and_leaves_unknown_keys_alone() {
    let mut host = full_keymap_host();
    host.kernel_settings().set_tab(
        "keyboard",
        json!({"shortcut_base": "ctrl+b", "navigation_shortcut_base": "ctrl+b", "preset": "tmux", "future_key": 7}),
    );
    host.send_action(keyboard_page(), "shortcut_base", ActionKind::Changed, ActionValue::Text("ctrl+a".to_string()));
    host.settle();
    let writes = host.kernel_settings().writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].1, "shortcut_base");
    assert_eq!(host.kernel_settings().tab("keyboard").expect("tab")["future_key"], 7);
}

/// REQ: F-007, COR-033
#[test]
fn keyboard_033_the_plugin_only_ever_writes_the_keyboard_tab() {
    let mut host = full_keymap_host();
    host.send_action(keyboard_page(), "shortcut_base", ActionKind::Changed, ActionValue::Text("ctrl+d".to_string()));
    host.send_action(keyboard_page(), "preset", ActionKind::Changed, ActionValue::Text("tmux".to_string()));
    host.settle();
    assert!(host.kernel_settings().writes().iter().all(|(tab, _, _)| tab == "keyboard"));
}

/// REQ: F-007
#[test]
fn keyboard_007_the_secrets_marker_in_a_read_is_not_rewritten_as_a_value() {
    let mut host = full_keymap_host();
    host.kernel_settings().set_tab("keyboard", json!({"shortcut_base": "<redacted>"}));
    host.deliver_one(Event::KernelSettingsChanged(ilium_plugin_sdk::types::events::KernelSettingsEvent {
        tab: "keyboard".to_string(),
        keys: vec!["shortcut_base".to_string()],
    }));
    host.settle();
    assert!(host.kernel_settings().writes().is_empty());
}

/// REQ: COR-022, F-007
#[test]
fn keyboard_022_when_the_plugin_is_disabled_the_tab_shows_the_values_read_only_with_the_notice() {
    let mut session = ilium_plugin_sdk::testing::FakeSession::new();
    session.install_core("keys-and-help").start();
    session.pump().expect("pump");
    session.plugin(PLUGIN_ID).kernel_settings().set_tab("keyboard", json!({"shortcut_base": "ctrl+b"}));
    session.disable(PLUGIN_ID);
    session.pump().expect("pump");
    let screen = session.plugin(PLUGIN_ID).render(keyboard_page(), 80, 20);
    assert!(screen.text().contains("Keys and help is disabled. Values are kept."), "{}", screen.text());
}
