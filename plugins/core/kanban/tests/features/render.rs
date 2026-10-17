//! What the user sees: the board pane body (golden screen text and structure at widths 40, 80, 120).

use ilium_plugin_sdk::testing::assert_golden;

use crate::common::{board_host, draw, rows_of, workspace_root, SAMPLE_BOARD};

fn golden_path(name: &str) -> String {
    workspace_root()
        .join("plugins/core/kanban/tests/features/golden")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// The first `pre.term` block of the page (the board figure), HTML-unescaped.
fn page_board_figure() -> String {
    let page = crate::common::page_text();
    let start = page.find("<pre class=\"term\">").expect("figure") + "<pre class=\"term\">".len();
    let end = page[start..].find("</pre>").expect("figure end");
    page[start..start + end].to_string()
}

/// REQ: F-024
#[test]
fn render_024_golden_figure_equals_the_page_figure() {
    let golden = std::fs::read_to_string(golden_path("board_48.txt")).expect("golden file");
    assert_eq!(golden.trim_end(), page_board_figure().trim_end());
}

/// REQ: F-024
#[test]
fn render_024_the_figure_is_48_cells_wide_in_every_row() {
    let golden = std::fs::read_to_string(golden_path("board_48.txt")).expect("golden file");
    assert!(golden.lines().all(|line| line.chars().count() == 48));
}

/// REQ: F-024
#[test]
fn render_024_headers_show_the_three_columns_with_card_counts_at_80() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let screen = draw(&mut host, node, 80, 20);
    let header = rows_of(&screen)
        .into_iter()
        .find(|row| row.contains("To do"))
        .expect("header row");
    assert!(header.contains("To do (2)"), "{header}");
    assert!(header.contains("Doing (1)"), "{header}");
    assert!(header.contains("Done (1)"), "{header}");
}

/// REQ: F-024
#[test]
fn render_024_headers_keep_the_column_order_to_do_doing_done() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let screen = draw(&mut host, node, 120, 20);
    let (_, to_do) = screen.find("To do").expect("To do");
    let (_, doing) = screen.find("Doing").expect("Doing");
    let (_, done) = screen.find("Done").expect("Done");
    assert!(to_do < doing && doing < done);
}

/// REQ: F-024
#[test]
fn render_024_all_three_headers_share_one_row_at_120() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let screen = draw(&mut host, node, 120, 20);
    let (to_do_row, _) = screen.find("To do").expect("To do");
    let (doing_row, _) = screen.find("Doing").expect("Doing");
    let (done_row, _) = screen.find("Done").expect("Done");
    assert_eq!(to_do_row, doing_row);
    assert_eq!(doing_row, done_row);
}

/// REQ: F-024
#[test]
fn render_024_every_card_title_is_drawn_at_80() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let text = draw(&mut host, node, 80, 20).text();
    for title in ["Write docs", "Add tests", "Port parser", "Fix typos"] {
        assert!(text.contains(title), "missing {title} in\n{text}");
    }
}

/// REQ: F-024
#[test]
fn render_024_cards_sit_in_rounded_boxes() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let text = draw(&mut host, node, 80, 20).text();
    assert!(text.contains('╭') && text.contains('╯'), "{text}");
}

/// REQ: F-024
#[test]
fn render_024_the_cards_of_a_column_are_listed_top_to_bottom_in_file_order() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let screen = draw(&mut host, node, 80, 20);
    let (write_row, _) = screen.find("Write").expect("first card");
    let (add_row, _) = screen.find("Add").expect("second card");
    assert!(write_row < add_row);
}

/// REQ: F-024
#[test]
fn render_024_no_row_is_wider_than_the_area_at_40_80_and_120() {
    for width in [40u16, 80, 120] {
        let (mut host, node) = board_host(SAMPLE_BOARD);
        let screen = draw(&mut host, node, width, 20);
        assert_eq!((screen.columns, screen.rows), (width, 20));
        assert!(
            rows_of(&screen)
                .iter()
                .all(|row| row.chars().count() <= usize::from(width)),
            "width {width}"
        );
    }
}

