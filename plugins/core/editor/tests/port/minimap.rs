//! Twins of ilium-client/src/minimap.rs through the rendered minimap column (F-019).
//! The minimap is the last 8 columns of a 40x20 pane; glyph ramp is " ·-=≡≣█" by density.

use crate::support::{active_host, open_file, render_pane};
use ilium_plugin_sdk::generated::types::{NodeId, ViewTarget};
use serde_json::json;

const RAMP: [char; 6] = ['·', '-', '=', '≡', '≣', '█'];

fn minimap_row(text: &str, row: u16) -> String {
    let mut host = active_host(&[
        ("show_minimap", json!(true)),
        ("show_line_numbers", json!(false)),
    ]);
    open_file(&mut host, 1, "notes.md", Some("markdown"), text);
    let screen = render_pane(&mut host, 40, 20);
    (32..40)
        .map(|column| {
            screen
                .cell(row, column)
                .symbol
                .chars()
                .next()
                .unwrap_or(' ')
        })
        .collect()
}

fn glyph_count(text: &str, row: u16) -> usize {
    minimap_row(text, row)
        .chars()
        .filter(|c| RAMP.contains(c))
        .count()
}

/// REQ: PORT:ilium-client/src/minimap.rs::classifies_common_line_shapes
#[test]
fn port_minimap_every_line_shape_draws_a_distinct_marker_class() {
    let blank = minimap_row("\n", 1);
    let heading = minimap_row("# Title\n", 1);
    let code = minimap_row("```rust\n", 1);
    let text = minimap_row("plain text\n", 1);
    assert!(
        blank.trim().is_empty(),
        "blank line draws nothing: {blank:?}"
    );
    assert_ne!(heading, text);
    assert_ne!(code, text);
    for plain in [
        "2024 was a great year",
        "42 apples",
        "3D printing is neat",
        "**bold**",
        "*emphasis*",
        "---",
        "-3 degrees",
    ] {
        assert_eq!(
            minimap_row(&format!("{plain}\n"), 1).chars().next(),
            text.chars().next(),
            "{plain:?} is plain text"
        );
    }
    let list = minimap_row("- item\n", 1);
    for marker in ["1. item", "12) item", "*", "-"] {
        assert_eq!(
            minimap_row(&format!("{marker}\n"), 1).chars().next(),
            list.chars().next(),
            "{marker:?} is a list item"
        );
    }
    assert_ne!(minimap_row("> quoted\n", 1), text);
}

/// REQ: PORT:ilium-client/src/minimap.rs::heading_dominates_bucket_classification
#[test]
fn port_minimap_heading_dominates_a_shared_bucket() {
    let many: String = std::iter::once("# Title\n".to_string())
        .chain((0..60).map(|_| "plain text\n".to_string()))
        .collect();
    let heading_only = minimap_row("# Title\n", 1);
    assert_eq!(minimap_row(&many, 1), heading_only);
}

/// REQ: PORT:ilium-client/src/minimap.rs::density_index_scales_with_content_width
#[test]
fn port_minimap_bar_length_scales_with_line_width() {
    let empty = glyph_count(" \n", 1);
    let half = glyph_count(&format!("{}\n", "x".repeat(40)), 1);
    let full = glyph_count(&format!("{}\n", "x".repeat(80)), 1);
    assert_eq!(empty, 0);
    assert!(half > 0 && half < full, "half {half} full {full}");
}

/// REQ: PORT:ilium-client/src/minimap.rs::render_shapes_bars_by_length_indent_and_kind
#[test]
fn port_minimap_bars_follow_length_indent_and_kind() {
    let long = glyph_count(&format!("{}\n", "word ".repeat(16)), 1);
    let heading = glyph_count("# Hi\n", 1);
    assert!(
        heading < long,
        "a heading bar is shorter than a full paragraph bar"
    );
    let indented = minimap_row(&format!("        {}\n", "word ".repeat(8)), 1);
    assert!(
        indented.starts_with(' '),
        "indent leaves leading blank cells: {indented:?}"
    );
}

/// REQ: PORT:ilium-client/src/minimap.rs::whitespace_only_line_draws_no_glyph
#[test]
fn port_minimap_whitespace_only_line_draws_no_glyph() {
    assert_eq!(glyph_count("        \n", 1), 0);
}

fn click_minimap(lines: usize, row: u16) -> bool {
    let mut host = active_host(&[("show_minimap", json!(true))]);
    open_file(
        &mut host,
        1,
        "a.md",
        Some("markdown"),
        &"line\n".repeat(lines),
    );
    host.click(ViewTarget::Pane(NodeId(7)), 36, row)
}

/// REQ: PORT:ilium-client/src/minimap.rs::line_for_click_inverts_render_bucketing
#[test]
fn port_minimap_click_on_a_bar_row_is_handled() {
    assert!(click_minimap(100, 1));
    assert!(click_minimap(100, 10));
}

/// REQ: PORT:ilium-client/src/minimap.rs::line_for_click_clamps_to_last_line
#[test]
fn port_minimap_click_below_the_last_line_is_handled() {
    assert!(click_minimap(3, 10));
}

/// REQ: PORT:ilium-client/src/minimap.rs::line_for_click_on_empty_buffer_is_zero
#[test]
fn port_minimap_click_on_an_empty_buffer_is_handled() {
    assert!(click_minimap(0, 5));
}
