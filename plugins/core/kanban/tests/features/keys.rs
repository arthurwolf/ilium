//! Keyboard use of the board (src/docs/editors-and-boards.md "Use a board") and the changes it saves.

use crate::common::{
    board_file, board_host, draw, press, sections, select_first_header, SAMPLE_BOARD,
};

fn selected_text(
    host: &mut ilium_plugin_sdk::testing::FakeHost,
    node: ilium_plugin_sdk::types::NodeId,
) -> String {
    draw(host, node, 80, 20).text()
}

/// REQ: F-024
#[test]
fn keys_024_down_moves_from_the_column_header_to_the_first_card() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    assert!(selected_text(&mut host, node).contains("> Write docs"));
}

/// REQ: F-024
#[test]
fn keys_024_the_down_arrow_is_the_same_as_j() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "down");
    assert!(selected_text(&mut host, node).contains("> Write docs"));
}

/// REQ: F-024
#[test]
fn keys_024_down_twice_selects_the_second_card() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "j");
    let text = selected_text(&mut host, node);
    assert!(
        text.contains("> Add tests") && !text.contains("> Write docs"),
        "{text}"
    );
}

/// REQ: F-024
#[test]
fn keys_024_down_past_the_last_card_stays_on_the_last_card() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    for _ in 0..6 {
        press(&mut host, node, "j");
    }
    assert!(selected_text(&mut host, node).contains("> Add tests"));
}

/// REQ: F-024
#[test]
fn keys_024_up_returns_from_the_first_card_to_the_header() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "k");
    assert!(!selected_text(&mut host, node).contains("> Write docs"));
}

/// REQ: F-024
#[test]
fn keys_024_right_selects_the_next_column() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "l");
    press(&mut host, node, "j");
    assert!(selected_text(&mut host, node).contains("> Port parser"));
}

/// REQ: F-024
#[test]
fn keys_024_the_right_arrow_is_the_same_as_l() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "right");
    press(&mut host, node, "down");
    assert!(selected_text(&mut host, node).contains("> Port parser"));
}

/// REQ: F-024
#[test]
fn keys_024_left_goes_back_to_the_previous_column() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "l");
    press(&mut host, node, "h");
    press(&mut host, node, "j");
    assert!(selected_text(&mut host, node).contains("> Write docs"));
}

/// REQ: F-024
#[test]
fn keys_024_right_past_the_last_column_stays_on_it() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    for _ in 0..6 {
        press(&mut host, node, "l");
    }
    press(&mut host, node, "j");
    assert!(selected_text(&mut host, node).contains("> Fix typos"));
}

/// REQ: F-024
#[test]
fn keys_024_left_at_the_first_column_stays_on_it() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "h");
    press(&mut host, node, "j");
    assert!(selected_text(&mut host, node).contains("> Write docs"));
}

/// REQ: F-024
#[test]
fn keys_024_enter_opens_the_details_panel_with_title_and_notes() {
    let (mut host, node) = board_host("## L\n- Write docs\n  some notes\n");
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "enter");
    let text = draw(&mut host, node, 120, 24).text().to_lowercase();
    assert!(text.contains("title") && text.contains("notes"), "{text}");
    assert!(text.contains("some notes"), "{text}");
}

/// REQ: F-024
#[test]
fn keys_024_escape_closes_the_details_panel() {
    let (mut host, node) = board_host("## L\n- Write docs\n  some notes\n");
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    let before = draw(&mut host, node, 120, 24).text();
    press(&mut host, node, "enter");
    press(&mut host, node, "escape");
    assert_eq!(draw(&mut host, node, 120, 24).text(), before);
}

/// REQ: F-024
#[test]
fn keys_024_shift_right_moves_the_selected_card_to_the_next_column_and_saves() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+right");
    let saved = sections(&board_file(&mut host));
    assert_eq!(
        saved[0],
        ("To do".to_string(), vec!["Add tests".to_string()])
    );
    assert!(saved[1].1.contains(&"Write docs".to_string()), "{saved:?}");
    assert_eq!(saved[1].1.len(), 2);
}

/// REQ: F-024
#[test]
fn keys_024_shift_left_moves_the_selected_card_to_the_previous_column() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "l");
    press(&mut host, node, "j");
    press(&mut host, node, "shift+left");
    let saved = sections(&board_file(&mut host));
    assert!(saved[0].1.contains(&"Port parser".to_string()), "{saved:?}");
    assert!(saved[1].1.is_empty(), "{saved:?}");
}

/// REQ: F-024
#[test]
fn keys_024_shift_down_moves_the_card_down_within_its_column() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+down");
    let saved = sections(&board_file(&mut host));
    assert_eq!(saved[0].1, ["Add tests", "Write docs"]);
}

/// REQ: F-024
#[test]
fn keys_024_shift_up_moves_the_card_up_within_its_column() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "j");
    press(&mut host, node, "shift+up");
    let saved = sections(&board_file(&mut host));
    assert_eq!(saved[0].1, ["Add tests", "Write docs"]);
}

/// REQ: F-024
#[test]
fn keys_024_shift_up_on_the_first_card_changes_nothing() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+up");
    assert_eq!(
        sections(&board_file(&mut host))[0].1,
        ["Write docs", "Add tests"]
    );
}

/// REQ: F-024
#[test]
fn keys_024_shift_left_in_the_first_column_changes_nothing() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+left");
    assert_eq!(sections(&board_file(&mut host)), sections(SAMPLE_BOARD));
}

/// REQ: F-024
#[test]
fn keys_024_shift_right_in_the_last_column_changes_nothing() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    for _ in 0..3 {
        press(&mut host, node, "l");
    }
    press(&mut host, node, "j");
    press(&mut host, node, "shift+right");
    assert_eq!(sections(&board_file(&mut host)), sections(SAMPLE_BOARD));
}

/// REQ: F-024
#[test]
fn keys_024_moving_a_card_keeps_the_total_number_of_cards() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+right");
    press(&mut host, node, "shift+right");
    let total: usize = sections(&board_file(&mut host))
        .iter()
        .map(|(_, cards)| cards.len())
        .sum();
    assert_eq!(total, 4);
}

/// REQ: F-024
#[test]
fn keys_024_delete_asks_for_confirmation_before_changing_the_file() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "d");
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
}

/// REQ: F-024
#[test]
fn keys_024_reload_keeps_the_board_as_it_is_on_disk() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let before = draw(&mut host, node, 80, 20).text();
    press(&mut host, node, "r");
    assert_eq!(draw(&mut host, node, 80, 20).text(), before);
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
}

/// REQ: F-024
#[test]
fn keys_024_a_key_the_board_does_not_use_changes_nothing() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let before = draw(&mut host, node, 80, 20).text();
    press(&mut host, node, "ctrl+q");
    assert_eq!(draw(&mut host, node, 80, 20).text(), before);
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
}

/// REQ: F-024
#[test]
fn keys_024_a_failed_write_leaves_the_file_untouched() {
    use ilium_plugin_sdk::types::{ErrorCode, PluginError};
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.inject_failure(
        "fs.write-atomic",
        PluginError {
            code: ErrorCode::Io,
            message: "disk full".to_string(),
            retry_after_ms: None,
        },
    );
    select_first_header(&mut host, node);
    press(&mut host, node, "j");
    press(&mut host, node, "shift+right");
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
}
