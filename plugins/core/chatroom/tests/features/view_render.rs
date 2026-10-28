//! The message view and composer of the client side (chatroom-B07, chatroom-B08, chatroom-B09): goldens at
//! widths 40, 80 and 120, the figure of the page, the empty view, the composer action and scrolling.

use ilium_plugin_sdk::testing::{assert_golden, FakeHost};
use ilium_plugin_sdk::types::{ActionKind, ActionValue, Intent, ViewTarget};

use crate::common::*;

fn golden_path(name: &str) -> String {
    workspace_root()
        .join("plugins/core/chatroom/tests/features/golden")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn feed(host: &mut FakeHost, lines: &[(&str, &str, &str)]) {
    for (time, author, content) in lines {
        host.deliver_one(topic_event(&format!(
            "2026-10-12 {time} +02:00 | {author} | {content}"
        )));
    }
    host.settle();
}

fn two_short_messages() -> FakeHost {
    let mut host = client_host();
    feed(
        &mut host,
        &[
            ("09:14:02", "fix-auth", "Hi."),
            ("09:15:40", "review", "Ok."),
        ],
    );
    host
}

fn draw(host: &mut FakeHost, columns: u16, rows: u16) -> String {
    host.render(ViewTarget::Panel(PANEL.to_string()), columns, rows)
        .text()
}

/// REQ: chatroom-B09, F-062
#[test]
fn view_b09_golden_at_40_columns() {
    let mut host = two_short_messages();
    assert_golden(&golden_path("view_40.txt"), &draw(&mut host, 40, 8));
}

/// REQ: chatroom-B09, F-062
#[test]
fn view_b09_golden_at_80_columns() {
    let mut host = two_short_messages();
    assert_golden(&golden_path("view_80.txt"), &draw(&mut host, 80, 8));
}

/// REQ: chatroom-B09, F-062
#[test]
fn view_b09_golden_at_120_columns() {
    let mut host = two_short_messages();
    assert_golden(&golden_path("view_120.txt"), &draw(&mut host, 120, 8));
}

/// REQ: chatroom-B09, F-062
#[test]
fn view_b09_figure_of_the_page_equals_the_golden_file() {
    let golden = std::fs::read_to_string(golden_path("view_48.txt")).expect("golden file");
    assert_eq!(
        golden,
        page_art("Rendered at 48 columns. Illustrative. Each message")
    );
}

/// REQ: chatroom-B09
#[test]
fn view_b09_the_figure_is_drawn_by_the_plugin_with_the_page_records() {
    // The figure is 50 cells wide in the page although its caption says 48 columns (see API_GAPS.md).
    let mut host = client_host();
    feed(
        &mut host,
        &[
            ("09:14:02", "fix-auth", "Claiming src/auth.rs until 10:00."),
            ("09:15:40", "review", "Tests pass."),
        ],
    );
    let text = draw(&mut host, 50, 9);
    assert_golden(&golden_path("view_48.txt"), &text);
}

/// REQ: chatroom-B09
#[test]
fn view_b09_every_row_is_timestamp_author_content_without_the_offset() {
    let mut host = two_short_messages();
    let text = draw(&mut host, 80, 8);
    assert!(text.contains("2026-10-12 09:14:02 fix-auth Hi."), "{text}");
    assert!(!text.contains("+02:00"));
    assert!(!text.contains(" | "));
}

/// REQ: chatroom-B09
#[test]
fn view_b09_newest_message_is_below_the_older_one() {
    let mut host = two_short_messages();
    let screen = host.render(ViewTarget::Panel(PANEL.to_string()), 80, 8);
    let (older, _) = screen.find("fix-auth Hi.").expect("older");
    let (newer, _) = screen.find("review Ok.").expect("newer");
    assert!(older < newer);
}

/// REQ: chatroom-B09
#[test]
fn view_b09_the_list_of_agents_comes_before_the_messages() {
    let mut host = two_short_messages();
    let screen = host.render(ViewTarget::Panel(PANEL.to_string()), 80, 8);
    let (heading, _) = screen.find("Agents in this project").expect("heading");
    let (first_message, _) = screen.find("fix-auth Hi.").expect("message");
    assert!(heading < first_message);
}

/// REQ: chatroom-B09
#[test]
fn view_b09_the_agents_list_names_each_author_once_in_order_of_first_message() {
    let mut host = client_host();
    feed(
        &mut host,
        &[
            ("09:00:00", "b", "1"),
            ("09:00:01", "a", "2"),
            ("09:00:02", "b", "3"),
        ],
    );
    assert!(draw(&mut host, 80, 10).contains(" b \u{b7} a"));
}

/// REQ: chatroom-B09
#[test]
fn view_b09_a_long_message_wraps_with_a_two_space_continuation_indent() {
    let mut host = client_host();
    feed(
        &mut host,
        &[("09:14:02", "fix-auth", "Claiming src/auth.rs until 10:00.")],
    );
    let text = draw(&mut host, 50, 9);
    assert!(
        text.contains("\u{2502} 2026-10-12 09:14:02 fix-auth Claiming"),
        "{text}"
    );
    assert!(
        text.contains("\u{2502}   src/auth.rs until 10:00."),
        "{text}"
    );
}

/// REQ: chatroom-B08
#[test]
fn view_b08_the_empty_view_reads_no_messages_yet_type_below_to_post_as_user() {
    let mut host = client_host();
    assert!(draw(&mut host, 80, 8).contains("No messages yet. Type below to post as user."));
}

/// REQ: chatroom-B08
#[test]
fn view_b08_the_empty_text_disappears_with_the_first_message() {
    let mut host = client_host();
    feed(&mut host, &[("09:00:00", "a", "hello")]);
    assert!(!draw(&mut host, 80, 8).contains("No messages yet"));
}

/// REQ: chatroom-B08
#[test]
fn view_b08_the_composer_shows_the_placeholder_write_message_enter_sends() {
    let mut host = client_host();
    assert!(draw(&mut host, 80, 8).contains("Write message - Enter sends"));
}

/// REQ: chatroom-B08
#[test]
fn view_b08_the_placeholder_disappears_while_a_draft_is_typed() {
    let mut host = client_host();
    host.send_action(
        ViewTarget::Panel(PANEL.to_string()),
        "composer",
        ActionKind::Changed,
        ActionValue::Text("abc".into()),
    );
    host.settle();
    let text = draw(&mut host, 80, 8);
    assert!(text.contains("abc"));
    assert!(!text.contains("Write message - Enter sends"));
}

/// REQ: chatroom-B08
#[test]
fn view_b08_enter_sends_the_draft_to_the_server_side_on_the_post_channel() {
    let mut host = client_host();
    host.send_action(
        ViewTarget::Panel(PANEL.to_string()),
        "composer",
        ActionKind::Submitted,
        ActionValue::Text("hello room".into()),
    );
    let sent: Vec<_> = host
        .submitted()
        .into_iter()
        .filter_map(|submission| match submission.intent {
            Intent::Message(spec) => Some(spec),
            _ => None,
        })
        .collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].channel, "post");
    assert_eq!(
        sent[0].to.plugin,
        ilium_plugin_sdk::types::PluginId(PLUGIN.to_string())
    );
    assert_eq!(sent[0].to.side, ilium_plugin_sdk::types::PluginSide::Server);
    assert_eq!(sent[0].payload, b"hello room");
}

