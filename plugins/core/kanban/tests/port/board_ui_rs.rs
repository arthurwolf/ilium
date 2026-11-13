//! Twins of ilium-client/src/board_ui.rs tests, on the drawn screen and mouse clicks.

use ilium_plugin_sdk::types::ViewTarget;
use serde_json::json;

use crate::common::{board_file, board_host, draw, press, sections, SAMPLE_BOARD};

fn board_with(cards: &[&str]) -> String {
    let mut text = String::from("## To do\n");
    for card in cards {
        text.push_str(&format!("- {card}\n"));
    }
    text
}

/// REQ: PORT:ilium-client/src/board_ui.rs::board_footer_hints_fit_and_keep_their_viewport_rows
#[test]
fn port_board_ui_footer_hints_name_the_board_actions_and_fit_every_width() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let wide = draw(&mut host, node, 120, 24).text();
    for expected in [
        "column", "card", "details", "new", "rename", "delete", "move", "drag",
    ] {
        assert!(
            wide.contains(expected),
            "missing board action {expected}: {wide}"
        );
    }
    for width in [20u16, 40, 60, 80, 120, 180] {
        for line in draw(&mut host, node, width, 24).text().lines() {
            assert!(
                line.chars().count() <= usize::from(width),
                "clipped at {width}: {line}"
            );
        }
    }
}

/// REQ: PORT:ilium-client/src/board_ui.rs::detail_panel_owns_exactly_the_rightmost_third
#[test]
fn port_board_ui_detail_panel_owns_the_rightmost_third() {
    let (mut host, node) = board_host("## To do\n- Task\n  complete body\n");
    press(&mut host, node, "j");
    press(&mut host, node, "enter");
    let screen = draw(&mut host, node, 120, 30);
    let (_, column) = screen.find("complete body").expect("detail body drawn");
    assert!(
        column >= 80,
        "detail starts at column {column}, expected the rightmost third (80..120)"
    );
}

/// REQ: PORT:ilium-client/src/board_ui.rs::minimum_width_pages_columns_and_exposes_horizontal_scrollbar
#[test]
fn port_board_ui_minimum_width_pages_columns_and_shows_a_horizontal_scrollbar() {
    let mut text = String::new();
    for index in 0..5 {
        text.push_str(&format!("## Column {index}\n- card\n\n"));
    }
    let (mut host, node) = board_host(&text);
    host.settings()
        .set("column_width", json!(20))
        .expect("valid");
    let screen = draw(&mut host, node, 60, 20).text();
    assert!(
        screen.contains("Column 0") && !screen.contains("Column 4"),
        "{screen}"
    );
    assert!(
        screen.contains('\u{25B6}'),
        "scrollbar arrow missing: {screen}"
    );
}

/// REQ: PORT:ilium-client/src/board_ui.rs::cards_leave_a_spacer_row_and_follow_the_preview_line_setting
#[test]
fn port_board_ui_cards_leave_a_spacer_row_between_them() {
    let (mut host, node) = board_host(&board_with(&["alpha", "beta"]));
    let screen = draw(&mut host, node, 40, 20);
    let (alpha, _) = screen.find("alpha").expect("alpha");
    let (beta, _) = screen.find("beta").expect("beta");
    assert!(beta >= alpha + 3, "alpha row {alpha}, beta row {beta}");
}

/// REQ: PORT:ilium-client/src/board_ui.rs::long_unicode_column_titles_leave_room_for_the_card_count
#[test]
fn port_board_ui_long_unicode_titles_keep_the_card_count_visible() {
    let (mut host, node) = board_host("## 制作中 👩‍💻 Very long column name\n- one\n");
    for width in [24u16, 30, 40, 80] {
        let text = draw(&mut host, node, width, 12).text();
        assert!(text.contains("1 "), "count hidden at width {width}: {text}");
    }
}

/// REQ: PORT:ilium-client/src/board_ui.rs::selected_cards_beyond_the_first_page_remain_visible_and_clickable
#[test]
fn port_board_ui_selected_card_beyond_the_first_page_stays_visible() {
    let titles: Vec<String> = (0..8).map(|index| format!("Card {index}")).collect();
    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
    let (mut host, node) = board_host(&board_with(&refs));
    for _ in 0..9 {
        press(&mut host, node, "j");
    }
    let screen = draw(&mut host, node, 60, 14);
    assert!(
        screen.find("Card 7").is_some() && screen.find("Card 0").is_none(),
        "{}",
        screen.text()
    );
    let (row, column) = screen.find("Card 7").expect("card 7");
    assert!(host.click(ViewTarget::Pane(node), column, row));
}

