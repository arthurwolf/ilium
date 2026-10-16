//! The Clock sample scene with glyph plane numerals (page sample 2, F-096).
use crate::common::*;

fn clock_screen(columns: u16, rows: u16) -> ilium_plugin_sdk::testing::Screen {
    fake_scene("clock", columns, rows).frame(braille_frame(columns, rows, 0))
}

/// REQ: F-096
#[test]
fn clock_096_twelve_sits_at_the_top_centre() {
    let screen = clock_screen(40, 20);
    assert_eq!(screen.cell(0, 40 / 2 - 1).symbol, "12");
}

/// REQ: F-096
#[test]
fn clock_096_three_six_nine_sit_at_the_documented_cells() {
    let screen = clock_screen(40, 20);
    assert_eq!(screen.cell(20 / 2, 40 - 2).symbol, "3");
    assert_eq!(screen.cell(20 - 1, 40 / 2).symbol, "6");
    assert_eq!(screen.cell(20 / 2, 1).symbol, "9");
}

/// REQ: F-096
#[test]
fn clock_096_numerals_follow_the_size_at_three_widths() {
    for (columns, rows) in [(40u16, 10u16), (80, 24), (120, 30)] {
        let screen = clock_screen(columns, rows);
        assert_eq!(screen.cell(0, columns / 2 - 1).symbol, "12", "{columns}");
        assert_eq!(screen.cell(rows / 2, columns - 2).symbol, "3", "{columns}");
    }
}

/// REQ: F-096
#[test]
fn clock_096_numerals_are_bold() {
    let screen = clock_screen(40, 20);
    assert!(screen.cell(0, 19).bold, "glyph plane cells use Modifiers::BOLD");
}

/// REQ: F-096
#[test]
fn clock_096_golden_at_three_widths() {
    for (columns, rows) in [(40u16, 10u16), (80, 24), (120, 30)] {
        ilium_plugin_sdk::testing::assert_golden(&format!("tests/features/golden/clock_{columns}x{rows}.txt"), &clock_screen(columns, rows).text());
    }
}
