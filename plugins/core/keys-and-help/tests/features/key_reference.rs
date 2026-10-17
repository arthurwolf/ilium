//! Sample 1 and the first figure: the Key reference panel (`keys`) draws every binding as a
//! `key-hints` pair grouped by area, rebuilds on `keymap-changed`, and shows the prefix and
//! the active preset on its last line.

use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::KernelSettingsEvent;
use serde_json::json;

use crate::common::*;

fn keys_panel() -> ViewTarget {
    ViewTarget::Panel("keys".to_string())
}

fn render_text(host: &mut ilium_plugin_sdk::testing::FakeHost, columns: u16, rows: u16) -> String {
    host.render(keys_panel(), columns, rows).text()
}

fn ready_host() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = seeded_host();
    seed_sample_labels(&mut host);
    host
}

/// REQ: F-010
#[test]
fn reference_010_the_panel_has_the_key_reference_title() {
    let mut host = ready_host();
    assert!(render_text(&mut host, 120, 14).contains("Key reference"));
}

/// REQ: F-010
#[test]
fn reference_010_the_panel_groups_bindings_under_their_area_headings_in_order() {
    let mut host = ready_host();
    let text = render_text(&mut host, 120, 14);
    let panes = text.find("Panes").expect("Panes heading");
    let split = text.find("Split vertically").expect("first binding");
    let tree = text.find("Tree").expect("Tree heading");
    let rename = text.find("Rename").expect("last binding");
    assert!(panes < split && split < tree && tree < rename, "{text}");
}

/// REQ: F-010
#[test]
fn reference_010_each_binding_is_a_label_leader_and_chord_label_row() {
    let mut host = ready_host();
    let screen = host.render(keys_panel(), 120, 14);
    let (row, _) = screen.find("Split vertically").expect("binding row");
    let line = screen.row(row);
    assert!(line.contains("Prefix  %"), "{line}");
    let label_end = line.find("Split vertically").unwrap() + "Split vertically".len();
    let chord_start = line.find("Prefix  %").unwrap();
    let between = &line[label_end..chord_start];
    assert!(
        between.starts_with(' ') && between.ends_with(' '),
        "one blank cell each side: {between:?}"
    );
    assert!(
        between.trim().chars().all(|character| character == '…'),
        "{between:?}"
    );
    assert!(between.trim().chars().count() >= 5, "{between:?}");
}

/// REQ: F-010
#[test]
fn reference_010_the_second_pane_binding_shows_its_own_chord_label() {
    let mut host = ready_host();
    let screen = host.render(keys_panel(), 120, 14);
    let (row, _) = screen.find("Split horizontally").expect("binding row");
    assert!(screen.row(row).contains("Prefix  \""));
}

/// REQ: F-010
#[test]
fn reference_010_tree_bindings_show_the_arrow_and_letter_labels() {
    let mut host = ready_host();
    let screen = host.render(keys_panel(), 120, 14);
    let (up_row, _) = screen.find("Move up").expect("Move up");
    let (rename_row, _) = screen.find("Rename").expect("Rename");
    assert!(screen.row(up_row).contains("Prefix  Up"));
    assert!(screen.row(rename_row).contains("Prefix  r"));
}

/// REQ: F-007, F-010
#[test]
fn reference_007_the_last_line_names_the_prefix_and_the_preset() {
    let mut host = ready_host();
    assert!(render_text(&mut host, 120, 14).contains("Prefix is Ctrl+B. Preset: tmux."));
}

/// REQ: F-007, F-010
#[test]
fn reference_007_the_screen_preset_is_labelled_gnu_screen_with_the_ctrl_a_prefix() {
    let mut host = ready_host();
    host.kernel_settings().set_tab(
        "keyboard",
        json!({"shortcut_base": "ctrl+a", "navigation_shortcut_base": "ctrl+b", "preset": "screen"}),
    );
    host.deliver_one(Event::KernelSettingsChanged(KernelSettingsEvent {
        tab: "keyboard".to_string(),
        keys: vec!["shortcut_base".to_string(), "preset".to_string()],
    }));
    host.settle();
    assert!(render_text(&mut host, 120, 14).contains("Prefix is Ctrl+A. Preset: GNU Screen."));
}

/// REQ: F-007, F-010
#[test]
fn reference_007_a_custom_prefix_letter_is_shown_in_upper_case() {
    let mut host = ready_host();
    host.kernel_settings().set_tab(
        "keyboard",
        json!({"shortcut_base": "ctrl+g", "navigation_shortcut_base": "ctrl+b", "preset": "custom"}),
    );
    host.deliver_one(Event::KernelSettingsChanged(KernelSettingsEvent {
        tab: "keyboard".to_string(),
        keys: vec!["shortcut_base".to_string()],
    }));
    host.settle();
    assert!(render_text(&mut host, 120, 14).contains("Prefix is Ctrl+G."));
}