/// REQ: PORT:ilium-client/src/board_ui.rs::hit_testing_uses_the_same_three_line_card_geometry
#[test]
fn port_board_ui_clicks_hit_the_card_that_is_drawn_there() {
    let (mut host, node) = board_host(&board_with(&["first card", "second item"]));
    let screen = draw(&mut host, node, 60, 20);
    let (row, column) = screen.find("second item").expect("second card");
    assert!(host.click(ViewTarget::Pane(node), column, row));
    host.settle();
    press(&mut host, node, "enter");
    assert!(draw(&mut host, node, 60, 20).text().contains("second item"));
}

/// REQ: PORT:ilium-client/src/board_ui.rs::render_shows_three_wrapped_lines_without_a_blank_card_row
#[test]
fn port_board_ui_a_long_title_wraps_into_three_lines() {
    let (mut host, node) = board_host(&board_with(&[
        "one two three four five six seven",
        "second item",
    ]));
    let rows: Vec<String> = draw(&mut host, node, 18, 20)
        .text()
        .lines()
        .map(str::to_string)
        .collect();
    assert!(rows[2].contains("one two three"), "{rows:?}");
    assert!(rows[3].contains("four five six"), "{rows:?}");
    assert!(rows[4].contains("seven"), "{rows:?}");
    assert!(rows[8].contains("second item"), "{rows:?}");
}

/// REQ: PORT:ilium-client/src/board_ui.rs::checkbox_hit_uses_the_wrapped_card_rendering, PORT:ilium-client/src/board_ui.rs::checkbox_index_stays_aligned_when_an_earlier_marker_would_have_wrapped
#[test]
fn port_board_ui_checkbox_click_toggles_the_marker_that_was_hit() {
    let (mut host, node) = board_host(&board_with(&["AAAA [ ] second [x] third"]));
    host.settings()
        .set("column_width", json!(12))
        .expect("valid");
    let screen = draw(&mut host, node, 11, 20);
    let (row, column) = screen.find("[x]").expect("second marker drawn");
    assert!(host.click(ViewTarget::Pane(node), column + 1, row));
    host.settle();
    assert_eq!(
        sections(&board_file(&mut host))[0].1,
        ["AAAA [ ] second [ ] third"]
    );
}

/// REQ: PORT:ilium-client/src/board_ui.rs::active_drag_renders_a_visible_insertion_line
#[test]
fn port_board_ui_an_active_drag_draws_an_insertion_line() {
    use ilium_plugin_sdk::types::{ActionKind, ActionValue, DragOverValue};
    let (mut host, node) = board_host(&board_with(&["one", "two"]));
    host.send_action(
        ViewTarget::Pane(node),
        "column-0",
        ActionKind::DragOver,
        ActionValue::DragOver(DragOverValue {
            source_key: "card-0-0".to_string(),
            target_key: "column-0".to_string(),
            index: 1,
            accept: ilium_plugin_sdk::types::DropAccept::Before,
        }),
    );
    host.settle();
    assert!(draw(&mut host, node, 30, 20).text().contains('\u{2501}'));
}

/// REQ: PORT:ilium-client/src/board_ui.rs::atomic_checkbox_title_keeps_each_marker_as_one_unbreakable_word, PORT:ilium-client/src/board_ui.rs::checkbox_marker_survives_a_wrap_point_between_its_brackets, PORT:ilium-client/src/board_ui.rs::checkbox_index_survives_an_earlier_marker_hard_split_inside_a_long_word
#[test]
fn port_board_ui_a_checkbox_marker_is_never_split_across_a_wrap_point() {
    let (mut host, node) = board_host(&board_with(&["AAAA [ ]"]));
    host.settings()
        .set("column_width", json!(12))
        .expect("valid");
    let screen = draw(&mut host, node, 9, 12);
    assert!(
        screen
            .text()
            .lines()
            .any(|line| line.contains('[') && line.contains(']')),
        "{}",
        screen.text()
    );
    let (row, column) = screen.find("[").expect("marker");
    assert!(host.click(ViewTarget::Pane(node), column + 1, row));
    host.settle();
    assert_eq!(sections(&board_file(&mut host))[0].1, ["AAAA [x]"]);
}
