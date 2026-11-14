//! Twins of the tests of ilium-client/src/help.rs: the Help overlay is the `keys` panel. The overlay
//! drew its own close hint and cut its table to the screen; the panel is closed by the host and
//! scrolls, so those twins assert the behaviour that remains (title, prefix line, mouse and history
//! lines, width fit).

use crate::common::*;

fn help_host() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = tmux_host();
    set_keymap(
        &mut host,
        vec![
            binding(
                "Panes",
                "New terminal pane in the selected group",
                "prefix+c",
                true,
            ),
            binding("Help", "Help", "prefix+?", true),
        ],
        &[("prefix+c", "Ctrl+B c"), ("prefix+?", "?")],
    );
    host
}

fn full_host() -> ilium_plugin_sdk::testing::FakeHost {
    column_host(Column::Tmux)
}

/// REQ: PORT:ilium-client/src/help.rs::help_summary_truncation_respects_terminal_cells_and_marks_omissions
#[test]
fn help_port_summary_truncation_respects_cells_and_marks_omissions() {
    let mut host = tmux_host();
    set_keymap(
        &mut host,
        vec![
            binding("Panes", "move workspace pane", "prefix+m", true),
            binding("Panes", "界面設定", "prefix+z", true),
        ],
        &[("prefix+m", "Prefix  m"), ("prefix+z", "Prefix  z")],
    );
    for columns in [14u16, 20, 30] {
        let screen = host.render(keys_panel(), columns, 12);
        let text = screen.text();
        for line in text.lines() {
            assert!(cell_width(line) <= columns as usize, "{columns}: {line:?}");
        }
        assert!(
            text.contains('…'),
            "an omitted label is marked with an ellipsis at {columns}: {text}"
        );
    }
}

/// The overlay kept an Esc hint alive when its fixed footer could not fit. The panel has no fixed
/// footer: the host closes a panel, and the reference is one scrollable paragraph that keeps its
/// title and its prefix line at every size down to a narrow panel.
/// REQ: PORT:ilium-client/src/help.rs::help_retains_an_escape_hint_when_the_fixed_footer_cannot_fit
#[test]
fn help_port_retains_the_title_and_prefix_line_when_the_panel_is_small() {
    for (columns, rows) in [(80u16, 4u16), (80, 5), (80, 6), (80, 8), (40, 6)] {
        let mut host = full_host();
        let text = render_text(&mut host, keys_panel(), columns, rows);
        assert!(
            text.contains("Key reference") && text.contains("Prefix is Ctrl+B"),
            "missing title or prefix line at {columns}x{rows}: {text}"
        );
    }
    let mut host = full_host();
    for line in render_text(&mut host, keys_panel(), 12, 6).lines() {
        assert!(cell_width(line) <= 12, "{line:?}");
    }
}

/// REQ: PORT:ilium-client/src/help.rs::help_renders_the_current_shortcut_base_in_rows_and_close_hint
#[test]
fn help_port_renders_the_current_shortcut_base_in_rows_and_the_prefix_line() {
    let mut host = help_host();
    let text = render_text(&mut host, keys_panel(), 140, 60);
    assert!(text.contains("Ctrl+B c"), "{text}");
    assert!(
        text.contains("New terminal pane in the selected group"),
        "{text}"
    );
    assert!(text.contains("Prefix is Ctrl+B."), "{text}");
    assert!(
        !text.contains("again, or Esc, to close"),
        "the host closes the panel, the reference has no close hint: {text}"
    );
    assert!(text.contains("Shift+End live · Ctrl+End app"), "{text}");
    assert!(
        !text.contains("Ctrl+B then ?"),
        "the pre live-remap wording is gone: {text}"
    );
}

/// REQ: PORT:ilium-client/src/help.rs::help_always_shows_the_close_hint_at_the_baseline_terminal_size
#[test]
fn help_port_always_shows_the_history_and_prefix_lines_at_the_baseline_terminal_size() {
    let mut host = full_host();
    let text = render_text(&mut host, keys_panel(), 80, 24);
    assert!(text.contains("Prefix is Ctrl+B. Preset: tmux."), "{text}");
    assert!(text.contains("Shift+End live · Ctrl+End app"), "{text}");
    assert!(!text.contains("more actions"), "the panel scrolls: {text}");
}

