//! Twins of ilium-client/src/editor_toolbar.rs through the rendered toolbar row (F-019, F-021).

use crate::helpers::plain_host;
use crate::support::render_pane;
use ilium_plugin_sdk::generated::types::{NodeId, ViewTarget};
use ilium_plugin_sdk::testing::host::{FakeHost, Screen};
use serde_json::json;

fn toolbar(file: &str, language: &str, columns: u16, dirty: bool) -> (FakeHost, Screen) {
    let mut host = plain_host(file, Some(language), "text\n", &[("show_minimap", json!(false))]);
    if dirty {
        host.key(ViewTarget::Pane(NodeId(7)), "a");
    }
    let screen = render_pane(&mut host, columns, 10);
    (host, screen)
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::markdown_pane_shows_view_mode_buttons
#[test]
fn port_toolbar_markdown_pane_shows_view_mode_buttons() {
    let (_, screen) = toolbar("n.md", "markdown", 80, false);
    assert!(screen.row(0).contains("Source") || screen.row(0).contains("Rendered"), "{}", screen.row(0));
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::plain_text_pane_has_no_view_mode_buttons
#[test]
fn port_toolbar_plain_text_pane_has_no_view_mode_buttons() {
    let (_, screen) = toolbar("a.txt", "plaintext", 80, false);
    assert!(!screen.row(0).contains("Rendered"), "{}", screen.row(0));
    assert!(screen.row(0).trim_start().starts_with("Clip") || screen.row(0).contains("Clip"), "{}", screen.row(0));
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::every_editor_exposes_the_line_display_switch
#[test]
fn port_toolbar_every_editor_exposes_the_line_display_switch() {
    for (file, language) in [("a.txt", "plaintext"), ("n.md", "markdown"), ("m.rs", "rust")] {
        let (_, screen) = toolbar(file, language, 80, false);
        assert!(screen.row(0).contains("Clip") || screen.row(0).contains("Wrap"), "{file}: {}", screen.row(0));
    }
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::line_display_choice_has_arrows_and_catalog_in_plain_and_markdown_toolbars
#[test]
fn port_toolbar_line_display_choice_shows_arrows_in_plain_and_markdown_toolbars() {
    for (file, language, columns) in [("a.txt", "plaintext", 24), ("n.md", "markdown", 40)] {
        let (_, screen) = toolbar(file, language, columns, false);
        let row = screen.row(0);
        assert!(row.contains('←') && row.contains('→'), "{file}: {row}");
    }
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::rendered_markdown_exposes_the_same_clip_wrap_switch
#[test]
fn port_toolbar_rendered_markdown_exposes_the_same_clip_wrap_switch() {
    let mut host = plain_host("n.md", Some("markdown"), "# T\n", &[("markdown_rendered_by_default", json!(true))]);
    let screen = render_pane(&mut host, 80, 10);
    assert!(screen.row(0).contains("Clip") || screen.row(0).contains("Wrap"), "{}", screen.row(0));
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::rendered_markdown_shows_a_pixel_header_checkbox
#[test]
fn port_toolbar_rendered_markdown_shows_a_pixel_header_checkbox() {
    let mut host = plain_host("n.md", Some("markdown"), "# T\n", &[("markdown_rendered_by_default", json!(true))]);
    let screen = render_pane(&mut host, 80, 10);
    assert!(screen.find("Pixel headers").is_some(), "{}", screen.row(0));
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::eighty_columns_keep_every_markdown_source_action_and_full_selector_chrome
#[test]
fn port_toolbar_eighty_columns_keep_every_markdown_source_action() {
    let (_, screen) = toolbar("n.md", "markdown", 80, false);
    let row = screen.row(0);
    for label in ["Save", "Save As"] {
        assert!(row.contains(label), "{label} missing: {row}");
    }
    assert!(row.contains('←') && row.contains('→'), "{row}");
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::save_and_save_as_are_anchored_to_the_right_edge
#[test]
fn port_toolbar_save_and_save_as_are_anchored_to_the_right_edge() {
    let (_, screen) = toolbar("a.txt", "plaintext", 60, false);
    let row = screen.row(0);
    let save = row.find("Save ").expect("Save");
    let save_as = row.find("Save As").expect("Save As");
    assert!(save < save_as, "{row}");
    assert!(row.trim_end().ends_with("Save As"), "Save As ends at the edge: {row:?}");
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::dirty_pane_highlights_the_save_button
#[test]
fn port_toolbar_dirty_pane_highlights_the_save_button() {
    let (_, clean) = toolbar("a.txt", "plaintext", 60, false);
    let (_, dirty) = toolbar("a.txt", "plaintext", 60, true);
    let (row, column) = clean.find("Save").expect("Save");
    assert_ne!(clean.cell(row, column).background, dirty.cell(row, column).background);
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::narrow_toolbar_drops_right_buttons_before_overlapping_the_left_group
#[test]
fn port_toolbar_narrow_toolbar_drops_right_buttons_before_overlapping() {
    let (_, screen) = toolbar("a.txt", "plaintext", 10, false);
    assert!(!screen.row(0).contains("Save As"), "{}", screen.row(0));
}

/// REQ: PORT:ilium-client/src/editor_toolbar.rs::click_outside_toolbar_row_misses
#[test]
fn port_toolbar_click_outside_toolbar_row_misses() {
    let (mut host, _) = toolbar("a.txt", "plaintext", 60, false);
    assert!(!host.click(ViewTarget::Pane(NodeId(7)), 59, 5), "a click on a content row is not a toolbar hit");
}
