//! Editor pane renders: line numbers, minimap column, long lines, highlighting, cursor line
//! (core-plugin-editor.md "What the user sees", src/docs/editors-and-boards.md).

use crate::support::{active_host, count_char, nodes_where, open_file, pane_tree, render_pane, MAIN_RS};
use ilium_plugin_sdk::generated::types::Widget;
use ilium_plugin_sdk::testing::golden::assert_golden;
use ilium_plugin_sdk::testing::host::Screen;
use serde_json::json;

const GLYPHS: [char; 6] = ['\u{b7}', '-', '=', '\u{2261}', '\u{2263}', '\u{2588}'];

fn rust_screen(settings: &[(&str, serde_json::Value)], columns: u16, rows: u16) -> Screen {
    let mut host = active_host(settings);
    open_file(&mut host, 1, "src/main.rs", Some("rust"), MAIN_RS);
    render_pane(&mut host, columns, rows)
}

fn long_line_screen(settings: &[(&str, serde_json::Value)], columns: u16) -> Screen {
    let mut host = active_host(settings);
    let text = format!("{}\nshort\n", "x".repeat(50));
    open_file(&mut host, 1, "src/long.txt", None, &text);
    render_pane(&mut host, columns, 6)
}

fn minimap_glyph_count(screen: &Screen, first_column: u16) -> usize {
    let mut found = 0;
    for row in 0..screen.rows {
        for column in first_column..screen.columns {
            if GLYPHS.contains(&screen.cell(row, column).symbol.chars().next().unwrap_or(' ')) {
                found += 1;
            }
        }
    }
    found
}

/// REQ: F-019, F-020
#[test]
fn f_019_line_numbers_are_one_based_and_precede_the_text() {
    let screen = rust_screen(&[("show_minimap", json!(false))], 48, 8);
    assert!(screen.row(0).trim_start().starts_with("1 use std::fs;"), "{:?}", screen.row(0));
    assert!(screen.row(2).trim_start().starts_with("3 fn main() {"));
    assert!(screen.row(3).trim_start().starts_with("4     let n = 42;"));
    assert!(screen.row(5).trim_start().starts_with("6 }"));
}

/// REQ: F-019
#[test]
fn f_019_gutter_width_is_digit_count_plus_two() {
    let mut host = active_host(&[("show_minimap", json!(false))]);
    let text = (1..=10).map(|n| format!("line{n}")).collect::<Vec<_>>().join("\n");
    open_file(&mut host, 1, "src/ten.txt", None, &text);
    let screen = render_pane(&mut host, 40, 12);
    assert!(screen.row(0).starts_with("  1 line1"), "{:?}", screen.row(0));
    assert!(screen.row(9).starts_with(" 10 line10"), "{:?}", screen.row(9));
}

/// REQ: F-019
#[test]
fn f_019_numbers_are_dropped_when_line_numbers_are_off() {
    let screen = rust_screen(&[("show_line_numbers", json!(false)), ("show_minimap", json!(false))], 48, 8);
    assert!(screen.row(0).starts_with("use std::fs;"), "{:?}", screen.row(0));
}

/// REQ: F-019
#[test]
fn f_019_minimap_column_is_eight_cells_at_the_right_edge() {
    let screen = rust_screen(&[], 40, 8);
    assert!(minimap_glyph_count(&screen, 32) > 0, "minimap glyphs in the last eight columns");
    assert_eq!(screen.cell(0, 31).symbol, " ", "the text area ends where the minimap column begins");
}

/// REQ: F-019
#[test]
fn f_019_no_minimap_glyphs_when_the_minimap_is_off() {
    let screen = rust_screen(&[("show_minimap", json!(false))], 40, 8);
    assert_eq!(minimap_glyph_count(&screen, 32), 0);
}

/// REQ: F-019
#[test]
fn f_019_minimap_is_dropped_when_the_pane_is_too_narrow() {
    let screen = rust_screen(&[], 8, 8);
    assert_eq!(screen.row(0), " 1 use s");
}

/// REQ: F-019
#[test]
fn f_019_minimap_takes_eight_columns_from_the_text_area() {
    let with_minimap = long_line_screen(&[("show_minimap", json!(true))], 40);
    let without_minimap = long_line_screen(&[("show_minimap", json!(false))], 40);
    assert_eq!(count_char(&with_minimap.row(0), 'x'), 29);
    assert_eq!(count_char(&without_minimap.row(0), 'x'), 37);
}

/// REQ: F-019
#[test]
fn f_019_clip_mode_cuts_long_lines_at_the_edge() {
    let screen = long_line_screen(&[("line_display", json!("clip")), ("show_minimap", json!(false))], 40);
    assert_eq!(count_char(&screen.row(0), 'x'), 37);
    assert!(screen.row(1).trim_start().starts_with("2 short"), "{:?}", screen.row(1));
}

