//! api-menu-provider.md: parallel provider calls, the 25 ms opening, static entries and
//! providers that are not running (HST-476, HST-477, HST-478, WIT menu-provider.menu-items).

use crate::support::menu::*;

fn entry(id: &str) -> MenuEntry {
    MenuEntry { id: id.to_owned(), label: id.to_owned(), ..MenuEntry::default() }
}

fn context() -> MenuContext {
    MenuContext { menu: "tree.row".to_owned(), ..MenuContext::default() }
}

/// REQ: HST-476
#[test]
fn hst_476_all_providers_of_one_opening_are_called_at_once() {
    let mut harness = MenuHarness::new();
    harness.add_provider("com.example.a", 5, vec![entry("a1")]);
    harness.add_provider("com.example.b", 5, vec![entry("b1")]);
    harness.add_provider("com.example.c", 5, vec![entry("c1")]);
    let opening = harness.open(context());
    let starts: Vec<u64> = opening.provider_calls.iter().map(|(_, start)| *start).collect();
    assert_eq!(starts.len(), 3);
    assert!(starts.iter().all(|start| *start == starts[0]), "{starts:?}");
}

/// REQ: HST-476
#[test]
fn hst_476_menu_opens_when_every_provider_has_answered() {
    let mut harness = MenuHarness::new();
    harness.add_provider("com.example.a", 3, vec![entry("a1")]);
    harness.add_provider("com.example.b", 7, vec![entry("b1")]);
    let opening = harness.open(context());
    assert_eq!(opening.opened_at_ms, 7);
    assert_eq!(opening.frames[0], vec!["a1", "b1"]);
}

/// REQ: HST-476
#[test]
fn hst_476_menu_opens_after_25_ms_when_a_provider_is_late() {
    let mut harness = MenuHarness::new();
    harness.add_provider("com.example.a", 3, vec![entry("a1")]);
    harness.add_provider("com.example.late", 60, vec![entry("late1")]);
    let opening = harness.open(context());
    assert_eq!(opening.opened_at_ms, 25);
}

/// REQ: HST-476
#[test]
fn hst_476_provider_answering_at_exactly_25_ms_is_in_the_menu() {
    let mut harness = MenuHarness::new();
    harness.add_provider("com.example.edge", 25, vec![entry("edge1")]);
    let opening = harness.open(context());
    assert_eq!(opening.frames[0], vec!["edge1"]);
}

/// REQ: HST-476
#[test]
fn hst_476_provider_answering_at_26_ms_is_dropped_for_that_opening() {
    let mut harness = MenuHarness::new();
    harness.add_provider("com.example.late", 26, vec![entry("late1")]);
    harness.add_provider("com.example.a", 1, vec![entry("a1")]);
    let opening = harness.open(context());
    assert_eq!(opening.opened_at_ms, 25);
    assert_eq!(opening.frames[0], vec!["a1"]);
}

/// REQ: HST-476
#[test]
fn hst_476_late_entries_never_appear_in_a_later_frame_of_the_same_opening() {
    let mut harness = MenuHarness::new();
    harness.add_provider("com.example.late", 40, vec![entry("late1")]);
    let opening = harness.open(context());
    assert!(opening.frames.iter().all(|frame| !frame.contains(&"late1".to_owned())));
}

/// REQ: HST-477
#[test]
fn hst_477_static_entries_are_in_the_first_frame() {
    let mut harness = MenuHarness::new();
    harness.static_entries(vec![entry("copy"), entry("paste")]);
    harness.add_provider("com.example.a", 10, vec![entry("a1")]);
    let opening = harness.open(context());
    assert!(opening.frames[0].starts_with(&["copy".to_owned(), "paste".to_owned()]));
}

/// REQ: HST-477
#[test]
fn hst_477_static_entries_are_in_the_first_frame_even_when_every_provider_is_late() {
    let mut harness = MenuHarness::new();
    harness.static_entries(vec![entry("copy")]);
    harness.add_provider("com.example.late", 100, vec![entry("late1")]);
    let opening = harness.open(context());
    assert_eq!(opening.frames[0], vec!["copy"]);
}

/// REQ: HST-477
#[test]
fn hst_477_menu_does_not_change_after_its_first_frame() {
    let mut harness = MenuHarness::new();
    harness.static_entries(vec![entry("copy")]);
    harness.add_provider("com.example.late", 80, vec![entry("late1")]);
    let opening = harness.open(context());
    assert_eq!(opening.frames.len(), 1, "exactly one frame is drawn for one opening");
}

/// REQ: HST-478
#[test]
fn hst_478_opening_does_not_wait_for_the_activation_of_a_provider_that_is_not_running() {
    let mut harness = MenuHarness::new();
    harness.static_entries(vec![entry("copy")]);
    harness.add_stopped_provider("com.example.cold", 500, vec![entry("cold1")]);
    let opening = harness.open(context());
    assert!(opening.opened_at_ms <= 25);
    assert!(opening.activations_waited_for.is_empty());
    assert_eq!(opening.frames[0], vec!["copy"]);
}

/// REQ: HST-478
#[test]
fn hst_478_a_stopped_provider_is_still_activated_for_a_later_opening() {
    let mut harness = MenuHarness::new();
    harness.add_stopped_provider("com.example.cold", 10, vec![entry("cold1")]);
    let first = harness.open(context());
    assert!(first.frames[0].is_empty());
    harness.advance_ms(50);
    let second = harness.open(context());
    assert_eq!(second.frames[0], vec!["cold1"]);
}

/// REQ: WIT:ilium:exports/menu-provider.menu-items
#[test]
fn wit_menu_items_returns_at_most_64_entries_in_the_menu() {
    let mut harness = MenuHarness::new();
    harness.add_provider("com.example.many", 1, (0..70).map(|index| entry(&format!("e{index}"))).collect());
    let opening = harness.open(context());
    assert!(opening.frames[0].len() <= 64 + 1, "64 entries and at most one more-row: {}", opening.frames[0].len());
}