/// REQ: chatroom-B08
#[test]
fn view_b08_the_composer_empties_after_enter() {
    let mut host = client_host();
    let panel = ViewTarget::Panel(PANEL.to_string());
    host.send_action(
        panel.clone(),
        "composer",
        ActionKind::Changed,
        ActionValue::Text("draft".into()),
    );
    host.send_action(
        panel,
        "composer",
        ActionKind::Submitted,
        ActionValue::Text("draft".into()),
    );
    host.settle();
    assert!(draw(&mut host, 80, 8).contains("Write message - Enter sends"));
}

/// REQ: chatroom-B08, chatroom-B03
#[test]
fn view_b08_a_blank_draft_sends_nothing() {
    let mut host = client_host();
    host.send_action(
        ViewTarget::Panel(PANEL.to_string()),
        "composer",
        ActionKind::Submitted,
        ActionValue::Text("   ".into()),
    );
    assert!(host
        .submitted()
        .iter()
        .all(|submission| !matches!(submission.intent, Intent::Message(_))));
}

/// REQ: chatroom-B08, PORT:ilium-client/src/chatroom_ui.rs::composer_keeps_the_tail_of_a_long_draft_visible
#[test]
fn view_b08_a_long_draft_keeps_its_tail_visible_and_drops_its_head() {
    let mut host = client_host();
    let draft = format!("{}TAIL-MARKER", "long draft padding ".repeat(20));
    host.send_action(
        ViewTarget::Panel(PANEL.to_string()),
        "composer",
        ActionKind::Changed,
        ActionValue::Text(draft),
    );
    host.settle();
    let text = draw(&mut host, 100, 18);
    assert!(text.contains("TAIL-MARKER"));
    assert!(!text.contains("long draft padding long draft"));
}

