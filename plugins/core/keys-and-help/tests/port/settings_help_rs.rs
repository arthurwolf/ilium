//! Twins of the ilium-client/src/settings_help tests that concern the help dialog surface.

use crate::common::*;
use ilium_plugin_sdk::prelude::*;

/// REQ: PORT:ilium-client/src/settings_help/dialog/tests.rs::dialog_layout_stays_inside_tiny_terminal_bounds
#[test]
fn settings_help_port_dialog_layout_stays_inside_tiny_terminal_bounds() {
    let mut host = tmux_host();
    let screen = host.render(keys_panel(), 32, 8);
    assert_eq!((screen.columns, screen.rows), (32, 8));
    for line in screen.text().lines() {
        assert!(cell_width(line) <= 32, "{line:?}");
    }
}

/// REQ: PORT:ilium-client/src/settings_help/dialog/tests.rs::wide_dialog_gives_the_explanation_and_illustration_separate_columns
#[test]
fn settings_help_port_wide_dialog_gives_separate_columns() {
    let mut host = column_host(Column::Tmux);
    let text = render_text(&mut host, keys_panel(), 120, 48);
    assert!(text.contains(" │ "), "{text}");
}

/// REQ: PORT:ilium-client/src/settings_help/tests.rs::closing_help_restores_the_exact_settings_navigation_state
#[test]
fn settings_help_port_closing_help_restores_the_exact_settings_navigation_state() {
    let mut host = column_host(Column::Tmux);
    let before = host.render(keyboard_page(), 100, 40).text();
    host.key(keys_panel(), "Esc");
    host.send_action(keys_panel(), "keys", ActionKind::Closed, ActionValue::None);
    host.settle();
    let after = host.render(keyboard_page(), 100, 40).text();
    assert_eq!(normalized(&before), normalized(&after));
    assert!(host.kernel_settings().writes().is_empty());
}

/// REQ: PORT:ilium-client/src/settings_help/dialog/tests.rs::narrow_dialog_stacks_its_panels_without_clipping_the_footer_area
#[test]
fn settings_help_port_narrow_dialog_keeps_the_footer_hint_unclipped() {
    let mut host = column_host(Column::Tmux);
    let text = render_text(&mut host, keys_panel(), 80, 24);
    assert!(text.contains("Esc"), "{text}");
    for line in text.lines() {
        assert!(cell_width(line) <= 80, "{line:?}");
    }
}

/// REQ: PORT:ilium-client/src/settings_help/tests.rs::catalog_covers_every_settings_help_id_once_with_complete_content
#[test]
fn settings_help_port_catalog_lists_the_keys_overview_topic_once_with_content() {
    let manifest = std::fs::read_to_string(manifest_path()).expect("manifest");
    assert_eq!(manifest.matches("\"keys-overview\"").count(), 1);
    let mut host = tmux_host();
    assert!(!render_text(&mut host, ViewTarget::Panel("keys".to_string()), 100, 30).trim().is_empty());
}
