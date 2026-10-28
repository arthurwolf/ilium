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

/// REQ: PORT:ilium-client/src/markdown/view.rs::content_height_counts_blank_line_blocks
#[test]
fn port_view_five_blank_lines_push_the_next_block_six_rows_down() {
    let screen = rendered_screen("one\n\n\n\n\n\ntwo\n");
    assert_eq!(row_of(&screen, "two") - row_of(&screen, "one"), 6, "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/markdown/view.rs::fitted_document_has_no_scroll_range
#[test]
fn port_view_short_document_starts_on_the_first_row() {
    let screen = rendered_screen("short\n");
    assert!(screen.row(0).contains("short"), "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/markdown/view.rs::line_display_changes_both_rendered_height_and_visible_text
#[test]
fn port_view_clipped_line_occupies_a_single_visible_row() {
    let mut host = plain_host(
        "n.md",
        Some("markdown"),
        "abcdef\n",
        &[
            ("markdown_rendered_by_default", json!(true)),
            ("line_display", json!("clip")),
        ],
    );
    let screen = render_pane(&mut host, 3, 6);
    let non_blank_rows = (0..screen.rows)
        .filter(|index| !screen.row(*index).trim().is_empty())
        .count();
    assert_eq!(non_blank_rows, 1, "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::plain_text_heading_avoids_the_graphics_protocol_and_remains_bold
#[test]
fn port_render_body_text_under_a_heading_is_not_bold() {
    let screen = rendered_screen("# Title\n\nbody\n");
    let (row, column) = screen.find("body").expect("body");
    assert!(!screen.cell(row, column).bold);
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::missing_local_image_falls_back_to_placeholder_text
#[test]
fn port_render_missing_image_path_is_not_shown() {
    assert!(!rendered_screen("![alt text](missing.png)\n")
        .text()
        .contains("missing.png"));
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::source_blank_line_paints_an_empty_terminal_row
#[test]
fn port_render_two_blank_source_lines_leave_two_empty_rows() {
    let screen = rendered_screen("one\n\n\ntwo\n");
    assert_eq!(row_of(&screen, "two") - row_of(&screen, "one"), 3, "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::blank_line_after_code_block_has_no_code_background
#[test]
fn port_render_code_text_has_a_different_background_from_plain_text() {
    let screen = rendered_screen("```\ncode\n```\n\ntext\n");
    let (code_row, code_column) = screen.find("code").expect("code");
    let (text_row, text_column) = screen.find("text").expect("text");
    assert_ne!(
        screen.cell(code_row, code_column).background,
        screen.cell(text_row, text_column).background
    );
}

/// REQ: PORT:ilium-client/src/markdown/view.rs::render_leaves_blank_rows_before_following_content
#[test]
fn port_view_two_blank_rows_stay_empty_before_the_following_block() {
    let screen = rendered_screen("a\n\n\nb\n");
    let first_blank = row_of(&screen, "a") as u16 + 1;
    assert!(screen.row(first_blank).trim().is_empty(), "{}", screen.text());
    assert!(screen.row(first_blank + 1).trim().is_empty(), "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/markdown/view.rs::scrolling_across_blank_rows_keeps_following_content_aligned
#[test]
fn port_view_last_block_of_a_long_document_is_not_visible_at_the_top() {
    let text: String = (0..30).map(|n| format!("line {n}\n\n")).collect();
    let mut host = plain_host(
        "n.md",
        Some("markdown"),
        &text,
        &[("markdown_rendered_by_default", json!(true))],
    );
    let top = render_pane(&mut host, 40, 8);
    assert!(top.find("line 29").is_none(), "{}", top.text());
}