/// REQ: F-019
#[test]
fn f_019_wrap_mode_continues_long_lines_on_the_next_row() {
    let screen = long_line_screen(&[("line_display", json!("wrap")), ("show_minimap", json!(false))], 40);
    assert_eq!(count_char(&screen.row(0), 'x'), 37);
    assert_eq!(count_char(&screen.row(1), 'x'), 13);
    assert!(screen.row(2).trim_start().starts_with("2 short"), "{:?}", screen.row(2));
}

/// REQ: F-019
#[test]
fn f_019_missing_file_opens_as_an_empty_buffer() {
    let mut host = active_host(&[("show_minimap", json!(false))]);
    host.deliver_one(ilium_plugin_sdk::generated::types::Event::EditorOpened(crate::support::buffer_ref(
        1,
        Some("src/new.rs"),
        Some("rust"),
        1,
    )));
    let screen = render_pane(&mut host, 40, 4);
    assert_eq!(screen.row(0).trim(), "1");
    assert_eq!(screen.row(1), "");
}

/// REQ: F-019
#[test]
fn f_019_current_line_is_underlined_and_other_lines_are_not() {
    let screen = rust_screen(&[("show_minimap", json!(false))], 48, 8);
    assert!(screen.cell(0, 5).underline, "cursor line");
    assert!(!screen.cell(2, 5).underline, "other line");
}

/// REQ: F-020
#[test]
fn f_020_keyword_and_string_cells_have_different_colours() {
    let mut host = active_host(&[("show_minimap", json!(false))]);
    open_file(&mut host, 1, "src/s.rs", Some("rust"), "fn main() { let s = \"hi\"; }");
    let screen = render_pane(&mut host, 48, 4);
    let keyword = screen.cell(0, 3).foreground;
    let string = screen.cell(0, 23).foreground;
    assert_ne!(keyword, string);
}

/// REQ: F-020
#[test]
fn f_020_unrecognized_language_draws_every_text_cell_in_one_colour() {
    let mut host = active_host(&[("show_minimap", json!(false))]);
    open_file(&mut host, 1, "notes.unknown", None, "fn main() { let s = \"hi\"; }");
    let screen = render_pane(&mut host, 48, 4);
    assert_eq!(screen.cell(0, 3).foreground, screen.cell(0, 23).foreground);
    assert_eq!(screen.cell(0, 3).foreground, screen.cell(0, 8).foreground);
}

/// REQ: F-019, F-020
#[test]
fn f_019_golden_rust_pane_at_40_columns() {
    let screen = rust_screen(&[("show_minimap", json!(false))], 40, 6);
    assert_golden("tests/features/golden/editor_main_rs_40.txt", &format!("{}\n", screen.text()));
}

/// REQ: F-019, F-020
#[test]
fn f_019_golden_rust_pane_at_80_columns() {
    let screen = rust_screen(&[("show_minimap", json!(false))], 80, 6);
    assert_golden("tests/features/golden/editor_main_rs_80.txt", &format!("{}\n", screen.text()));
}

/// REQ: F-019, F-020
#[test]
fn f_019_golden_rust_pane_at_120_columns() {
    let screen = rust_screen(&[("show_minimap", json!(false))], 120, 6);
    assert_golden("tests/features/golden/editor_main_rs_120.txt", &format!("{}\n", screen.text()));
}

/// REQ: F-019
#[test]
fn f_019_code_view_flags_follow_the_settings() {
    let mut editor = crate::support::direct(
        &json!({"show_line_numbers": false, "show_minimap": false, "line_display": "wrap"}).to_string(),
    );
    crate::support::direct_open(&mut editor, 1, Some("rust"), 6);
    let tree = pane_tree(&mut editor, 48, 8);
    let views = nodes_where(&tree, |widget| matches!(widget, Widget::CodeView(_)));
    assert_eq!(views.len(), 1);
    if let Widget::CodeView(spec) = &views[0].widget {
        assert!(!spec.line_numbers);
        assert!(!spec.minimap);
        assert!(spec.wrap);
        assert!(!spec.read_only);
    }
}

/// REQ: F-019
#[test]
fn f_019_code_view_defaults_are_numbers_on_minimap_on_clip() {
    let mut editor = crate::support::direct("{}");
    crate::support::direct_open(&mut editor, 1, Some("rust"), 6);
    let tree = pane_tree(&mut editor, 48, 8);
    let views = nodes_where(&tree, |widget| matches!(widget, Widget::CodeView(_)));
    assert_eq!(views.len(), 1);
    if let Widget::CodeView(spec) = &views[0].widget {
        assert!(spec.line_numbers);
        assert!(spec.minimap);
        assert!(!spec.wrap);
    }
}
