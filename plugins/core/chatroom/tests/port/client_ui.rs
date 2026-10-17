//! Twins of the tests of `ilium-client/src/chatroom_ui.rs` and the chatroom case of `ilium-client/src/mouse.rs`,
//! through the message view of the client side: `render` of the panel, the actions `scrolled` and `changed`.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};

use crate::common::*;

fn panel() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

fn feed_numbered(host: &mut FakeHost, count: usize, hour: u32, label: &str) {
    for index in 0..count {
        host.deliver_one(topic_event(&format!(
            "2026-07-26 {hour}:{index:02}:00 +02:00 | agent:codex | {label} {index:02}"
        )));
    }
    host.settle();
}

fn draw(host: &mut FakeHost, columns: u16, rows: u16) -> String {
    host.render(panel(), columns, rows).text()
}

fn scroll_to(host: &mut FakeHost, offset: f64) {
    host.send_action(
        panel(),
        "scroll",
        ActionKind::Scrolled,
        ActionValue::Number(offset),
    );
    host.settle();
}

/// REQ: PORT:ilium-client/src/chatroom_ui.rs::overflowing_room_draws_a_scrollbar_and_starts_at_the_live_tail
#[test]
fn port_overflowing_room_draws_a_scrollbar_and_starts_at_the_live_tail() {
    let mut host = client_host();
    feed_numbered(&mut host, 20, 12, "unique chatroom message");
    let text = draw(&mut host, 100, 18);
    assert!(
        text.contains('\u{2588}'),
        "a scrollbar thumb is drawn:\n{text}"
    );
    assert!(text.contains("unique chatroom message 19"));
    assert!(!text.contains("unique chatroom message 00"));
}

/// REQ: PORT:ilium-client/src/chatroom_ui.rs::overflowing_room_draws_a_scrollbar_and_starts_at_the_live_tail
#[test]
fn port_a_room_that_fits_draws_no_scrollbar_thumb() {
    let mut host = client_host();
    feed_numbered(&mut host, 2, 12, "unique chatroom message");
    let text = draw(&mut host, 100, 18);
    assert!(!text.contains('\u{2588}'), "{text}");
    assert!(
        text.contains("unique chatroom message 00") && text.contains("unique chatroom message 01")
    );
}

/// REQ: PORT:ilium-client/src/chatroom_ui.rs::composer_keeps_the_tail_of_a_long_draft_visible
#[test]
fn port_composer_keeps_the_tail_of_a_long_draft_visible() {
    let mut host = client_host();
    let draft = format!("{}TAIL-MARKER", "long draft padding ".repeat(20));
    host.send_action(
        panel(),
        "composer",
        ActionKind::Changed,
        ActionValue::Text(draft),
    );
    host.settle();
    let text = draw(&mut host, 100, 18);
    assert!(text.contains("TAIL-MARKER"), "{text}");
    assert!(
        !text.contains("long draft padding long draft"),
        "the scrolled-off head must not be drawn"
    );
}

/// REQ: PORT:ilium-client/src/chatroom_ui.rs::composer_keeps_the_tail_of_a_long_draft_visible
#[test]
fn port_composer_shows_a_short_draft_from_its_first_character() {
    let mut host = client_host();
    host.send_action(
        panel(),
        "composer",
        ActionKind::Changed,
        ActionValue::Text("short draft".to_string()),
    );
    host.settle();
    assert!(draw(&mut host, 100, 18).contains("short draft"));
}

/// REQ: PORT:ilium-client/src/chatroom_ui.rs::tail_relative_offset_resolves_to_oldest_and_newest_viewports
#[test]
fn port_tail_relative_offset_resolves_to_oldest_and_newest_viewports() {
    let mut host = client_host();
    feed_numbered(&mut host, 20, 12, "unique chatroom message");
    scroll_to(&mut host, 0.0);
    let oldest = draw(&mut host, 90, 16);
    assert!(oldest.contains("unique chatroom message 00"), "{oldest}");
    assert!(!oldest.contains("unique chatroom message 19"));
    scroll_to(&mut host, 1_000_000.0);
    let newest = draw(&mut host, 90, 16);
    assert!(newest.contains("unique chatroom message 19"), "{newest}");
    assert!(!newest.contains("unique chatroom message 00"));
}

/// REQ: PORT:ilium-client/src/chatroom_ui.rs::scrollbar_track_rows_map_to_bounded_history_positions
#[test]
fn port_scrollbar_track_rows_map_to_bounded_history_positions() {
    let mut host = client_host();
    feed_numbered(&mut host, 100, 14, "history message");
    // Top of the track: the oldest messages; far beyond the bottom: clamped to the newest.
    scroll_to(&mut host, 0.0);
    assert!(draw(&mut host, 100, 18).contains("history message 00"));
    scroll_to(&mut host, 99_999.0);
    let newest = draw(&mut host, 100, 18);
    assert!(newest.contains("history message 99"), "{newest}");
    // A position inside the history shows neither end.
    scroll_to(&mut host, 40.0);
    let middle = draw(&mut host, 100, 18);
    assert!(middle.contains("history message 40"), "{middle}");
    assert!(!middle.contains("history message 00") && !middle.contains("history message 99"));
}

/// REQ: PORT:ilium-client/src/chatroom_ui.rs::scrollbar_track_rows_map_to_bounded_history_positions
#[test]
fn port_scrollbar_a_position_above_the_track_is_clamped_to_the_oldest_message() {
    let mut host = client_host();
    feed_numbered(&mut host, 100, 14, "history message");
    scroll_to(&mut host, -5.0);
    let text = draw(&mut host, 100, 18);
    assert!(text.contains("history message 00"), "{text}");
    assert!(!host.is_disabled());
}

/// REQ: PORT:ilium-client/src/mouse.rs::wheel_and_scrollbar_drag_route_through_the_virtual_chatroom_view
#[test]
fn port_wheel_and_scrollbar_drag_route_through_the_virtual_chatroom_view() {
    let mut host = client_host();
    feed_numbered(&mut host, 30, 14, "mouse history message");
    // Starts at the live tail; a drag to the top of the scrollbar shows the oldest message.
    assert!(draw(&mut host, 100, 18).contains("mouse history message 29"));
    scroll_to(&mut host, 0.0);
    let top = draw(&mut host, 100, 18);
    assert!(
        top.contains("mouse history message 00") && !top.contains("mouse history message 29"),
        "{top}"
    );
    // A drag to the bottom row ends at the newest message again.
    scroll_to(&mut host, 1_000_000.0);
    assert!(draw(&mut host, 100, 18).contains("mouse history message 29"));
}

/// REQ: PORT:ilium-client/src/mouse.rs::wheel_and_scrollbar_drag_route_through_the_virtual_chatroom_view
#[test]
fn port_scrolling_away_from_the_tail_and_back_restores_the_identical_live_tail_view() {
    let mut host = client_host();
    feed_numbered(&mut host, 30, 14, "mouse history message");
    let tail = draw(&mut host, 100, 18);
    assert!(tail.contains("mouse history message 29"));
    scroll_to(&mut host, 0.0);
    scroll_to(&mut host, 1_000_000.0);
    let restored = draw(&mut host, 100, 18);
    assert_eq!(
        restored, tail,
        "wheel up and back down returns to the identical live-tail view"
    );
}
