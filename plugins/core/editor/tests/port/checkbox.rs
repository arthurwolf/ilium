//! Twins of ilium-client/src/markdown/checkbox.rs: a click on the inner cell of a task bracket
//! submits one edit that flips it; anything else submits nothing (F-021).

use crate::helpers::{click_event, plain_host, single_edit};

const FILE: &str = "ilium-client/src/markdown/checkbox.rs";

/// Clicks `column` of line 0 of a Markdown buffer holding `line` and returns the edit, if any.
fn click_edit(line: &str, column: u32) -> Option<(u32, u32, u32, u32, String)> {
    let mut host = plain_host("tasks.md", Some("markdown"), &format!("{line}\n"), &[]);
    let submissions = host.deliver_one(click_event(0, column));
    single_edit(&submissions)
}

fn flips(line: &str, bracket_offset: u32, replacement: &str) {
    let inner = bracket_offset + 1;
    let edit = click_edit(line, inner);
    assert_eq!(edit, Some((0, inner, 0, inner + 1, replacement.to_string())), "{line:?}");
}

fn inert(line: &str, column: u32) {
    assert_eq!(click_edit(line, column), None, "{line:?} at column {column}");
}

/// REQ: PORT:ilium-client/src/markdown/checkbox.rs::unchecked_dash_item
#[test]
fn port_checkbox_unchecked_dash_item_is_checked_by_a_click() {
    let _ = FILE;
    flips("- [ ] buy milk", 2, "x");
}

/// REQ: PORT:ilium-client/src/markdown/checkbox.rs::checked_dash_item
#[test]
fn port_checkbox_checked_dash_item_is_unchecked_by_a_click() {
    flips("- [x] buy milk", 2, " ");
}

/// REQ: PORT:ilium-client/src/markdown/checkbox.rs::checked_uppercase_x
#[test]
fn port_checkbox_checked_uppercase_x_is_unchecked_by_a_click() {
    flips("- [X] buy milk", 2, " ");
}

/// REQ: PORT:ilium-client/src/markdown/checkbox.rs::indented_item
#[test]
fn port_checkbox_indented_item_bracket_sits_after_the_indent() {
    flips("  - [ ] nested", 4, "x");
}

/// REQ: PORT:ilium-client/src/markdown/checkbox.rs::star_and_plus_markers
#[test]
fn port_checkbox_star_and_plus_markers_are_task_items() {
    flips("* [ ] a", 2, "x");
    flips("+ [x] a", 2, " ");
}

/// REQ: PORT:ilium-client/src/markdown/checkbox.rs::ordered_marker
#[test]
fn port_checkbox_ordered_markers_with_dot_and_paren_are_task_items() {
    flips("1. [ ] a", 3, "x");
    flips("12) [x] a", 4, " ");
}

/// REQ: PORT:ilium-client/src/markdown/checkbox.rs::plain_list_item_has_no_checkbox
#[test]
fn port_checkbox_plain_list_item_has_no_checkbox() {
    inert("- just text", 3);
    inert("- just text", 2);
}

/// REQ: PORT:ilium-client/src/markdown/checkbox.rs::non_list_line_is_not_a_checkbox
#[test]
fn port_checkbox_non_list_line_is_not_a_checkbox() {
    inert("plain paragraph [ ] not a checkbox", 17);
}

/// REQ: PORT:ilium-client/src/markdown/checkbox.rs::brackets_without_space_or_x_are_rejected
#[test]
fn port_checkbox_brackets_without_space_or_x_are_rejected() {
    inert("- [y] a", 3);
    inert("- [  ] a", 3);
}
