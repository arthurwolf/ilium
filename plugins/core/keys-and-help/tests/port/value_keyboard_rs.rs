//! Twins of ilium-client/src/value_keyboard.rs: the prefix controls of the Keyboard tab.

use serde_json::json;

use crate::common::*;
use ilium_plugin_sdk::prelude::*;

fn change(host: &mut ilium_plugin_sdk::testing::FakeHost, key: &str, value: &str) {
    host.send_action(keyboard_page(), key, ActionKind::Changed, ActionValue::Text(value.to_string()));
    host.settle();
}

/// REQ: PORT:ilium-client/src/value_keyboard.rs::both_prefix_catalogs_contain_every_letter_and_paint_precise_controls
#[test]
fn value_keyboard_port_both_prefix_catalogs_contain_every_letter_and_paint_precise_controls() {
    for key in ["shortcut_base", "navigation_shortcut_base"] {
        for letter in 'a'..='z' {
            let mut host = tmux_host();
            change(&mut host, key, &format!("ctrl+{letter}"));
            assert!(
                host.kernel_settings().writes().iter().any(|(_, k, v)| k == key && v == &format!("\"ctrl+{letter}\"")),
                "{key} {letter}"
            );
        }
    }
    for columns in [24u16, 40, 80, 140] {
        let mut host = tmux_host();
        let text = render_text(&mut host, keyboard_page(), columns, 30);
        assert!(text.contains("Ctrl+B"), "value clipped at {columns}: {text}");
    }
}

/// REQ: PORT:ilium-client/src/value_keyboard.rs::prefix_catalog_retains_selection_after_save_failure_then_retries_without_remapping
#[test]
fn value_keyboard_port_prefix_catalog_retains_selection_after_save_failure_then_retries_without_remapping() {
    let mut host = tmux_host();
    host.inject_failure(
        "kernel-settings.write",
        PluginError { code: ErrorCode::NotPermitted, message: "read-only file".to_string(), retry_after_ms: None },
    );
    change(&mut host, "shortcut_base", "ctrl+z");
    let text = render_text(&mut host, keyboard_page(), 100, 30);
    assert!(text.contains("Ctrl+B"), "the saved prefix is unchanged: {text}");
    assert!(text.contains("Prefix is Ctrl+B."), "the panel still states the saved prefix: {text}");
    assert!(!text.contains("Ctrl+Z"), "the rejected choice is not painted as saved: {text}");
    assert!(host.kernel_settings().tab("keyboard").to_string().contains("ctrl+b"));
    change(&mut host, "shortcut_base", "ctrl+z");
    assert!(host.kernel_settings().writes().iter().any(|(_, k, v)| k == "shortcut_base" && v == "\"ctrl+z\""));
    assert!(host.kernel_settings().tab("keyboard").to_string().contains("navigation_shortcut_base"));
    assert!(render_text(&mut host, keyboard_page(), 100, 30).contains("Navigation prefix"));
}

/// REQ: PORT:ilium-client/src/value_keyboard.rs::settings_renderer_paints_both_prefixes_at_the_pointer_geometry
#[test]
fn value_keyboard_port_settings_renderer_paints_both_prefixes() {
    let mut host = host_with_tab(json!({"shortcut_base": "ctrl+b", "navigation_shortcut_base": "ctrl+g", "preset": "tmux"}));
    let screen = host.render(keyboard_page(), 100, 30);
    for (label, value) in [("Navigation prefix", "Ctrl+G"), ("Prefix", "Ctrl+B")] {
        let (row, _) = screen.find(label).unwrap_or_else(|| panic!("{label}: {}", screen.text()));
        let line = screen.row(row);
        assert!(line.contains(value), "{label} row carries {value}: {line:?}");
        assert!(line.contains('\u{2026}'), "{label} row has a dotted leader: {line:?}");
        assert!(line.find(label) < line.find(value), "label left of value: {line:?}");
    }
}
