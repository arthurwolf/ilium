//! The client panel (F-031) and its two figures (golden files equal the page figures).

use ilium_plugin_sdk::testing::assert_golden;
use ilium_plugin_sdk::ViewTarget;

use crate::common::*;

fn read_golden(name: &str) -> String {
    std::fs::read_to_string(golden_path(name)).expect("golden readable")
}

/// REQ: F-031
#[test]
fn doc_f_031_stats_golden_equals_the_first_page_figure() {
    assert_eq!(read_golden("cost_stats_48.txt"), term_blocks(&page_text())[0]);
}

/// REQ: F-038
#[test]
fn doc_f_038_quota_golden_equals_the_second_page_figure() {
    assert_eq!(read_golden("cost_quota_48.txt"), term_blocks(&page_text())[1]);
}

/// REQ: F-031
#[test]
fn doc_f_031_stats_figure_is_a_titled_frame_47_columns_wide() {
    let figure = term_blocks(&page_text())[0].clone();
    assert!(figure.lines().next().expect("line").starts_with("╭ Session statistics"));
    assert!(figure.lines().last().expect("line").starts_with('╰'));
    assert!(figure.lines().all(|line| line.chars().count() == 47));
}

/// REQ: F-038
#[test]
fn doc_f_038_quota_figure_is_a_titled_frame_47_columns_wide() {
    let figure = term_blocks(&page_text())[1].clone();
    assert!(figure.lines().next().expect("line").starts_with("╭ Quota"));
    assert!(figure.lines().all(|line| line.chars().count() == 47));
}

fn render(cols: u16, rows: u16) -> String {
    let mut host = activated_client();
    host.render(ViewTarget::Panel(PANEL.to_string()), cols, rows).text()
}

/// REQ: F-031
#[test]
fn panel_031_renders_a_titled_frame_at_width_40() {
    let text = render(40, 14);
    assert!(text.contains("╭ Session statistics"), "{text}");
    assert!(text.contains('╰'));
}

/// REQ: F-031
#[test]
fn panel_031_renders_a_titled_frame_at_width_80() {
    let text = render(80, 14);
    assert!(text.contains("╭ Session statistics"), "{text}");
}

/// REQ: F-031
#[test]
fn panel_031_renders_a_titled_frame_at_width_120() {
    let text = render(120, 14);
    assert!(text.contains("╭ Session statistics"), "{text}");
}

/// REQ: F-031
#[test]
fn panel_031_rows_never_exceed_the_width() {
    for cols in [40u16, 80, 120] {
        let text = render(cols, 14);
        assert!(text.lines().all(|line| line.chars().count() <= cols as usize));
    }
}

/// REQ: F-031
#[test]
fn panel_031_stats_render_at_width_48_matches_the_golden() {
    let mut host = activated_client();
    let screen = host.render(ViewTarget::Panel(PANEL.to_string()), 48, 11);
    assert_golden(&golden_path("cost_stats_48.txt"), &screen.text());
}