/// True when the text holds "record <n>" not followed by another digit.
fn shows_record(text: &str, n: usize) -> bool {
    let needle = format!("record {n}");
    text.match_indices(&needle).any(|(at, _)| {
        !text[at + needle.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    })
}

/// REQ: chatroom-B07
#[test]
fn view_b07_the_view_shows_the_newest_200_messages() {
    let mut host = client_host();
    for n in 0..201usize {
        host.deliver_one(topic_event(numbered_line(n).trim_start_matches("- ")));
    }
    host.settle();
    let text = draw(&mut host, 120, 400);
    for n in 1..=200usize {
        assert!(shows_record(&text, n), "record {n} is among the newest 200");
    }
    assert!(!shows_record(&text, 0), "message 0 is the 201st newest");
}

/// REQ: chatroom-B07
#[test]
fn view_b07_exactly_200_messages_are_all_shown() {
    let mut host = client_host();
    for n in 0..200usize {
        host.deliver_one(topic_event(numbered_line(n).trim_start_matches("- ")));
    }
    host.settle();
    let text = draw(&mut host, 120, 400);
    for n in 0..200usize {
        assert!(shows_record(&text, n), "record {n} is shown");
    }
}

/// REQ: chatroom-B07
#[test]
fn view_b07_199_messages_are_all_shown() {
    let mut host = client_host();
    for n in 0..199usize {
        host.deliver_one(topic_event(numbered_line(n).trim_start_matches("- ")));
    }
    host.settle();
    assert!(draw(&mut host, 120, 400).contains("record 198"));
}

/// REQ: chatroom-B07, PORT:ilium-client/src/chatroom_ui.rs::overflowing_room_draws_a_scrollbar_and_starts_at_the_live_tail
#[test]
fn view_b07_an_overflowing_room_starts_at_the_live_tail_and_shows_a_scrollbar() {
    let mut host = client_host();
    for n in 0..20usize {
        host.deliver_one(topic_event(&format!(
            "2026-07-26 12:{n:02}:00 +02:00 | agent:codex | unique chatroom message {n:02}"
        )));
    }
    host.settle();
    let text = draw(&mut host, 100, 18);
    assert!(text.contains("unique chatroom message 19"));
    assert!(!text.contains("unique chatroom message 00"));
    assert!(text.contains('\u{2588}'), "a scrollbar thumb is drawn");
}

/// REQ: chatroom-B07, PORT:ilium-client/src/chatroom_ui.rs::tail_relative_offset_resolves_to_oldest_and_newest_viewports
#[test]
fn view_b07_the_scroll_offset_zero_shows_the_oldest_and_a_large_offset_shows_the_newest() {
    let mut host = client_host();
    for n in 0..20usize {
        host.deliver_one(topic_event(&format!(
            "2026-07-26 12:{n:02}:00 +02:00 | agent:codex | unique chatroom message {n:02}"
        )));
    }
    host.settle();
    let panel = ViewTarget::Panel(PANEL.to_string());
    host.send_action(
        panel.clone(),
        "scroll",
        ActionKind::Scrolled,
        ActionValue::Number(0.0),
    );
    host.settle();
    let oldest = draw(&mut host, 90, 16);
    assert!(oldest.contains("unique chatroom message 00"));
    assert!(!oldest.contains("unique chatroom message 19"));
    host.send_action(
        panel,
        "scroll",
        ActionKind::Scrolled,
        ActionValue::Number(1_000_000.0),
    );
    host.settle();
    let newest = draw(&mut host, 90, 16);
    assert!(newest.contains("unique chatroom message 19"));
}

/// REQ: chatroom-B07, PORT:ilium-client/src/mouse.rs::wheel_and_scrollbar_drag_route_through_the_virtual_chatroom_view
#[test]
fn view_b07_a_wheel_step_up_then_down_returns_to_the_live_tail() {
    let mut host = client_host();
    for n in 0..30usize {
        host.deliver_one(topic_event(&format!(
            "2026-07-26 14:{n:02}:00 +02:00 | agent:codex | mouse history message {n:02}"
        )));
    }
    host.settle();
    let panel = ViewTarget::Panel(PANEL.to_string());
    host.send_action(
        panel.clone(),
        "scroll",
        ActionKind::Scrolled,
        ActionValue::Number(0.0),
    );
    host.settle();
    assert!(!draw(&mut host, 100, 18).contains("mouse history message 29"));
    host.send_action(
        panel,
        "scroll",
        ActionKind::Scrolled,
        ActionValue::Number(1_000_000.0),
    );
    host.settle();
    assert!(draw(&mut host, 100, 18).contains("mouse history message 29"));
}

/// REQ: chatroom-B07, PORT:ilium-client/src/app.rs::chatroom_refresh_follows_only_while_the_view_is_at_the_live_tail
#[test]
fn view_b07_a_new_message_follows_only_while_the_view_is_at_the_live_tail() {
    let mut host = client_host();
    for n in 0..20usize {
        host.deliver_one(topic_event(&format!(
            "2026-07-26 12:{n:02}:00 +02:00 | agent:codex | initial history message {n:02}"
        )));
    }
    host.settle();
    let panel = ViewTarget::Panel(PANEL.to_string());
    // Scrolled up by hand: the view keeps its place when a message arrives.
    host.send_action(
        panel.clone(),
        "scroll",
        ActionKind::Scrolled,
        ActionValue::Number(0.0),
    );
    host.deliver_one(topic_event(
        "2026-07-26 12:30:00 +02:00 | agent:claude | new message while history is being inspected",
    ));
    host.settle();
    assert!(!draw(&mut host, 90, 16).contains("new message while history is being inspected"));
    // Back at the live tail: the next message is followed.
    host.send_action(
        panel,
        "scroll",
        ActionKind::Scrolled,
        ActionValue::Number(1_000_000.0),
    );
    host.deliver_one(topic_event(
        "2026-07-26 12:31:00 +02:00 | agent:codex | new message while following the live tail",
    ));
    host.settle();
    assert!(draw(&mut host, 90, 16).contains("new message while following the live tail"));
}

/// REQ: chatroom-B07, PORT:ilium-client/src/chatroom_ui.rs::scrollbar_track_rows_map_to_bounded_history_positions
#[test]
fn view_b07_scroll_offsets_beyond_the_ends_are_clamped_not_rejected() {
    let mut host = client_host();
    for n in 0..30usize {
        host.deliver_one(topic_event(&format!(
            "2026-07-26 14:{n:02}:00 +02:00 | agent:codex | mouse history message {n:02}"
        )));
    }
    host.settle();
    let panel = ViewTarget::Panel(PANEL.to_string());
    for offset in [-5.0, 0.0, 1.0, 1e9] {
        host.send_action(
            panel.clone(),
            "scroll",
            ActionKind::Scrolled,
            ActionValue::Number(offset),
        );
        host.settle();
        assert!(!draw(&mut host, 100, 18).is_empty());
    }
    assert!(!host.is_disabled());
}

/// REQ: chatroom-B09, F-062
#[test]
fn view_b09_the_tree_figure_shows_the_unread_title_the_row_publishes() {
    let figure = std::fs::read_to_string(golden_path("tree_48.txt")).expect("golden file");
    assert!(figure.contains("Chatroom (3 new)"));
    let mut host = server_host();
    seed_room(
        &mut host,
        &[numbered_line(1), numbered_line(2), numbered_line(3)],
    );
    assert_eq!(
        row_titles(&host.submitted()).pop().as_deref(),
        Some("Chatroom (3 new)")
    );
}

/// REQ: chatroom-B09
#[test]
fn view_b09_a_hand_written_non_record_line_is_not_a_row() {
    let mut host = client_host();
    host.deliver_one(topic_event("just a note"));
    host.settle();
    let text = draw(&mut host, 80, 8);
    assert!(!text.contains("just a note"));
}

/// REQ: chatroom-B09
#[test]
fn view_b09_the_panel_is_registered_with_the_client_side_ui_panels_grant() {
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("manifest"))
            .expect("json");
    let client: Vec<&str> = manifest["capabilities"]["client"]
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(client.contains(&"ui.panels"));
}
