//! Dragging a card (src/docs/editors-and-boards.md "Use a board" and the page sample "Drop marker during a drag").
//!
//! The drag keys are assumed: `card-<column>-<row>` for a card and `column-<column>` for a column
//! (see API_GAPS.md).

use ilium_plugin_sdk::types::{ActionKind, ActionValue, DragOverValue, DropAccept, DropValue, ViewTarget};

use crate::common::{SAMPLE_BOARD, board_file, board_host, sections};

fn drop_card(source: &str, target: &str, index: u32) -> ActionValue {
    ActionValue::Drop(DropValue { source_key: source.to_string(), target_key: target.to_string(), index })
}

/// REQ: F-024
#[test]
fn drag_024_dropping_a_card_on_another_column_moves_it_there_and_saves() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.send_action(ViewTarget::Pane(node), "column-1", ActionKind::Dropped, drop_card("card-0-0", "column-1", 0));
    host.settle();
    let saved = sections(&board_file(&mut host));
    assert_eq!(saved[0].1, ["Add tests"]);
    assert_eq!(saved[1].1, ["Write docs", "Port parser"]);
}

/// REQ: F-024
#[test]
fn drag_024_the_drop_index_is_the_insertion_point_in_the_target_column() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.send_action(ViewTarget::Pane(node), "column-1", ActionKind::Dropped, drop_card("card-0-0", "column-1", 1));
    host.settle();
    assert_eq!(sections(&board_file(&mut host))[1].1, ["Port parser", "Write docs"]);
}

/// REQ: F-024
#[test]
fn drag_024_dropping_within_the_same_column_reorders_the_cards() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.send_action(ViewTarget::Pane(node), "column-0", ActionKind::Dropped, drop_card("card-0-0", "column-0", 2));
    host.settle();
    assert_eq!(sections(&board_file(&mut host))[0].1, ["Add tests", "Write docs"]);
}

/// REQ: F-024
#[test]
fn drag_024_dropping_a_card_where_it_already_is_changes_nothing() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.send_action(ViewTarget::Pane(node), "column-0", ActionKind::Dropped, drop_card("card-0-0", "column-0", 0));
    host.settle();
    assert_eq!(sections(&board_file(&mut host)), sections(SAMPLE_BOARD));
}

/// REQ: F-024
#[test]
fn drag_024_hovering_a_card_over_a_column_does_not_write_the_file() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.send_action(
        ViewTarget::Pane(node),
        "column-1",
        ActionKind::DragOver,
        ActionValue::DragOver(DragOverValue {
            source_key: "card-0-0".to_string(),
            target_key: "column-1".to_string(),
            index: 0,
            accept: DropAccept::Before,
        }),
    );
    host.settle();
    assert_eq!(board_file(&mut host), SAMPLE_BOARD);
}

/// REQ: F-024
#[test]
fn drag_024_a_refused_drag_over_shows_no_marker_and_no_change() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let before = crate::common::draw(&mut host, node, 80, 20).text();
    host.send_action(
        ViewTarget::Pane(node),
        "column-1",
        ActionKind::DragOver,
        ActionValue::DragOver(DragOverValue {
            source_key: "card-0-0".to_string(),
            target_key: "column-1".to_string(),
            index: 0,
            accept: DropAccept::None,
        }),
    );
    host.settle();
    assert_eq!(crate::common::draw(&mut host, node, 80, 20).text(), before);
}

/// REQ: F-024
#[test]
fn drag_024_an_accepted_drag_over_draws_a_marker_the_refused_one_does_not() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    let plain = crate::common::draw(&mut host, node, 80, 20).text();
    host.send_action(
        ViewTarget::Pane(node),
        "column-1",
        ActionKind::DragOver,
        ActionValue::DragOver(DragOverValue {
            source_key: "card-0-0".to_string(),
            target_key: "column-1".to_string(),
            index: 0,
            accept: DropAccept::Before,
        }),
    );
    host.settle();
    assert_ne!(crate::common::draw(&mut host, node, 80, 20).text(), plain);
}

/// REQ: F-024
#[test]
fn drag_024_a_drop_keeps_the_total_number_of_cards() {
    let (mut host, node) = board_host(SAMPLE_BOARD);
    host.send_action(ViewTarget::Pane(node), "column-2", ActionKind::Dropped, drop_card("card-1-0", "column-2", 0));
    host.settle();
    let total: usize = sections(&board_file(&mut host)).iter().map(|(_, cards)| cards.len()).sum();
    assert_eq!(total, 4);
}
