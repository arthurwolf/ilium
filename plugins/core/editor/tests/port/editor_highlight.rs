//! Twins of ilium-client/src/editor_highlight.rs (and its window_* variants): rendered syntax
//! colours, gutter, cursor line and wrapping (F-019, F-020).

use crate::helpers::plain_host;
use crate::support::render_pane;
use serde_json::json;

const RUST_LINE: &str = "fn main() { let s = \"hi\"; }";

fn host_for(
    text: &str,
    language: Option<&str>,
    extra: &[(&str, serde_json::Value)],
) -> ilium_plugin_sdk::testing::host::FakeHost {
    plain_host("sample.rs", language, text, extra)
}

fn colour_at(
    text: &str,
    extra: &[(&str, serde_json::Value)],
    row: u16,
    needle: &str,
) -> ilium_plugin_sdk::testing::host::ScreenCell {
    let mut host = host_for(text, Some("rust"), extra);
    let screen = render_pane(&mut host, 64, 8);
    let found = screen.find(needle).expect("needle on screen");
    let _ = row;
    screen.cell(found.0, found.1)
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::keyword_and_string_literal_get_different_colors, PORT:ilium-client/src/editor_highlight.rs::window_keyword_and_string_literal_get_different_colors
#[test]
fn port_highlight_keyword_and_string_literal_get_different_colors() {
    let no_numbers = [("show_line_numbers", json!(false))];
    let keyword = colour_at(RUST_LINE, &no_numbers, 0, "fn");
    let string = colour_at(RUST_LINE, &no_numbers, 0, "\"hi\"");
    assert_ne!(keyword.foreground, string.foreground);
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::line_number_gutter_shows_one_based_row_numbers, PORT:ilium-client/src/editor_highlight.rs::window_line_number_gutter_shows_one_based_row_numbers
#[test]
fn port_highlight_line_number_gutter_shows_one_based_row_numbers() {
    let mut host = host_for(
        "a\nb\nc\n",
        Some("rust"),
        &[("show_line_numbers", json!(true))],
    );
    let screen = render_pane(&mut host, 40, 6);
    for (row, digit) in ["1", "2", "3"].iter().enumerate() {
        assert_eq!(
            screen.cell(row as u16, 1).symbol,
            *digit,
            "row {row}: {}",
            screen.row(row as u16)
        );
    }
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::cursor_cell_is_reversed_and_current_line_is_underlined, PORT:ilium-client/src/editor_highlight.rs::window_cursor_cell_is_reversed_and_current_line_is_underlined
#[test]
fn port_highlight_current_line_is_underlined_and_other_rows_are_not() {
    let mut host = host_for(
        "one\ntwo\n",
        Some("rust"),
        &[("show_line_numbers", json!(false))],
    );
    let screen = render_pane(&mut host, 40, 6);
    assert!(
        screen.cell(0, 0).underline,
        "cursor line must be underlined"
    );
    assert!(
        !screen.cell(1, 0).underline,
        "other rows must not be underlined"
    );
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::unrecognized_language_leaves_highlighted_lines_empty
#[test]
fn port_highlight_unrecognized_language_draws_one_colour() {
    let mut host = host_for(
        RUST_LINE,
        Some("no-such-language"),
        &[("show_line_numbers", json!(false))],
    );
    let screen = render_pane(&mut host, 64, 4);
    let keyword = screen.find("fn").expect("text");
    let string = screen.find("\"hi\"").expect("text");
    assert_eq!(
        screen.cell(keyword.0, keyword.1).foreground,
        screen.cell(string.0, string.1).foreground
    );
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::wrapping_keeps_syntax_highlighting_on_every_visual_row, PORT:ilium-client/src/editor_highlight.rs::window_wrapping_keeps_syntax_highlighting_on_every_visual_row
#[test]
fn port_highlight_wrapping_keeps_syntax_colours_on_every_visual_row() {
    let extra = [
        ("show_line_numbers", json!(false)),
        ("line_display", json!("wrap")),
    ];
    let mut host = host_for("let greeting = \"hello\";", Some("rust"), &extra);
    let screen = render_pane(&mut host, 8, 6);
    assert_eq!(screen.row(0).trim_end(), "let gree");
    assert!(screen.row(1).starts_with("ting = \""), "{}", screen.row(1));
    let quote = screen.cell(1, 7);
    let keyword = screen.cell(0, 0);
    assert_ne!(
        quote.foreground, keyword.foreground,
        "wrapped string keeps its colour"
    );
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::wrapped_continuation_row_expands_a_tab_relative_to_its_own_start, PORT:ilium-client/src/editor_highlight.rs::window_wrapped_continuation_expands_tab_from_its_own_start
#[test]
fn port_highlight_wrapped_continuation_expands_a_tab_from_its_own_start() {
    let extra = [
        ("show_line_numbers", json!(false)),
        ("line_display", json!("wrap")),
    ];
    let mut host = host_for("//abcdefgh12\tXYZ", Some("rust"), &extra);
    let screen = render_pane(&mut host, 10, 6);
    assert_eq!(
        screen.row(1),
        "12  XYZ   ",
        "second visual row: {:?}",
        screen.row(1)
    );
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::window_multiline_comment_keeps_sequential_syntax_across_physical_lines
#[test]
fn port_highlight_multiline_comment_keeps_sequential_syntax_across_lines() {
    let text = "/* opening\ninside block comment\n*/ fn main() { let s = \"hi\"; }";
    let mut host = host_for(text, Some("rust"), &[("show_line_numbers", json!(false))]);
    let screen = render_pane(&mut host, 64, 6);
    assert!(
        screen.row(1).starts_with("inside block comment"),
        "{}",
        screen.row(1)
    );
    assert_eq!(
        screen.cell(1, 0).foreground,
        screen.cell(0, 0).foreground,
        "comment colour continues"
    );
    let keyword = screen.cell(2, 3);
    assert_ne!(
        keyword.foreground,
        screen.cell(0, 0).foreground,
        "fn after the close regains the keyword colour"
    );
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::keyword_and_string_literal_get_different_colors, PORT:ilium-client/src/editor_highlight.rs::window_keyword_and_string_literal_get_different_colors
#[test]
fn port_highlight_two_keywords_share_one_colour() {
    let no_numbers = [("show_line_numbers", json!(false))];
    let first = colour_at(RUST_LINE, &no_numbers, 0, "fn");
    let second = colour_at(RUST_LINE, &no_numbers, 0, "let");
    assert_eq!(first.foreground, second.foreground);
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::line_number_gutter_shows_one_based_row_numbers, PORT:ilium-client/src/editor_highlight.rs::window_line_number_gutter_shows_one_based_row_numbers
#[test]
fn port_highlight_gutter_shows_two_digit_row_ten() {
    let text = "x\n".repeat(10);
    let mut host = host_for(&text, Some("rust"), &[("show_line_numbers", json!(true))]);
    let screen = render_pane(&mut host, 40, 12);
    assert!(screen.row(9).contains("10"), "{}", screen.row(9));
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::cursor_cell_is_reversed_and_current_line_is_underlined, PORT:ilium-client/src/editor_highlight.rs::window_cursor_cell_is_reversed_and_current_line_is_underlined
#[test]
fn port_highlight_underline_covers_the_whole_cursor_row() {
    let mut host = host_for(
        "one\ntwo\n",
        Some("rust"),
        &[("show_line_numbers", json!(false))],
    );
    let screen = render_pane(&mut host, 40, 6);
    assert!(
        screen.cell(0, 20).underline,
        "empty cells of the cursor row must be underlined"
    );
    assert!(
        !screen.cell(1, 20).underline,
        "other rows must not be underlined"
    );
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::wrapping_keeps_syntax_highlighting_on_every_visual_row, PORT:ilium-client/src/editor_highlight.rs::window_wrapping_keeps_syntax_highlighting_on_every_visual_row
#[test]
fn port_highlight_string_colour_continues_onto_a_third_visual_row() {
    let extra = [
        ("show_line_numbers", json!(false)),
        ("line_display", json!("wrap")),
    ];
    let mut host = host_for("let greeting = \"hello\";", Some("rust"), &extra);
    let screen = render_pane(&mut host, 8, 6);
    assert!(screen.row(2).starts_with("hello"), "{}", screen.row(2));
    assert_eq!(
        screen.cell(2, 0).foreground,
        screen.cell(1, 7).foreground,
        "the string continues in the same colour"
    );
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::wrapped_continuation_row_expands_a_tab_relative_to_its_own_start, PORT:ilium-client/src/editor_highlight.rs::window_wrapped_continuation_expands_tab_from_its_own_start
#[test]
fn port_highlight_first_wrapped_row_holds_the_unbroken_prefix() {
    let extra = [
        ("show_line_numbers", json!(false)),
        ("line_display", json!("wrap")),
    ];
    let mut host = host_for("//abcdefgh12\tXYZ", Some("rust"), &extra);
    let screen = render_pane(&mut host, 10, 6);
    assert_eq!(screen.row(0), "//abcdefgh", "first visual row");
}

/// REQ: PORT:ilium-client/src/editor_highlight.rs::window_multiline_comment_keeps_sequential_syntax_across_physical_lines
#[test]
fn port_highlight_closing_delimiter_row_keeps_the_comment_colour() {
    let text = "/* opening\ninside block comment\n*/ fn main() { let s = \"hi\"; }";
    let mut host = host_for(text, Some("rust"), &[("show_line_numbers", json!(false))]);
    let screen = render_pane(&mut host, 64, 6);
    assert_eq!(
        screen.cell(2, 0).foreground,
        screen.cell(0, 0).foreground,
        "the closing delimiter is comment text"
    );
}
