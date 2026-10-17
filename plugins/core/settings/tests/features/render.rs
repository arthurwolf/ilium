//! What the user sees, at the widths 40, 80 and 120: the overview panel, the Appearance section
//! of the User Interface tab, and the row leaders (a row joins label and control with `…`).

use ilium_plugin_sdk::types::ViewTarget;

use crate::common::*;

const WIDTHS: [u16; 3] = [40, 80, 120];

fn overview() -> ViewTarget {
    ViewTarget::Panel("overview".to_string())
}

fn ui_tab() -> ViewTarget {
    ViewTarget::SettingsPage("user-interface".to_string())
}

/// REQ: F-090
#[test]
fn render_090_the_overview_panel_fills_the_requested_area_at_three_widths() {
    for width in WIDTHS {
        let mut host = client_host();
        let screen = host.render(overview(), width, 24);
        assert_eq!((screen.columns, screen.rows), (width, 24), "width {width}");
        assert!(!screen.text().trim().is_empty(), "width {width}");
    }
}

/// REQ: F-090
#[test]
fn render_090_the_overview_panel_lists_the_density_key_at_three_widths() {
    for width in WIDTHS {
        let mut host = client_host();
        let text = host.render(overview(), width, 24).text();
        assert!(text.contains("density"), "width {width}: {text}");
    }
}

/// REQ: F-090
#[test]
fn render_090_the_overview_panel_is_deterministic() {
    let mut host = client_host();
    let first = host.render(overview(), 80, 24);
    let second = host.render(overview(), 80, 24);
    assert_eq!(first, second);
}

/// REQ: F-090
#[test]
fn render_090_no_row_of_the_overview_exceeds_the_width() {
    for width in WIDTHS {
        let mut host = client_host();
        let screen = host.render(overview(), width, 24);
        for row in 0..screen.rows {
            assert!(
                screen.row(row).chars().count() <= usize::from(width),
                "width {width} row {row}"
            );
        }
    }
}

/// REQ: F-090
#[test]
fn render_090_the_user_interface_tab_shows_the_appearance_rows_at_three_widths() {
    for width in WIDTHS {
        let mut host = client_host();
        let text = host.render(ui_tab(), width, 24).text();
        assert!(text.contains("Density"), "width {width}: {text}");
        assert!(text.contains("Comfortable"), "width {width}: {text}");
        assert!(text.contains("Accent colour"), "width {width}: {text}");
        assert!(text.contains("#4a9eff"), "width {width}: {text}");
    }
}

/// REQ: F-090
#[test]
fn render_090_the_density_row_joins_label_and_value_with_leader_dots() {
    for width in [80u16, 120] {
        let mut host = client_host();
        let screen = host.render(ui_tab(), width, 24);
        let (row, _) = screen.find("Density").expect("Density row");
        let line = screen.row(row);
        assert!(line.contains('…'), "{line}");
        assert!(line.contains("‹ Comfortable ›"), "{line}");
        let label_end = line.find("Density").expect("label") + "Density".len();
        assert!(
            line[label_end..].starts_with(" …"),
            "one blank cell before the dots: {line}"
        );
    }
}

/// REQ: F-090
#[test]
fn render_090_leader_dots_never_overwrite_the_label() {
    for width in WIDTHS {
        let mut host = client_host();
        let text = host.render(ui_tab(), width, 24).text();
        assert!(text.contains("Accent colour"), "width {width}: {text}");
        assert!(text.contains("Density"), "width {width}: {text}");
    }
}

/// REQ: F-090
#[test]
fn render_090_the_density_value_follows_the_stored_setting() {
    let mut host = client_host();
    host.settings()
        .set("density", serde_json::json!("compact"))
        .expect("accepted");
    host.settle();
    let text = host.render(ui_tab(), 80, 24).text();
    assert!(text.contains("‹ Compact ›"), "{text}");
    assert!(!text.contains("Comfortable"), "{text}");
}

/// REQ: F-092
#[test]
fn render_092_the_accent_row_shows_the_stored_colour() {
    let mut host = client_host();
    host.settings()
        .set("accent", serde_json::json!("#102030"))
        .expect("accepted");
    host.settle();
    let text = host.render(ui_tab(), 80, 24).text();
    assert!(text.contains("#102030"), "{text}");
}

/// REQ: F-090
#[test]
fn render_090_a_narrow_area_still_draws_the_tab_without_failure() {
    let mut host = client_host();
    let screen = host.render(ui_tab(), 40, 6);
    assert_eq!((screen.columns, screen.rows), (40, 6));
    assert!(host.failures().is_empty());
}
