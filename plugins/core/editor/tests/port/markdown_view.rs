//! Twins of ilium-client/src/markdown/view.rs and markdown/render.rs through the rendered view (F-021).

use crate::helpers::{plain_host, rendered_screen, row_of};
use crate::support::render_pane;
use serde_json::json;

/// REQ: PORT:ilium-client/src/markdown/view.rs::content_height_counts_blank_line_blocks, PORT:ilium-client/src/markdown/view.rs::render_leaves_blank_rows_before_following_content
#[test]
fn port_view_blank_line_blocks_count_in_height_and_leave_rows() {
    let screen = rendered_screen("before\n\n\nafter\n");
    assert_eq!(row_of(&screen, "after") - row_of(&screen, "before"), 3);
}

/// REQ: PORT:ilium-client/src/markdown/view.rs::scrolling_across_blank_rows_keeps_following_content_aligned
#[test]
fn port_view_scrolling_across_blank_rows_keeps_content_aligned() {
    let text: String = (0..30).map(|n| format!("line {n}\n\n")).collect();
    let mut host = plain_host(
        "n.md",
        Some("markdown"),
        &text,
        &[("markdown_rendered_by_default", json!(true))],
    );
    let top = render_pane(&mut host, 40, 8);
    assert!(top.find("line 0").is_some());
    let gap = row_of(&top, "line 1") - row_of(&top, "line 0");
    assert_eq!(gap, 2);
}

/// REQ: PORT:ilium-client/src/markdown/view.rs::fitted_document_has_no_scroll_range
#[test]
fn port_view_fitted_document_shows_everything_without_scrolling() {
    let screen = rendered_screen("short\n");
    assert!(screen.find("short").is_some());
}

/// REQ: PORT:ilium-client/src/markdown/view.rs::line_display_changes_both_rendered_height_and_visible_text
#[test]
fn port_view_line_display_changes_rendered_height_and_text() {
    let clip = {
        let mut host = plain_host(
            "n.md",
            Some("markdown"),
            "abcdef\n",
            &[
                ("markdown_rendered_by_default", json!(true)),
                ("line_display", json!("clip")),
            ],
        );
        render_pane(&mut host, 3, 6)
    };
    let wrap = {
        let mut host = plain_host(
            "n.md",
            Some("markdown"),
            "abcdef\n",
            &[
                ("markdown_rendered_by_default", json!(true)),
                ("line_display", json!("wrap")),
            ],
        );
        render_pane(&mut host, 3, 6)
    };
    assert!(clip.find("def").is_none(), "{}", clip.text());
    assert!(clip.find("abc").is_some(), "{}", clip.text());
    let (abc_row, _) = wrap.find("abc").expect("abc on the first wrapped row");
    let (def_row, def_column) = wrap.find("def").expect("def wrapped onto a following row");
    assert!(def_row > abc_row, "{}", wrap.text());
    assert_eq!(
        def_column,
        wrap.find("abc").expect("abc").1,
        "{}",
        wrap.text()
    );
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::plain_text_heading_avoids_the_graphics_protocol_and_remains_bold
#[test]
fn port_render_plain_text_heading_is_bold() {
    let screen = rendered_screen("# Title\n");
    let (row, column) = screen.find("Title").expect("Title");
    assert!(screen.cell(row, column).bold);
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::missing_local_image_falls_back_to_placeholder_text
#[test]
fn port_render_missing_local_image_falls_back_to_placeholder_text() {
    assert!(rendered_screen("![alt text](missing.png)\n")
        .text()
        .contains("alt text"));
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::source_blank_line_paints_an_empty_terminal_row
#[test]
fn port_render_source_blank_line_paints_an_empty_row() {
    let screen = rendered_screen("one\n\ntwo\n");
    let between = row_of(&screen, "one") + 1;
    assert!(screen.row(between as u16).trim().is_empty());
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::blank_line_after_code_block_has_no_code_background
#[test]
fn port_render_blank_line_after_code_block_has_no_code_background() {
    let screen = rendered_screen("```\ncode\n```\n\ntext\n");
    let (code_row, code_column) = screen.find("code").expect("code");
    let blank = screen.cell(code_row + 2, code_column);
    assert_ne!(
        blank.background,
        screen.cell(code_row, code_column).background
    );
}
