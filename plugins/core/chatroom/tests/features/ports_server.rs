//! Ports of the baseline tests of `ilium-server/src/chatroom_router.rs`, `ilium-client` (app, filesystem,
//! chatroom) and `ilium-server/tests/smoke.rs` that the plugin must still pass, written against the plugin
//! channels: `fs-changed` and `fs::tail` in, topic `chatroom` out.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Intent, NodeClass};

use crate::common::*;

fn host_with_room() -> FakeHost {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    host
}

fn append_raw(host: &mut FakeHost, text: &str) {
    let mut current = room_text(host);
    current.push_str(text);
    host.fs().write(ROOM, current.as_bytes());
    host.settle();
}

/// REQ: F-062, F-064, PORT:ilium-server/src/chatroom_router.rs::parses_escaped_chat_record_and_multiple_references
#[test]
fn router_f062_an_escaped_record_is_published_with_its_escapes_intact() {
    let mut host = host_with_room();
    append_raw(&mut host, "- t | sender | hello\\n@compiler @reviewer\n");
    let texts = published_texts(&host);
    assert_eq!(texts, ["t | sender | hello\\n@compiler @reviewer"]);
}

/// REQ: F-062, F-064, PORT:ilium-server/src/chatroom_router.rs::notification_lines_preserve_each_enter_boundary
#[test]
fn router_f062_forwarded_text_keeps_one_input_line_per_enter_boundary() {
    let mut host = host_with_room();
    let pane = host.tree().add(named_pane("@compiler"));
    append_raw(&mut host, "- 2026-10-07 05:00:00 +02:00 | @sender | @compiler\\n/goal wake the compiler\n");
    let inputs = sent_inputs(&host, pane);
    assert!(!inputs.is_empty());
    let joined = inputs.join("");
    assert!(joined.contains("/goal wake the compiler"), "{joined:?}");
    assert!(joined.ends_with('\n') || inputs.last().is_some_and(|last| last.is_empty() || last.ends_with('\n')));
}

fn named_pane(name: &str) -> ilium_plugin_sdk::testing::TreeNode {
    ilium_plugin_sdk::testing::TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    }
}

fn sent_inputs(host: &FakeHost, pane: u64) -> Vec<String> {
    host.submitted()
        .into_iter()
        .filter_map(|submission| match submission.intent {
            Intent::SendInput(spec) if spec.pane.0 == pane => Some(spec.text),
            _ => None,
        })
        .collect()
}

/// REQ: F-062, F-064, PORT:ilium-server/tests/smoke.rs::named_chatroom_reference_submits_each_forwarded_line_to_the_named_pane
#[test]
fn router_f062_a_record_naming_a_pane_submits_each_forwarded_line_to_that_pane() {
    let mut host = host_with_room();
    let pane = host.tree().add(named_pane("@compiler"));
    append_raw(&mut host, "- 2026-10-07 05:00:00 +02:00 | @sender | @compiler\\n/goal wake the compiler\n");
    let inputs = sent_inputs(&host, pane);
    assert!(inputs.iter().any(|text| text.contains("/goal wake the compiler")), "{inputs:?}");
}

/// REQ: F-062, F-064
#[test]
fn router_f062_a_record_naming_no_pane_forwards_nothing() {
    let mut host = host_with_room();
    let pane = host.tree().add(named_pane("@compiler"));
    append_raw(&mut host, "- t | sender | no reference here\n");
    assert!(sent_inputs(&host, pane).is_empty());
}

/// REQ: F-062, F-064, PORT:ilium-server/src/chatroom_router.rs::append_reader_returns_only_one_ordered_record_batch
#[test]
fn router_f062_one_poll_publishes_at_most_64_records_and_the_next_poll_the_rest() {
    let mut host = host_with_room();
    let mut text = String::new();
    for n in 0..70 {
        text.push_str(&format!("- t | sender | record-{n}\n"));
    }
    let mut current = room_text(&mut host);
    current.push_str(&text);
    host.fs().write(ROOM, current.as_bytes());
    host.deliver(Vec::new());
    let first = published_texts(&host);
    assert_eq!(first.len(), 64, "one poll must not materialize the whole backlog");
    assert!(first[0].ends_with("record-0") && first[63].ends_with("record-63"));
    host.settle();
    let all = published_texts(&host);
    assert_eq!(all.len(), 70);
    assert!(all[64].ends_with("record-64"));
}

