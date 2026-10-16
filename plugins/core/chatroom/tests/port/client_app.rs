//! Twins of the chatroom tests of `ilium-client/src/app.rs` and `ilium-client/src/filesystem/tests.rs`: the
//! reconcile and repair cadence, following the live tail, the idempotent setup, the change fingerprint.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};

use crate::common::*;

fn install_calls(host: &FakeHost) -> usize {
    host.service_calls().iter().filter(|(service, method, _)| service == "ilium.core.agent-setup" && method == "install").count()
}

fn host_with_room() -> FakeHost {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    host
}

fn append_quiet(host: &mut FakeHost, line: &str) {
    let mut text = room_text(host);
    text.push_str(line);
    text.push('\n');
    host.fs().seed(ROOM, text.as_bytes());
}

/// REQ: PORT:ilium-client/src/app.rs::chatroom_reconciliation_uses_an_independent_one_second_deadline
#[test]
fn port_chatroom_reconciliation_waits_one_second_between_scans() {
    let mut host = host_with_room();
    append_quiet(&mut host, &numbered_line(1));
    host.advance_clock(999);
    assert!(published_texts(&host).is_empty(), "no scan before the one second deadline");
    host.advance_clock(1);
    assert_eq!(published_texts(&host).len(), 1);
}

/// REQ: PORT:ilium-client/src/app.rs::chatroom_reconciliation_uses_an_independent_one_second_deadline
#[test]
fn port_chatroom_reconciliation_deadline_moves_one_second_after_each_scan() {
    let mut host = host_with_room();
    append_quiet(&mut host, &numbered_line(1));
    host.advance_clock(1_000);
    assert_eq!(published_texts(&host).len(), 1);
    append_quiet(&mut host, &numbered_line(2));
    host.advance_clock(999);
    assert_eq!(published_texts(&host).len(), 1, "the next deadline is a full second after the last");
    host.advance_clock(1);
    assert_eq!(published_texts(&host).len(), 2);
}

/// REQ: PORT:ilium-client/src/app.rs::chatroom_full_repairs_are_throttled_between_structural_requests
#[test]
fn port_chatroom_full_repairs_are_throttled_to_the_fifteen_second_interval() {
    let mut host = host_with_room();
    host.advance_clock(1_000);
    let after_first = install_calls(&host);
    host.advance_clock(13_000);
    assert_eq!(install_calls(&host), after_first, "no repair inside the 15 s interval");
    host.advance_clock(2_000);
    assert!(install_calls(&host) > after_first, "a repair runs once 15 s have passed");
}

/// REQ: PORT:ilium-client/src/app.rs::chatroom_full_repairs_are_throttled_between_structural_requests
#[test]
fn port_chatroom_a_structural_change_forces_a_repair_before_the_interval_ends() {
    let mut host = server_host();
    host.advance_clock(1_000);
    let before = install_calls(&host);
    let run = chat(&mut host, &["init"]);
    assert_eq!(run.code, Some(0), "{}", run.err);
    host.settle();
    assert!(install_calls(&host) > before, "creating the room asks Agent setup at once: {:?}", host.service_calls());
}

/// REQ: PORT:ilium-client/src/app.rs::chatroom_refresh_follows_only_while_the_view_is_at_the_live_tail
#[test]
fn port_chatroom_refresh_follows_only_while_the_view_is_at_the_live_tail() {
    let mut host = client_host();
    for index in 0..20usize {
        host.deliver_one(topic_event(&format!("2026-07-26 12:{index:02}:00 +02:00 | agent:codex | initial history message {index:02}")));
    }
    host.settle();
    let panel = ViewTarget::Panel(PANEL.to_string());
    let tail = host.render(panel.clone(), 90, 16).text();
    assert!(tail.contains("initial history message 19"), "{tail}");

    host.send_action(panel.clone(), "scroll", ActionKind::Scrolled, ActionValue::Number(0.0));
    host.deliver_one(topic_event("2026-07-26 12:30:00 +02:00 | agent:claude | new message while history is being inspected"));
    host.settle();
    let inspecting = host.render(panel.clone(), 90, 16).text();
    assert!(inspecting.contains("initial history message 00"), "{inspecting}");
    assert!(!inspecting.contains("new message while history is being inspected"), "{inspecting}");

    host.send_action(panel.clone(), "scroll", ActionKind::Scrolled, ActionValue::Number(1_000_000.0));
    host.deliver_one(topic_event("2026-07-26 12:31:00 +02:00 | agent:codex | new message while following the live tail"));
    host.settle();
    let following = host.render(panel, 90, 16).text();
    assert!(following.contains("new message while following the live tail"), "{following}");
}

/// REQ: PORT:ilium-client/src/app.rs::chatroom_refresh_follows_only_while_the_view_is_at_the_live_tail
#[test]
fn port_chatroom_refresh_with_no_change_leaves_the_view_unchanged() {
    let mut host = client_host();
    for index in 0..20usize {
        host.deliver_one(topic_event(&format!("2026-07-26 12:{index:02}:00 +02:00 | agent:codex | initial history message {index:02}")));
    }
    host.settle();
    let panel = ViewTarget::Panel(PANEL.to_string());
    let before = host.render(panel.clone(), 90, 16).text();
    host.settle();
    assert_eq!(host.render(panel, 90, 16).text(), before);
}

/// REQ: PORT:ilium-client/src/app.rs::project_chatroom_setup_refresh_is_idempotent_and_preserves_room_data
#[test]
fn port_project_chatroom_setup_refresh_is_idempotent_and_preserves_room_data() {
    let mut host = server_host();
    let first = chat(&mut host, &["init"]);
    assert_eq!(first.code, Some(0), "{}", first.err);
    assert!(host.fs().read(ROOM).is_some());
    send(&mut host, "keep me");
    let before = room_text(&mut host);

    let second = chat(&mut host, &["init"]);
    assert_eq!(second.code, Some(0), "{}", second.err);
    assert_eq!(room_text(&mut host), before, "a second setup keeps the room and its records");
    assert!(install_calls(&host) >= 1);
}

/// REQ: PORT:ilium-client/src/app.rs::project_chatroom_setup_refresh_is_idempotent_and_preserves_room_data
#[test]
fn port_project_chatroom_setup_refresh_ticks_never_rewrite_the_room() {
    let mut host = host_with_room();
    send(&mut host, "keep me");
    let before = room_text(&mut host);
    for _ in 0..3 {
        host.advance_clock(16_000);
    }
    assert_eq!(room_text(&mut host), before);
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::chatroom_fingerprint_is_returned_by_ordered_io_job
#[test]
fn port_chatroom_fingerprint_changes_when_a_message_is_appended() {
    let mut host = server_host();
    chat(&mut host, &["init"]);
    host.settle();
    let first_length = room_text(&mut host).len() as u64;

    chat(&mut host, &["send", "--message", "second message", "--author", "external"]);
    host.settle();
    let second_length = room_text(&mut host).len() as u64;
    assert!(second_length > first_length, "the changed room has a new length");
    assert_eq!(host.fs().tail_log().last().map(|call| call.returned_cursor), Some(second_length));
    assert_eq!(published_texts(&host).len(), 1);
    assert!(published_texts(&host)[0].ends_with("| external | second message"));
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::chatroom_fingerprint_is_returned_by_ordered_io_job
#[test]
fn port_chatroom_fingerprint_unchanged_room_publishes_nothing_new() {
    let mut host = host_with_room();
    send(&mut host, "once");
    host.settle();
    let count = published_texts(&host).len();
    host.settle();
    host.advance_clock(2_000);
    assert_eq!(published_texts(&host).len(), count);
}