/// REQ: PORT:ilium-client/src/help.rs::help_keeps_the_close_hint_even_on_a_tiny_terminal
#[test]
fn help_port_keeps_the_prefix_line_even_on_a_tiny_terminal() {
    for rows in 9..=14u16 {
        let mut host = full_host();
        let text = render_text(&mut host, keys_panel(), 80, rows);
        assert!(
            text.contains("Prefix is Ctrl+B"),
            "prefix line clipped at terminal height {rows}: {text}"
        );
    }
}

/// REQ: PORT:ilium-client/src/help.rs::help_renders_a_live_remapped_action_table
#[test]
fn help_port_renders_a_live_remapped_action_table() {
    let mut host = tmux_host();
    set_tab(
        &mut host,
        serde_json::json!({"shortcut_base": "ctrl+a", "navigation_shortcut_base": "ctrl+b", "preset": "custom"}),
        "shortcut_base",
    );
    set_keymap(
        &mut host,
        vec![binding("Panes", "New group", "ctrl+a z", false)],
        &[("ctrl+a z", "Ctrl+A z")],
    );
    let text = render_text(&mut host, keys_panel(), 140, 60);
    assert!(text.contains("New group"), "{text}");
    assert!(text.contains("Ctrl+A z"), "{text}");
    assert!(!text.contains("Ctrl+A g"), "{text}");
}

/// REQ: PORT:ilium-client/src/help.rs::help_summary_truncation_respects_terminal_cells_and_marks_omissions
#[test]
fn help_port_second_wide_terminal_keeps_every_line_inside_its_columns() {
    let mut host = help_host();
    let screen = host.render(keys_panel(), 120, 40);
    for line in screen.text().lines() {
        assert!(cell_width(line) <= 120, "{line:?}");
    }
}

/// REQ: PORT:ilium-client/src/help.rs::help_retains_an_escape_hint_when_the_fixed_footer_cannot_fit
#[test]
fn help_port_second_roomy_panel_shows_the_title_as_well() {
    let mut host = full_host();
    let text = render_text(&mut host, keys_panel(), 80, 30);
    assert!(text.contains("Key reference"), "{text}");
}

/// REQ: PORT:ilium-client/src/help.rs::help_renders_the_current_shortcut_base_in_rows_and_close_hint
#[test]
fn help_port_second_screen_base_is_named_in_the_prefix_line() {
    let mut host = column_host(Column::Screen);
    let text = render_text(&mut host, keys_panel(), 140, 60);
    assert!(text.contains("Prefix is Ctrl+A."), "{text}");
}

/// REQ: PORT:ilium-client/src/help.rs::help_always_shows_the_close_hint_at_the_baseline_terminal_size
#[test]
fn help_port_second_the_baseline_size_names_the_preset_with_the_prefix() {
    let mut host = help_host();
    let text = render_text(&mut host, keys_panel(), 80, 24);
    assert!(text.contains("Preset: tmux"), "{text}");
}

/// REQ: PORT:ilium-client/src/help.rs::help_keeps_the_close_hint_even_on_a_tiny_terminal
#[test]
fn help_port_second_the_prefix_line_survives_a_narrow_tiny_terminal() {
    for rows in 9..=14u16 {
        let mut host = full_host();
        let text = render_text(&mut host, keys_panel(), 40, rows);
        assert!(
            text.contains("Prefix is"),
            "prefix line clipped at 40x{rows}: {text}"
        );
    }
}

/// REQ: PORT:ilium-client/src/help.rs::help_renders_a_live_remapped_action_table
#[test]
fn help_port_second_a_remapped_base_drops_the_old_base_from_the_help() {
    let mut host = tmux_host();
    set_tab(
        &mut host,
        serde_json::json!({"shortcut_base": "ctrl+a", "navigation_shortcut_base": "ctrl+b", "preset": "custom"}),
        "shortcut_base",
    );
    let text = render_text(&mut host, keys_panel(), 140, 60);
    assert!(!text.contains("Prefix is Ctrl+B"), "{text}");
    assert!(text.contains("Prefix is Ctrl+A"), "{text}");
}
