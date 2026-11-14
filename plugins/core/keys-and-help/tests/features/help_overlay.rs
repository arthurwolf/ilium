//! Behaviour the plugin's `keys` panel keeps from the kernel Help screen it replaced: the mouse
//! and history lines, a live remap that shows at once, a footer that follows the prefix, and a
//! table that scrolls instead of being cut. What the screen had for itself and the panel does not
//! need is gone with it: the host closes a panel, so the reference carries no close hint, and a
//! scrollable paragraph never has to say how many actions it left out.

use ilium_plugin_core_keys_and_help::key_reference::{command_label, TITLE};
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

fn host_with_every_action() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = seeded_host();
    let bindings: Vec<_> = action_rows()
        .iter()
        .map(|row| action_binding(row, &row.tmux))
        .collect();
    host.keymap().set_bindings(bindings);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    host
}

/// REQ: F-010
#[test]
fn overlay_010_the_panel_carries_no_close_hint_because_the_host_closes_it() {
    let mut host = host_with_help_key();
    for (columns, rows) in [(80u16, 24u16), (40, 24), (12, 6)] {
        let text = host.render(keys_panel(), columns, rows).text();
        assert!(!text.contains("to close"), "{columns}x{rows}: {text}");
        assert!(!text.contains("Esc closes"), "{columns}x{rows}: {text}");
    }
}

/// REQ: F-010
#[test]
fn overlay_010_the_footer_follows_a_changed_prefix() {
    let mut host = host_with_help_key();
    assert!(host
        .render(keys_panel(), 80, 24)
        .text()
        .contains("Prefix is Ctrl+B. Preset: tmux."));
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
    let text = host.render(keys_panel(), 80, 24).text();
    assert!(
        text.contains("Prefix is Ctrl+A. Preset: GNU Screen."),
        "{text}"
    );
    assert!(!text.contains("Prefix is Ctrl+B"), "{text}");
}

/// REQ: F-010
#[test]
fn overlay_010_a_remapped_help_key_shows_after_the_prefix_label() {
    let mut host = seeded_host();
    host.keymap().set_label("h", "h");
    host.keymap()
        .set_bindings(vec![binding("prefix", "Help", "h", false)]);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    let text = host.render(keys_panel(), 80, 24).text();
    assert!(text.contains("Prefix  h"), "{text}");
    assert!(!text.contains("Prefix  ?"), "{text}");
}

/// REQ: F-010
#[test]
fn overlay_010_every_line_fits_the_width_down_to_the_narrowest_panel() {
    let mut host = host_with_every_action();
    for columns in [20u16, 39, 40, 59, 62, 80, 140] {
        let text = host.render(keys_panel(), columns, 24).text();
        for line in text.lines() {
            assert!(
                unicode_width::UnicodeWidthStr::width(line) <= usize::from(columns),
                "{columns}: {line:?}"
            );
        }
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
    assert!(text.contains("Mouse: pane focus · tree expand/reorder · context menus"));
    assert!(
        text.contains("Terminal history: wheel / Shift+PgUp/PgDn · Shift+End live · Ctrl+End app")
    );
}

/// REQ: F-010
#[test]
fn overlay_010_narrow_widths_use_the_short_mouse_and_history_lines() {
    let mut host = host_with_help_key();
    let text = host.render(keys_panel(), 50, 40).text();
    assert!(text.contains("Mouse: focus · menus · tree reorder"));
    assert!(text.contains("History: wheel / Shift+PgUp/PgDn"));
    assert!(!text.contains("Shift+End live"));
}

/// REQ: F-010
#[test]
fn overlay_010_a_long_table_is_complete_and_scrolls_instead_of_being_cut() {
    let mut host = host_with_every_action();
    // Far fewer rows than the 35 actions: the panel still holds every one of them.
    let text = host.render(keys_panel(), 80, 14).text();
    assert!(!text.contains("more actions"), "{text}");
    let rows = action_rows();
    let shown = rows
        .iter()
        .filter(|row| row.tmux != "unbound")
        .filter(|row| text.contains(&command_label(&row.command)))
        .count();
    assert_eq!(
        shown,
        rows.iter().filter(|row| row.tmux != "unbound").count()
    );
    assert!(text.contains(TITLE), "{text}");
}
