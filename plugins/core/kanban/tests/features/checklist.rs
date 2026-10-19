//! Task checkboxes in card titles: a click toggles the single character and writes it at once.

use crate::common::{board_file, board_host, draw, sections};
use ilium_plugin_sdk::types::ViewTarget;

const OPEN_TASK: &str = "# Board\n\n## Doing\n- [ ] emit\n- [x] lex\n";

/// REQ: F-024
#[test]
fn check_024_clicking_an_open_checkbox_ticks_it_in_the_file() {
    let (mut host, node) = board_host(OPEN_TASK);
    let (row, column) = draw(&mut host, node, 80, 20)
        .find("[ ] emit")
        .expect("open task drawn");
    assert!(host.click(ViewTarget::Pane(node), column + 1, row));
    host.settle();
    assert_eq!(
        sections(&board_file(&mut host))[0].1,
        ["[x] emit", "[x] lex"]
    );
}

/// REQ: F-024
#[test]
fn check_024_clicking_a_ticked_checkbox_unticks_it() {
    let (mut host, node) = board_host(OPEN_TASK);
    let (row, column) = draw(&mut host, node, 80, 20)
        .find("[x] lex")
        .expect("ticked task drawn");
    assert!(host.click(ViewTarget::Pane(node), column + 1, row));
    host.settle();
    assert_eq!(
        sections(&board_file(&mut host))[0].1,
        ["[ ] emit", "[ ] lex"]
    );
}

/// REQ: F-024
#[test]
fn check_024_the_write_changes_exactly_one_character() {
    let (mut host, node) = board_host(OPEN_TASK);
    let (row, column) = draw(&mut host, node, 80, 20)
        .find("[ ] emit")
        .expect("open task drawn");
    host.click(ViewTarget::Pane(node), column + 1, row);
    host.settle();
    let after = board_file(&mut host);
    assert_eq!(after.len(), OPEN_TASK.len());
    let differing = after
        .chars()
        .zip(OPEN_TASK.chars())
        .filter(|(left, right)| left != right)
        .count();
    assert_eq!(differing, 1);
}

/// REQ: F-024
#[test]
fn check_024_the_drawn_checkbox_follows_the_file_after_a_click() {
    let (mut host, node) = board_host(OPEN_TASK);
    let (row, column) = draw(&mut host, node, 80, 20)
        .find("[ ] emit")
        .expect("open task drawn");
    host.click(ViewTarget::Pane(node), column + 1, row);
    host.settle();
    let text = draw(&mut host, node, 80, 20).text();
    assert!(
        text.contains("[x] emit") && !text.contains("[ ] emit"),
        "{text}"
    );
}

/// REQ: F-024
#[test]
fn check_024_two_clicks_restore_the_original_file() {
    let (mut host, node) = board_host(OPEN_TASK);
    let (row, column) = draw(&mut host, node, 80, 20)
        .find("[ ] emit")
        .expect("open task drawn");
    host.click(ViewTarget::Pane(node), column + 1, row);
    host.settle();
    let (row, column) = draw(&mut host, node, 80, 20)
        .find("[x] emit")
        .expect("ticked task drawn");
    host.click(ViewTarget::Pane(node), column + 1, row);
    host.settle();
    assert_eq!(board_file(&mut host), OPEN_TASK);
}

/// REQ: F-024
#[test]
fn check_024_a_click_on_the_card_text_does_not_toggle_the_checkbox() {
    let (mut host, node) = board_host(OPEN_TASK);
    let (row, column) = draw(&mut host, node, 80, 20)
        .find("emit")
        .expect("task drawn");
    host.click(ViewTarget::Pane(node), column + 1, row);
    host.settle();
    assert_eq!(sections(&board_file(&mut host)), sections(OPEN_TASK));
}

/// REQ: F-024
#[test]
fn check_024_a_click_on_an_empty_cell_takes_no_widget() {
    let (mut host, node) = board_host(OPEN_TASK);
    let _ = draw(&mut host, node, 80, 20);
    assert!(!host.click(ViewTarget::Pane(node), 79, 19));
    assert_eq!(
        host.fs()
            .read(crate::common::BOARD_PATH)
            .map(|bytes| bytes.len()),
        Some(OPEN_TASK.len())
    );
}
