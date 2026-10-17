//! Behaviour of the original Help screen (ilium-client help.rs) that the plugin form must
//! reproduce: the close hint is always visible, shrinking at narrow widths, and a live remap shows
//! in the action table.

use ilium_plugin_sdk::prelude::*;

use crate::common::*;

fn keys_panel() -> ViewTarget {
    ViewTarget::Panel("keys".to_string())
}

fn host_with_help_key() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = seeded_host();
    host.keymap().set_label("prefix+?", "?");
    host.keymap()
        .set_bindings(vec![binding("Help", "Help", "prefix+?", true)]);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    host
}

/// REQ: F-010
#[test]
fn overlay_010_the_close_hint_names_the_prefix_and_the_help_key() {
    let mut host = host_with_help_key();
    assert!(host
        .render(keys_panel(), 80, 24)
        .text()
        .contains("press Ctrl+B ? again, or Esc, to close"));
}

/// REQ: F-010
#[test]
fn overlay_010_the_close_hint_follows_a_changed_prefix() {
    let mut host = host_with_help_key();
    host.kernel_settings().set_tab(
        "keyboard",
        serde_json::json!({"shortcut_base": "ctrl+a", "navigation_shortcut_base": "ctrl+b", "preset": "screen"}),
    );
    host.deliver_one(Event::KernelSettingsChanged(
        ilium_plugin_sdk::types::events::KernelSettingsEvent {
            tab: "keyboard".to_string(),
            keys: vec!["shortcut_base".to_string()],
        },
    ));
    host.settle();
    assert!(host
        .render(keys_panel(), 80, 24)
        .text()
        .contains("press Ctrl+A ? again, or Esc, to close"));
}

/// REQ: F-010
#[test]
fn overlay_010_a_remapped_help_key_appears_in_the_close_hint() {
    let mut host = host_with_help_key();
    host.keymap().set_label("prefix+h", "h");
    host.keymap()
        .set_bindings(vec![binding("Help", "Help", "prefix+h", false)]);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    assert!(host
        .render(keys_panel(), 80, 24)
        .text()
        .contains("press Ctrl+B h again, or Esc, to close"));
}

/// REQ: F-010
#[test]
fn overlay_010_the_close_hint_fits_exactly_at_40_columns() {
    let mut host = host_with_help_key();
    // 38 cells of text between two border cells.
    assert!(host
        .render(keys_panel(), 40, 24)
        .text()
        .contains("press Ctrl+B ? again, or Esc, to close"));
}

/// REQ: F-010
#[test]
fn overlay_010_one_cell_narrower_the_hint_shrinks_to_esc_closes() {
    let mut host = host_with_help_key();
    let text = host.render(keys_panel(), 39, 24).text();
    assert!(text.contains("Esc closes"), "{text}");
    assert!(!text.contains("again, or Esc, to close"));
}

/// REQ: F-010
#[test]
fn overlay_010_below_ten_inner_cells_the_hint_is_just_esc() {
    let mut host = host_with_help_key();
    let text = host.render(keys_panel(), 11, 24).text();
    assert!(!text.contains("Esc closes"), "{text}");
    assert!(text.contains("Esc"), "{text}");
}

/// REQ: F-010
#[test]
fn overlay_010_ten_inner_cells_still_show_esc_closes() {
    let mut host = host_with_help_key();
    assert!(host
        .render(keys_panel(), 12, 24)
        .text()
        .contains("Esc closes"));
}

/// REQ: F-010
#[test]
fn overlay_010_the_close_hint_stays_the_last_visible_row_on_a_short_terminal() {
    for height in 4..=12u16 {
        let mut host = host_with_help_key();
        let screen = host.render(keys_panel(), 80, height);
        assert!(
            screen.text().contains("again, or Esc, to close"),
            "close hint clipped at terminal height {height}"
        );
    }
}

/// REQ: F-010
#[test]
fn overlay_010_a_live_remap_shows_the_new_chord_and_not_the_old_one() {
    let mut host = seeded_host();
    host.keymap().set_label("ctrl+a z", "Ctrl+A z");
    host.keymap()
        .set_bindings(vec![binding("Panes", "New group", "ctrl+a z", false)]);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    let text = host.render(keys_panel(), 140, 30).text();
    assert!(text.contains("Ctrl+A z"));
    assert!(!text.contains("Ctrl+A g"));
}

/// REQ: F-010
#[test]
fn overlay_010_the_tree_and_mouse_hints_are_present_at_wide_widths() {
    let mut host = host_with_help_key();
    let text = host.render(keys_panel(), 140, 40).text();
    assert!(
        text.contains("Terminal history: wheel / Shift+PgUp/PgDn · Shift+End live · Ctrl+End app")
    );
}

/// REQ: F-010
#[test]
fn overlay_010_narrow_widths_use_the_short_history_line() {
    let mut host = host_with_help_key();
    let text = host.render(keys_panel(), 50, 40).text();
    assert!(text.contains("History: wheel / Shift+PgUp/PgDn"));
}

/// REQ: F-010
#[test]
fn overlay_010_a_truncated_table_says_how_many_actions_are_hidden() {
    let mut host = seeded_host();
    let bindings: Vec<_> = action_rows()
        .iter()
        .map(|row| action_binding(row, &row.tmux))
        .collect();
    host.keymap().set_bindings(bindings);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    let text = host.render(keys_panel(), 80, 14).text();
    assert!(
        text.contains("more actions — see Settings → Keyboard for the full list"),
        "{text}"
    );
}

/// REQ: F-010
#[test]
fn overlay_010_a_narrow_truncated_table_uses_the_short_notice() {
    let mut host = seeded_host();
    let bindings: Vec<_> = action_rows()
        .iter()
        .map(|row| action_binding(row, &row.tmux))
        .collect();
    host.keymap().set_bindings(bindings);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    let text = host.render(keys_panel(), 50, 14).text();
    assert!(text.contains("more: Settings → Keyboard"), "{text}");
}
