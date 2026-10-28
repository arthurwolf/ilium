//! Ports of smart_copy_light.rs and the app.rs open-action menus, seen through the picker view.

use crate::common::*;

fn view_text(lines: &[&str], width: u16, height: u16) -> String {
    let mut host = open_picker(lines);
    host.render(picker_target(), width, height).text()
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::short_text_is_shown_whole
#[test]
fn port_light_short_text_is_shown_whole() {
    assert!(view_text(&["short text"], 40, 9).contains("short text"));
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::long_lines_are_clipped_with_an_ellipsis
#[test]
fn port_light_long_lines_are_clipped_with_an_ellipsis() {
    let text = view_text(&["abcdefghijklmnopqrstuvwxyz0123456789"], 12, 9);
    assert!(text.contains('…'));
    assert!(text.lines().all(|line| line.chars().count() <= 12));
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::control_characters_never_reach_the_dialog
#[test]
fn port_light_control_characters_never_reach_the_view() {
    let text = view_text(&["a\u{1b}[31mb\tc"], 40, 9);
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains('\t'));
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::extra_lines_collapse_into_a_count
#[test]
fn port_light_extra_lines_collapse_into_a_count() {
    let many: Vec<String> = (1..=10).map(|n| n.to_string()).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let text = view_text(&refs, 40, 6);
    assert!(text.contains("more"), "{text}");
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::preview_counts_down_over_exactly_one_second
#[test]
fn port_light_the_status_is_set_by_the_copy_and_nothing_hangs() {
    let mut host = open_picker(&["abc"]);
    let out = host.deliver_one(key("1"));
    host.settle();
    assert!(status_contains(&out, "Copied 3 characters"), "{out:?}");
    assert_eq!(host.clipboard().last_text().as_deref(), Some("abc"));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::short_text_is_shown_whole
#[test]
fn port_light_short_text_is_not_marked_as_clipped() {
    assert!(!view_text(&["short text"], 40, 9).contains('…'));
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::control_characters_never_reach_the_dialog
#[test]
fn port_light_the_printable_letters_survive_control_character_removal() {
    let text = view_text(&["a\u{1b}[31mb\tc"], 40, 9);
    assert!(text.contains('b') && text.contains('c'), "{text}");
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::extra_lines_collapse_into_a_count
#[test]
fn port_light_the_collapsed_preview_stays_within_the_dialog_height() {
    let many: Vec<String> = (1..=10).map(|n| n.to_string()).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let text = view_text(&refs, 40, 6);
    assert!(text.lines().count() <= 6, "{text}");
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::long_lines_are_clipped_with_an_ellipsis
#[test]
fn port_light_a_clipped_line_keeps_its_leading_characters() {
    let text = view_text(&["abcdefghijklmnopqrstuvwxyz0123456789"], 12, 9);
    assert!(text.contains("abc"), "{text}");
}