/// REQ: F-062, F-064, PORT:ilium-server/src/chatroom_router.rs::append_reader_can_resume_after_the_last_successfully_delivered_record
#[test]
fn router_f062_a_failed_publish_leaves_the_record_for_the_next_poll() {
    let mut host = host_with_room();
    host.inject_failure(
        "topics.publish",
        ilium_plugin_sdk::types::PluginError::new(ilium_plugin_sdk::types::ErrorCode::Busy, "bus busy"),
    );
    append_raw(&mut host, "- t | sender | first\n- t | sender | second\nhand-edited note\n");
    host.settle();
    let texts = published_texts(&host);
    assert_eq!(texts.iter().filter(|text| text.ends_with("first")).count(), 1, "{texts:?}");
    assert_eq!(texts.iter().filter(|text| text.ends_with("second")).count(), 1, "{texts:?}");
}

/// REQ: F-062, F-064, PORT:ilium-server/src/chatroom_router.rs::append_reader_restarts_when_equal_length_log_is_atomically_replaced
#[test]
fn router_f062_an_atomically_replaced_log_of_equal_length_is_read_again_from_the_start() {
    let mut host = host_with_room();
    host.fs().write(ROOM, b"- t | sender | old record\n");
    host.settle();
    host.fs().replace_atomically(ROOM, b"- t | sender | new record\n");
    host.settle();
    let texts = published_texts(&host);
    assert!(texts.iter().any(|text| text.ends_with("old record")));
    assert!(texts.iter().any(|text| text.ends_with("new record")), "{texts:?}");
}

/// REQ: F-062, F-064
#[test]
fn router_f062_a_longer_replacement_is_read_from_byte_zero() {
    let mut host = host_with_room();
    host.fs().write(ROOM, b"- t | sender | new record\n");
    host.settle();
    host.fs().replace_atomically(ROOM, b"- t | sender | a longer replacement record\n");
    host.settle();
    assert!(published_texts(&host).iter().any(|text| text.ends_with("a longer replacement record")));
}

/// REQ: F-062, F-064, PORT:ilium-server/src/chatroom_router.rs::append_reader_bounds_bytes_even_when_record_count_is_below_its_cap
#[test]
fn router_f062_one_poll_reads_at_most_64_kib_even_with_fewer_than_64_records() {
    let mut host = host_with_room();
    let line = format!("- t | sender | {}\n", "x".repeat(20 * 1024 - 20));
    let mut current = room_text(&mut host);
    current.push_str(&line.repeat(20));
    host.fs().write(ROOM, current.as_bytes());
    host.deliver(Vec::new());
    let first = published_texts(&host).len();
    assert!(first < 64);
    assert!(first * line.len() <= 64 * 1024, "{first} records of {} bytes", line.len());
    assert!(first < 20, "the backlog is not read in one poll");
    host.settle();
    assert_eq!(published_texts(&host).len(), 20);
}

/// REQ: F-062, F-064, PORT:ilium-server/src/chatroom_router.rs::append_reader_keeps_an_unterminated_record_for_the_next_batch
#[test]
fn router_f062_an_unterminated_last_record_waits_for_its_newline() {
    let mut host = host_with_room();
    append_raw(&mut host, "- t | sender | first\n- t | sender | second");
    assert_eq!(published_texts(&host).len(), 1);
    append_raw(&mut host, "\n");
    let texts = published_texts(&host);
    assert_eq!(texts.len(), 2);
    assert!(texts[1].ends_with("second"));
}

/// REQ: F-062, F-064, PORT:ilium-server/src/chatroom_router.rs::append_reader_refuses_a_record_beyond_its_byte_limit_without_advancing
#[test]
fn router_f062_a_record_beyond_20_kib_is_refused_and_not_published() {
    let mut host = host_with_room();
    append_raw(&mut host, &format!("- t | sender | {}\n", "x".repeat(20 * 1024)));
    assert!(published_texts(&host).is_empty());
    assert!(!host.is_disabled());
    assert!(host.diagnostics().iter().any(|entry| entry.message.contains("record")), "{:?}", host.diagnostics());
}

/// REQ: F-062, F-064
#[test]
fn router_f062_a_record_of_exactly_20_kib_is_still_published() {
    let mut host = host_with_room();
    let prefix = "- t | sender | ";
    let content = "x".repeat(20 * 1024 - prefix.len() - 1);
    append_raw(&mut host, &format!("{prefix}{content}\n"));
    assert_eq!(published_texts(&host).len(), 1);
}

/// REQ: F-062, F-064, PORT:ilium-server/src/chatroom_router.rs::append_reader_rejects_a_non_regular_path_without_reading_it
#[test]
fn router_f062_a_directory_at_the_room_path_is_not_read_and_not_published() {
    let mut host = server_host();
    host.fs().mkdir(ROOM);
    host.settle();
    assert!(published_texts(&host).is_empty());
    assert!(!host.is_disabled());
}

