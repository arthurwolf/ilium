//! Twins of ilium-client/src/editor_chrome.rs: toolbar row and minimap column geometry (F-019).

use crate::helpers::plain_host;
use crate::support::{active_host, open_file, render_pane};
use serde_json::json;

fn minimap_cells(columns: u16, show_minimap: bool) -> usize {
    let mut host = active_host(&[
        ("show_minimap", json!(show_minimap)),
        ("show_line_numbers", json!(false)),
    ]);
    open_file(
        &mut host,
        1,
        "a.rs",
        Some("rust"),
        &"fn main() {}\n".repeat(8),
    );
    let screen = render_pane(&mut host, columns, 20);
    let glyphs = ['·', '-', '=', '≡', '≣', '█'];
    (0..columns)
        .filter(|column| {
            screen
                .cell(3, *column)
                .symbol
                .chars()
                .any(|c| glyphs.contains(&c))
        })
        .count()
}

/// REQ: PORT:ilium-client/src/editor_chrome.rs::reserves_toolbar_row_and_minimap_column
#[test]
fn port_chrome_reserves_toolbar_row_and_minimap_column() {
    let mut host = active_host(&[
        ("show_minimap", json!(true)),
        ("show_line_numbers", json!(false)),
    ]);
    open_file(&mut host, 1, "a.rs", Some("rust"), "fn main() {}\n");
    let screen = render_pane(&mut host, 40, 20);
    assert!(!screen.row(0).trim().is_empty(), "toolbar occupies row 0");
    assert!(
        screen.row(1).starts_with("fn main"),
        "content starts on row 1: {}",
        screen.row(1)
    );
    assert!(
        minimap_cells(40, true) > 0 && minimap_cells(40, true) <= 8,
        "minimap is at most 8 cells wide"
    );
    assert!(
        screen
            .row(1)
            .chars()
            .take(32)
            .collect::<String>()
            .contains("fn main"),
        "content keeps 32 columns"
    );
}

/// REQ: PORT:ilium-client/src/editor_chrome.rs::drops_minimap_when_disabled
#[test]
fn port_chrome_drops_minimap_when_disabled() {
    assert_eq!(minimap_cells(40, false), 0);
}

/// REQ: PORT:ilium-client/src/editor_chrome.rs::drops_minimap_when_too_narrow
#[test]
fn port_chrome_drops_minimap_when_too_narrow() {
    let mut host = plain_host(
        "a.rs",
        Some("rust"),
        "use std;\n",
        &[
            ("show_minimap", json!(true)),
            ("show_line_numbers", json!(true)),
        ],
    );
    let screen = render_pane(&mut host, 8, 6);
    assert!(
        screen.row(1).starts_with(" 1 use s"),
        "content keeps all 8 columns: {:?}",
        screen.row(1)
    );
}

/// REQ: PORT:ilium-client/src/editor_chrome.rs::reserves_toolbar_row_and_minimap_column
#[test]
fn port_chrome_toolbar_row_stays_reserved_with_the_minimap_off() {
    let mut host = active_host(&[
        ("show_minimap", json!(false)),
        ("show_line_numbers", json!(false)),
    ]);
    open_file(&mut host, 1, "a.rs", Some("rust"), "fn main() {}\n");
    let screen = render_pane(&mut host, 40, 20);
    assert!(!screen.row(0).trim().is_empty(), "toolbar occupies row 0");
    assert!(screen.row(1).starts_with("fn main"), "{}", screen.row(1));
}

/// REQ: PORT:ilium-client/src/editor_chrome.rs::drops_minimap_when_disabled
#[test]
fn port_chrome_disabled_minimap_draws_no_glyphs_at_a_wide_width() {
    assert_eq!(minimap_cells(80, false), 0);
}

/// REQ: PORT:ilium-client/src/editor_chrome.rs::drops_minimap_when_too_narrow
#[test]
fn port_chrome_no_minimap_glyphs_remain_at_eight_columns() {
    assert_eq!(minimap_cells(8, true), 0);
}