/// REQ: F-024
#[test]
fn render_024_at_40_cells_the_first_column_is_whole_and_the_board_scrolls_horizontally() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let text = draw(&mut host, node, 40, 20).text();
    assert!(text.contains("To do (2)"), "{text}");
    assert!(text.contains("Write docs"), "{text}");
    assert!(
        !text.contains("Done (1)"),
        "narrow viewports show whole columns behind a scrollbar:\n{text}"
    );
}

/// REQ: F-024
#[test]
fn render_024_a_cell_below_one_column_still_draws_without_failing() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let screen = draw(&mut host, node, 10, 4);
    assert_eq!((screen.columns, screen.rows), (10, 4));
}

/// REQ: F-024
#[test]
fn render_024_an_empty_board_file_draws_no_columns() {
    let (mut host, node) = board_host("");
    let text = draw(&mut host, node, 80, 20).text();
    assert!(!text.contains("To do"), "{text}");
}

/// REQ: F-024
#[test]
fn render_024_headings_with_no_cards_show_a_zero_count() {
    let (mut host, node) = board_host("# Board\n\n## To do\n\n## Doing\n\n## Done\n");
    let text = draw(&mut host, node, 120, 20).text();
    assert!(
        text.contains("To do (0)") && text.contains("Doing (0)") && text.contains("Done (0)"),
        "{text}"
    );
}

/// REQ: F-024
#[test]
fn render_024_a_long_card_title_wraps_inside_its_column() {
    let long = "A very long card title that cannot fit on one line of a narrow column";
    let (mut host, node) = board_host(&format!("# Board\n\n## To do\n- {long}\n"));
    let screen = draw(&mut host, node, 80, 20);
    assert!(rows_of(&screen).iter().all(|row| row.chars().count() <= 80));
    assert!(
        screen.text().contains("A very long card"),
        "{}",
        screen.text()
    );
}

/// REQ: F-024
#[test]
fn render_024_the_selected_card_carries_the_marker_of_the_page_sample() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    crate::common::select_first_header(&mut host, node);
    crate::common::press(&mut host, node, "j");
    let text = draw(&mut host, node, 80, 20).text();
    assert!(text.contains("> Write docs"), "{text}");
    assert!(!text.contains("> Add tests"), "{text}");
}

/// REQ: F-024
#[test]
fn render_024_a_redraw_keeps_the_selected_card_in_view_state() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    crate::common::select_first_header(&mut host, node);
    crate::common::press(&mut host, node, "j");
    let first = draw(&mut host, node, 80, 20).text();
    let second = draw(&mut host, node, 80, 20).text();
    assert_eq!(first, second);
}

/// REQ: F-024
#[test]
fn render_024_card_preview_off_hides_the_notes_of_a_card() {
    let text = "# Board\n\n## To do\n- Write docs\n  Notes line one\n  Notes line two\n";
    let (mut host, node) = board_host(text);
    host.settings()
        .set("card_preview", serde_json::json!(false))
        .expect("valid");
    let screen = draw(&mut host, node, 80, 20).text();
    assert!(screen.contains("Write docs"));
    assert!(!screen.contains("Notes line one"), "{screen}");
}

/// REQ: F-024
#[test]
fn render_024_card_preview_on_shows_the_notes_of_a_card() {
    let text = "# Board\n\n## To do\n- Write docs\n  Notes line one\n  Notes line two\n";
    let (mut host, node) = board_host(text);
    let screen = draw(&mut host, node, 80, 20).text();
    assert!(screen.contains("Notes line one"), "{screen}");
}

/// REQ: F-024
#[test]
fn render_024_golden_board_at_48_columns() {
    let source = "# Board\n\n## To do\n- Write docs\n- Add tests\n\n## Doing\n- Port parser\n  - [x] lex\n  - [ ] emit\n\n## Done\n- Fix typos\n";
    let (mut host, node) = board_host(source);
    let screen = draw(&mut host, node, 48, 16);
    assert_golden(&golden_path("board_48.txt"), &screen.text());
}