/// REQ: F-062, F-064, PORT:ilium-server/src/chatroom_router.rs::chatroom_read_job_uses_the_finite_io_bank_and_returns_a_retained_batch
#[test]
fn router_f062_the_read_is_bounded_in_bytes_per_call_of_fs_tail() {
    let mut host = host_with_room();
    append_raw(&mut host, "- t | sender | bounded\n");
    let tails = host.fs().tail_log();
    assert!(!tails.is_empty());
    assert!(tails.iter().all(|call| call.max_bytes <= 65_536), "{tails:?}");
    assert_eq!(published_texts(&host), ["t | sender | bounded"]);
}

/// REQ: F-062, F-064, PORT:ilium-client/src/filesystem/tests.rs::chatroom_fingerprint_is_returned_by_ordered_io_job
#[test]
fn router_f062_the_tail_cursor_follows_the_file_length_after_each_read() {
    let mut host = host_with_room();
    let first_length = room_text(&mut host).len() as u64;
    append_raw(&mut host, "- t | sender | one\n");
    let second_length = room_text(&mut host).len() as u64;
    let cursors = host.fs().tail_log().iter().map(|call| call.returned_cursor).collect::<Vec<_>>();
    assert_eq!(cursors.last().copied(), Some(second_length));
    assert!(second_length > first_length);
    send(&mut host, "two");
    host.settle();
    assert_eq!(host.fs().tail_log().last().map(|call| call.returned_cursor), Some(room_text(&mut host).len() as u64));
}

/// REQ: F-062, F-064, PORT:ilium-client/src/chatroom.rs::append_repairs_a_missing_trailing_newline_left_by_a_human_edit
#[test]
fn router_f062_append_repairs_a_missing_trailing_newline_left_by_a_human_edit() {
    let mut host = host_with_room();
    let mut text = room_text(&mut host);
    while text.ends_with('\n') {
        text.pop();
    }
    host.fs().write(ROOM, text.as_bytes());
    host.settle();
    send(&mut host, "hello");
    let records = room_records(&mut host);
    assert_eq!(records.len(), 1);
    assert_eq!(split_record(&records[0]).2, "hello");
    assert!(room_text(&mut host).contains("## Messages\n- "));
}

/// REQ: F-062, F-064, PORT:ilium-client/src/chatroom.rs::integration_repair_upgrades_an_older_hook_command_in_place
#[test]
fn router_f062_a_start_with_a_room_asks_agent_setup_to_repair_the_hook_in_place() {
    let mut host = server_host();
    host.fs().write(ROOM, initial_room().as_bytes());
    host.settle();
    let calls = host.service_calls();
    let repairs = calls.iter().filter(|(service, method, _)| service == "ilium.core.agent-setup" && method == "install").count();
    assert!(repairs >= 1, "{calls:?}");
    assert!(host.fs().read("project:.claude/settings.local.json").is_none(), "this plugin writes no agent file");
}

/// REQ: F-062, F-064, PORT:ilium-client/src/app.rs::chatroom_reconciliation_uses_an_independent_one_second_deadline
#[test]
fn router_f062_the_room_is_reconciled_on_a_one_second_deadline() {
    let mut host = host_with_room();
    let text = room_text(&mut host);
    host.fs().seed(ROOM, format!("{}{}\n", text, numbered_line(1)).as_bytes());
    host.advance_clock(999);
    assert!(published_texts(&host).is_empty(), "no reconcile before one second");
    host.advance_clock(1);
    assert_eq!(published_texts(&host).len(), 1);
}

/// REQ: F-062, F-064, PORT:ilium-client/src/app.rs::chatroom_full_repairs_are_throttled_between_structural_requests
#[test]
fn router_f062_full_repairs_run_at_most_once_every_15_seconds() {
    let mut host = host_with_room();
    let count = |host: &FakeHost| host.service_calls().iter().filter(|(service, method, _)| service == "ilium.core.agent-setup" && method == "install").count();
    host.advance_clock(1_000);
    let after_first = count(&host);
    host.advance_clock(13_000);
    assert_eq!(count(&host), after_first, "no repair inside the 15 s interval");
    host.advance_clock(1_000);
    host.advance_clock(1_000);
    assert!(count(&host) > after_first, "a repair runs once 15 s have passed");
}

/// REQ: F-062, F-064, PORT:ilium-client/src/app.rs::project_chatroom_setup_refresh_is_idempotent_and_preserves_room_data
#[test]
fn router_f062_repeated_repairs_leave_the_room_untouched() {
    let mut host = host_with_room();
    send(&mut host, "keep me");
    let before = room_text(&mut host);
    for _ in 0..3 {
        host.advance_clock(16_000);
    }
    assert_eq!(room_text(&mut host), before);
}
