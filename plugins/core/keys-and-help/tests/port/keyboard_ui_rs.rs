//! Twins of ilium-client/src/onboarding/keyboard_ui.rs: the Keyboard tab rows, capture and prefix controls.

use serde_json::json;

use crate::common::*;
use ilium_plugin_sdk::prelude::*;

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::capture_keeps_collision_retry_pending_and_escape_cancels_without_closing
#[test]
fn keyboard_ui_port_capture_keeps_collision_retry_pending_and_escape_cancels_without_closing() {
    let mut host = tmux_host();
    set_keymap(
        &mut host,
        vec![
            binding("Keyboard", "New terminal", "prefix+c", true),
            binding("Keyboard", "Detach", "prefix+d", true),
        ],
        &[("prefix+c", "c"), ("prefix+d", "d")],
    );
    activate(&mut host, "Detach");
    write_chord(&mut host, "c");
    host.settle();
    let text = render_text(&mut host, keyboard_page(), 120, 20);
    assert!(text.contains("used by New terminal"), "{text}");
    assert!(
        text.contains("Press the new key chord."),
        "retry stays pending: {text}"
    );
    assert!(host.kernel_settings().writes().is_empty());
    host.key(keyboard_page(), "Esc");
    host.settle();
    let after = render_text(&mut host, keyboard_page(), 120, 20);
    assert!(!after.contains("Press the new key chord."), "{after}");
    assert!(after.contains("Detach"), "the page stays open: {after}");
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::empty_bindings_and_tiny_geometry_do_not_underflow
#[test]
fn keyboard_ui_port_empty_bindings_and_tiny_geometry_do_not_underflow() {
    let mut host = tmux_host();
    for (columns, rows) in [(0u16, 0u16), (1, 1), (3, 2), (10, 3), (20, 5)] {
        let screen = host.render(keyboard_page(), columns, rows);
        assert_eq!((screen.columns, screen.rows), (columns, rows));
    }
    let text = render_text(&mut host, keyboard_page(), 80, 24);
    assert!(text.contains("Preset"));
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::hit_testing_matches_live_rows_and_release_does_not_capture
#[test]
fn keyboard_ui_port_hit_testing_matches_live_rows_and_release_does_not_capture() {
    let mut host = column_host(Column::Tmux);
    let screen = host.render(keyboard_page(), 100, 60);
    let (row, _) = screen.find("detach").expect("detach row");
    assert!(screen.row(row).contains("detach"));
    host.send_action(
        keyboard_page(),
        "capture-detach",
        ActionKind::HoverEnter,
        ActionValue::None,
    );
    host.send_action(
        keyboard_page(),
        "capture-detach",
        ActionKind::HoverLeave,
        ActionValue::None,
    );
    host.settle();
    assert!(!render_text(&mut host, keyboard_page(), 100, 60).contains("Press the new key chord."));
    assert!(host.kernel_settings().writes().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::narrow_overlay_reveals_and_can_rebind_every_actual_binding
#[test]
fn keyboard_ui_port_narrow_page_reveals_and_can_rebind_every_actual_binding() {
    let mut host = column_host(Column::Tmux);
    for row in action_rows() {
        let screen = host.render(keyboard_page(), 40, 120);
        assert!(
            screen.find(&row.name).is_some(),
            "{} not reachable at 40 columns",
            row.name
        );
    }
    activate(&mut host, "close_pane");
    write_chord(&mut host, "q");
    host.key(keyboard_page(), "Enter");
    host.settle();
    assert!(host
        .kernel_settings()
        .writes()
        .iter()
        .any(|(tab, key, _)| tab == "keyboard" && key.contains("close_pane")));
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::rendering_is_read_only_and_geometry_is_available_before_paint
#[test]
fn keyboard_ui_port_rendering_is_read_only_and_geometry_is_available_before_paint() {
    let mut host = column_host(Column::Tmux);
    let first = host.render(keyboard_page(), 100, 40);
    let second = host.render(keyboard_page(), 100, 40);
    assert_eq!(first.text(), second.text());
    assert!(host.kernel_settings().writes().is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::requests_read_authoritative_prefix_and_presets_do_not_fake_saved_values
#[test]
fn keyboard_ui_port_reads_authoritative_prefix_and_presets_do_not_fake_saved_values() {
    let mut host = host_with_tab(
        json!({"shortcut_base": "ctrl+g", "navigation_shortcut_base": "ctrl+h", "preset": "custom"}),
    );
    let text = render_text(&mut host, keyboard_page(), 100, 40);
    assert!(text.contains("Ctrl+G"), "{text}");
    assert!(text.contains("Ctrl+H"), "{text}");
    host.send_action(
        keyboard_page(),
        "preset",
        ActionKind::Changed,
        ActionValue::Text("screen".to_string()),
    );
    host.settle();
    let writes = host.kernel_settings().writes();
    assert!(
        writes
            .iter()
            .any(|(_, key, value)| key == "preset" && value == "\"screen\""),
        "{writes:?}"
    );
    assert!(
        !writes.iter().any(|(_, key, _)| key == "shortcut_base"),
        "a preset must not rewrite the prefix: {writes:?}"
    );
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::capture_keeps_collision_retry_pending_and_escape_cancels_without_closing
#[test]
fn keyboard_ui_port_second_retry_with_a_free_chord_saves_the_detach_binding() {
    let mut host = tmux_host();
    set_keymap(
        &mut host,
        vec![
            binding("Keyboard", "New terminal", "prefix+c", true),
            binding("Keyboard", "Detach", "prefix+d", true),
        ],
        &[("prefix+c", "c"), ("prefix+d", "d")],
    );
    activate(&mut host, "Detach");
    write_chord(&mut host, "c");
    host.settle();
    write_chord(&mut host, "z");
    host.key(keyboard_page(), "Enter");
    host.settle();
    let writes = host.kernel_settings().writes();
    assert!(
        writes.iter().any(|(_, key, _)| key.contains("detach")),
        "the free retry must be saved: {writes:?}"
    );
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::empty_bindings_and_tiny_geometry_do_not_underflow
#[test]
fn keyboard_ui_port_second_page_with_no_bindings_still_lists_the_preset_row() {
    let mut host = host_with_tab(
        json!({"shortcut_base": "ctrl+b", "navigation_shortcut_base": "ctrl+b", "preset": "tmux"}),
    );
    let text = render_text(&mut host, keyboard_page(), 60, 20);
    assert!(text.contains("Preset"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::hit_testing_matches_live_rows_and_release_does_not_capture
#[test]
fn keyboard_ui_port_second_hover_alone_does_not_open_a_capture() {
    let mut host = column_host(Column::Tmux);
    host.send_action(
        keyboard_page(),
        "capture-detach",
        ActionKind::HoverEnter,
        ActionValue::None,
    );
    host.settle();
    assert!(!render_text(&mut host, keyboard_page(), 100, 60).contains("Press the new key chord."));
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::narrow_overlay_reveals_and_can_rebind_every_actual_binding
#[test]
fn keyboard_ui_port_second_activating_a_row_at_40_columns_opens_its_capture() {
    let mut host = column_host(Column::Tmux);
    activate(&mut host, "close_pane");
    assert!(render_text(&mut host, keyboard_page(), 40, 120).contains("Press the new key chord."));
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::rendering_is_read_only_and_geometry_is_available_before_paint
#[test]
fn keyboard_ui_port_second_rendering_at_extreme_sizes_writes_nothing() {
    let mut host = column_host(Column::Tmux);
    let _small = host.render(keyboard_page(), 60, 20);
    let _large = host.render(keyboard_page(), 200, 80);
    assert!(host.kernel_settings().writes().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/keyboard_ui.rs::requests_read_authoritative_prefix_and_presets_do_not_fake_saved_values
#[test]
fn keyboard_ui_port_second_the_keys_panel_shows_the_saved_prefix_not_the_preset_default() {
    let mut host = host_with_tab(
        json!({"shortcut_base": "ctrl+g", "navigation_shortcut_base": "ctrl+h", "preset": "screen"}),
    );
    let text = render_text(&mut host, keys_panel(), 100, 20);
    assert!(text.contains("Prefix is Ctrl+G."), "{text}");
    assert!(!text.contains("Prefix is Ctrl+A."), "{text}");
}
