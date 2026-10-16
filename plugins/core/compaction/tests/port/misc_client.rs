//! Twins of four client tests that touch compaction-owned surfaces (icon assignment pointer
//! steps, sidebar selection tail, compact toolbar hit testing, status bar reset watch label).
//! Gaps: `icon_assignment_click`, `icon_assignment_glyph`, `icon_assignment_suggestions`,
//! `icon_picker_open`, `sidebar_render`, `toolbar_buttons`, `toolbar_action_at`,
//! `status_bar_text` (see ../API_GAPS_port.md).

use std::time::Duration;

use crate::common::*;

/// REQ: PORT:ilium-client/src/mouse.rs::compact_assignment_pointer_steps_both_directions_and_opens_catalog
#[test]
fn pmisc_compact_assignment_pointer_steps_both_directions_and_opens_catalog() {
    let mut host = client_host();
    host.set_screen_size(40, 24);
    let suggestions = host.icon_assignment_suggestions();
    host.icon_assignment_set(&suggestions[0]);
    let last = suggestions.len() - 1;
    for (part, button, expected, opens) in [
        ("previous", "left", last, false),
        ("next", "left", 0, false),
        ("value", "left", 1, false),
        ("value", "right", 0, false),
        ("open", "left", 0, true),
    ] {
        host.icon_assignment_click(part, button);
        host.settle();
        assert_eq!(host.icon_assignment_glyph(), suggestions[expected], "{part}");
        assert_eq!(host.icon_picker_open(), opens, "{part}");
    }
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::selected_row_extends_through_its_blank_tail_in_compact_and_expanded_sidebars
#[test]
fn pmisc_selected_row_extends_through_its_blank_tail_in_compact_and_expanded_sidebars() {
    let mut host = client_host();
    host.tree().add_group("selected group");
    for width in [33u16, 65] {
        let cell = host.sidebar_last_list_cell(width, 8, "selected group");
        assert_eq!(cell.symbol, " ", "selection tail was not materialized at width {width}");
        assert_eq!(cell.background, host.accent_background(), "selection did not reach the right edge at width {width}");
    }
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::toolbar_hit_testing_maps_each_compact_icon
#[test]
fn pmisc_toolbar_hit_testing_maps_each_compact_icon() {
    let host = client_host();
    for (action, column, row) in host.toolbar_buttons(32, 12) {
        assert_eq!(host.toolbar_action_at(32, 12, column, row), Some(action));
    }
}

/// REQ: PORT:ilium-client/src/ui.rs::active_codex_reset_watch_keeps_a_compact_label_at_typical_terminal_width
#[test]
fn pmisc_active_codex_reset_watch_keeps_a_compact_label_at_typical_terminal_width() {
    let mut host = client_host();
    host.set_codex_reset_watch_expiry(Duration::from_secs(2 * 86_400 + 3_600 + 60), "human");
    let line = host.status_bar_text(58);
    assert!(line.contains("Possible Codex reset watch ends in 2d1h"), "{line}");
}