/// REQ: F-010
#[test]
fn reference_010_a_remap_appears_after_keymap_changed() {
    let mut host = ready_host();
    host.keymap().set_label("prefix+z", "Prefix  z");
    host.keymap().set_bindings(vec![binding(
        "Panes",
        "Split vertically",
        "prefix+z",
        false,
    )]);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    let text = render_text(&mut host, 120, 14);
    assert!(text.contains("Prefix  z"), "{text}");
    assert!(!text.contains("Prefix  %"), "{text}");
}

/// REQ: F-010
#[test]
fn reference_010_keymap_changed_invalidates_exactly_the_keys_panel() {
    let mut host = ready_host();
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    assert_eq!(host.invalidated(), vec![keys_panel()]);
}

/// REQ: F-010
#[test]
fn reference_010_another_event_does_not_invalidate_the_panel() {
    let mut host = ready_host();
    host.deliver_one(Event::ThemeChanged);
    host.settle();
    assert!(host.invalidated().is_empty());
}

/// REQ: F-010
#[test]
fn reference_010_two_keymap_changed_events_in_one_batch_are_a_single_redraw() {
    let mut host = ready_host();
    host.deliver(vec![Event::KeymapChanged, Event::KeymapChanged]);
    host.settle();
    assert_eq!(host.invalidated().len(), 1);
}

/// REQ: F-010
#[test]
fn reference_010_an_empty_keymap_renders_the_title_without_binding_rows() {
    let mut host = ready_host();
    host.keymap().set_bindings(Vec::new());
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    let text = render_text(&mut host, 80, 10);
    assert!(text.contains("Key reference"));
    assert!(!text.contains("Split vertically"));
}

/// REQ: F-010
#[test]
fn reference_010_a_binding_without_a_chord_is_left_out() {
    let mut host = ready_host();
    let mut unbound = binding("Tree", "Rename", "prefix+r", false);
    unbound.chords.clear();
    host.keymap().set_bindings(vec![
        unbound,
        binding("Panes", "Split vertically", "prefix+%", true),
    ]);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    let text = render_text(&mut host, 80, 10);
    assert!(!text.contains("Rename"), "{text}");
    assert!(text.contains("Split vertically"));
}

/// REQ: F-010
#[test]
fn reference_010_only_the_first_chord_of_a_binding_is_shown() {
    let mut host = ready_host();
    let mut two = binding("Panes", "Split vertically", "prefix+%", true);
    two.chords.push("prefix+5".to_string());
    host.keymap().set_label("prefix+5", "Prefix  5");
    host.keymap().set_bindings(vec![two]);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    let text = render_text(&mut host, 80, 10);
    assert!(text.contains("Prefix  %"));
    assert!(!text.contains("Prefix  5"));
}

/// REQ: F-010
#[test]
fn reference_010_wide_render_at_120_columns_fits_every_row_and_the_footer() {
    let mut host = ready_host();
    let screen = host.render(keys_panel(), 120, 14);
    assert_eq!((screen.columns, screen.rows), (120, 14));
    assert!(screen.text().contains("Prefix is Ctrl+B. Preset: tmux."));
}

/// REQ: F-010
#[test]
fn reference_010_render_at_80_columns_keeps_labels_and_chords_visible() {
    let mut host = ready_host();
    let text = render_text(&mut host, 80, 14);
    assert!(text.contains("Split horizontally"));
    assert!(text.contains("Prefix  \""));
    assert!(text.contains("Prefix is Ctrl+B. Preset: tmux."));
}

/// REQ: F-010
#[test]
fn reference_010_render_at_40_columns_keeps_label_and_chord_on_one_row() {
    let mut host = ready_host();
    let screen = host.render(keys_panel(), 40, 14);
    let (row, _) = screen.find("Split vertically").expect("label");
    let line = screen.row(row);
    assert!(line.contains("Prefix  %"), "{line}");
    assert!(line.chars().count() <= 40);
}

/// REQ: F-010
#[test]
fn reference_010_a_gap_shorter_than_five_cells_stays_blank_without_dots() {
    let mut host = ready_host();
    host.keymap().set_label("prefix+%", "Prefix  %");
    host.keymap()
        .set_bindings(vec![binding("Panes", "Split vertically", "prefix+%", true)]);
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    // 16 label cells, 9 chord cells and two borders leave a 3-cell gap at 30 columns.
    let screen = host.render(keys_panel(), 30, 8);
    let (row, _) = screen.find("Split vertically").expect("label");
    assert!(!screen.row(row).contains('…'), "{}", screen.row(row));
}

/// REQ: F-010
#[test]
fn reference_010_the_command_palette_entry_show_keys_opens_this_panel() {
    let mut host = ready_host();
    let submissions = host.deliver_one(Event::CommandInvoked(
        ilium_plugin_sdk::types::events::CommandInvokedEvent {
            command: format!("{PLUGIN_ID}/show-keys"),
            arguments_json: "{}".to_string(),
            origin: ilium_plugin_sdk::types::events::CommandOrigin::Keybinding,
            context: Default::default(),
        },
    ));
    let opened: Vec<&ViewTarget> = submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::OpenView(spec) => Some(&spec.view),
            _ => None,
        })
        .collect();
    assert_eq!(opened, vec![&keys_panel()]);
}
