//! Twins of the tests of `ilium-server/src/chatroom_router.rs` and `ilium-server/tests/smoke.rs`: the tail of
//! CHATROOM.md on `fs-changed` through `fs::tail`, the topic `chatroom`, and the forwarding of `@name` references.

use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{ErrorCode, Intent, NodeClass, PluginError};

use crate::common::*;

const MAX_RECORD_BYTES: usize = 20 * 1024;
const MAX_BATCH_BYTES: usize = 64 * 1024;

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

fn named_pane(name: &str) -> TreeNode {
    TreeNode { id: 0, parent: None, name: name.to_string(), class: NodeClass::Pane, content_kind: Some("ilium.terminal".to_string()), agent: None }
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

/// REQ: PORT:ilium-server/src/chatroom_router.rs::parses_escaped_chat_record_and_multiple_references
#[test]
fn port_parses_escaped_chat_record_and_multiple_references() {
    let mut host = host_with_room();
    let compiler = host.tree().add(named_pane("@compiler"));
    let reviewer = host.tree().add(named_pane("@reviewer"));
    append_raw(&mut host, "- t | sender | hello\\n@compiler @reviewer\n");
    assert_eq!(published_texts(&host), ["t | sender | hello\\n@compiler @reviewer"]);
    assert!(!sent_inputs(&host, compiler).is_empty(), "@compiler is forwarded");
    assert!(!sent_inputs(&host, reviewer).is_empty(), "@reviewer is forwarded");
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::notification_lines_preserve_each_enter_boundary
#[test]
fn port_notification_lines_preserve_each_enter_boundary() {
    let mut host = host_with_room();
    let pane = host.tree().add(named_pane("@compiler"));
    append_raw(&mut host, "- 2026-10-07 05:00:00 +02:00 | @sender | @compiler\\n/goal\\n\n");
    let inputs = sent_inputs(&host, pane);
    assert!(inputs.len() >= 3, "one submission per line, empty lines as a bare Enter: {inputs:?}");
    assert!(inputs.iter().any(|text| text == "/goal"), "{inputs:?}");
    assert!(inputs.iter().any(|text| text.is_empty() || text == "\r"), "{inputs:?}");
}

/// REQ: PORT:ilium-server/tests/smoke.rs::named_chatroom_reference_submits_each_forwarded_line_to_the_named_pane
#[test]
fn port_named_chatroom_reference_submits_each_forwarded_line_to_the_named_pane() {
    let mut host = host_with_room();
    let pane = host.tree().add(named_pane("@compiler"));
    let other = host.tree().add(named_pane("@reviewer"));
    append_raw(&mut host, "- 2026-10-07 05:00:00 +02:00 | @sender | @compiler\\n/goal wake the compiler\n");
    let inputs = sent_inputs(&host, pane);
    let joined = inputs.join("\n");
    assert!(joined.contains("sent you (called @compiler) the following message in the chatroom:"), "{joined:?}");
    assert!(inputs.iter().any(|text| text == "/goal wake the compiler"), "{inputs:?}");
    assert!(sent_inputs(&host, other).is_empty(), "only the named pane receives the message");
}

/// REQ: PORT:ilium-server/tests/smoke.rs::named_chatroom_reference_submits_each_forwarded_line_to_the_named_pane
#[test]
fn port_a_record_that_names_no_pane_forwards_nothing() {
    let mut host = host_with_room();
    let pane = host.tree().add(named_pane("@compiler"));
    append_raw(&mut host, "- t | sender | no reference here\n");
    assert!(sent_inputs(&host, pane).is_empty());
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_returns_only_one_ordered_record_batch
#[test]
fn port_append_reader_returns_only_one_ordered_record_batch() {
    let mut host = host_with_room();
    let mut text = String::new();
    for index in 0..70 {
        text.push_str(&format!("- t | sender | record-{index}\n"));
    }
    let mut current = room_text(&mut host);
    current.push_str(&text);
    host.fs().write(ROOM, current.as_bytes());
    host.deliver(Vec::new());
    let first = published_texts(&host);
    assert_eq!(first.len(), 64, "one poll must not materialize the whole backlog");
    assert_eq!(first[0], "t | sender | record-0");
    assert_eq!(first[63], "t | sender | record-63");
    host.settle();
    let all = published_texts(&host);
    assert_eq!(all.len(), 70);
    assert_eq!(all[64], "t | sender | record-64");
    assert_eq!(all[69], "t | sender | record-69");
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_can_resume_after_the_last_successfully_delivered_record
#[test]
fn port_append_reader_can_resume_after_the_last_successfully_delivered_record() {
    let mut host = host_with_room();
    host.inject_failure("topics.publish", PluginError::new(ErrorCode::Busy, "bus busy"));
    append_raw(&mut host, "- t | sender | first\n- t | sender | second\nhand-edited note\n");
    host.settle();
    let texts = published_texts(&host);
    assert_eq!(texts.iter().filter(|text| text.ends_with("first")).count(), 1, "{texts:?}");
    assert_eq!(texts.iter().filter(|text| text.ends_with("second")).count(), 1, "{texts:?}");
    assert_eq!(texts.len(), 2, "the hand-edited note is never published: {texts:?}");
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_can_resume_after_the_last_successfully_delivered_record
#[test]
fn port_append_reader_after_a_full_delivery_does_not_publish_again() {
    let mut host = host_with_room();
    append_raw(&mut host, "- t | sender | first\n- t | sender | second\nhand-edited note\n");
    host.settle();
    host.settle();
    assert_eq!(published_texts(&host).len(), 2);
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_restarts_when_equal_length_log_is_atomically_replaced
#[test]
fn port_append_reader_restarts_when_equal_length_log_is_atomically_replaced() {
    let mut host = server_host();
    host.fs().write(ROOM, b"- t | sender | old record\n");
    host.settle();
    host.fs().replace_atomically(ROOM, b"- t | sender | new record\n");
    host.settle();
    let texts = published_texts(&host);
    assert!(texts.iter().any(|text| text.ends_with("old record")), "{texts:?}");
    assert!(texts.iter().any(|text| text.ends_with("new record")), "replacement starts a new file generation: {texts:?}");
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_restarts_when_equal_length_log_is_atomically_replaced
#[test]
fn port_append_reader_restarts_when_a_longer_log_is_atomically_replaced() {
    let mut host = server_host();
    host.fs().write(ROOM, b"- t | sender | new record\n");
    host.settle();
    host.fs().replace_atomically(ROOM, b"- t | sender | a longer replacement record\n");
    host.settle();
    let texts = published_texts(&host);
    assert!(texts.iter().any(|text| text.ends_with("a longer replacement record")), "larger replacement starts at byte zero: {texts:?}");
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_bounds_bytes_even_when_record_count_is_below_its_cap
#[test]
fn port_append_reader_bounds_bytes_even_when_record_count_is_below_its_cap() {
    let mut host = host_with_room();
    let line = format!("- t | sender | {}\n", "x".repeat(MAX_RECORD_BYTES - 20));
    let mut current = room_text(&mut host);
    current.push_str(&line.repeat(20));
    host.fs().write(ROOM, current.as_bytes());
    host.deliver(Vec::new());
    let first = published_texts(&host).len();
    assert!(first < 64);
    assert!(first * line.len() <= MAX_BATCH_BYTES, "{first} records of {} bytes", line.len());
    assert!(first < 20, "the backlog is not read in one poll");
    host.settle();
    assert_eq!(published_texts(&host).len(), 20);
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_accepts_the_writer_limit_of_four_byte_unicode_messages
#[test]
fn port_append_reader_accepts_the_writer_limit_of_four_byte_unicode_messages() {
    let mut host = host_with_room();
    let content = "🧭".repeat(4_000);
    let line = format!("- t | sender | {content}\n");
    assert!(line.len() <= MAX_RECORD_BYTES);
    append_raw(&mut host, &line);
    assert_eq!(published_texts(&host), [format!("t | sender | {content}")]);
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_accepts_the_writer_limit_of_four_byte_unicode_messages
#[test]
fn port_writer_limit_message_of_four_thousand_four_byte_characters_is_written_and_published() {
    let mut host = host_with_room();
    let content = "🧭".repeat(4_000);
    let run = send(&mut host, &content);
    assert_eq!(run.code, Some(0), "{}", run.err);
    host.settle();
    let texts = published_texts(&host);
    assert_eq!(texts.len(), 1);
    assert!(texts[0].ends_with(&format!("| agent | {content}")));
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_keeps_an_unterminated_record_for_the_next_batch
#[test]
fn port_append_reader_keeps_an_unterminated_record_for_the_next_batch() {
    let mut host = host_with_room();
    append_raw(&mut host, "- t | sender | first\n- t | sender | second");
    assert_eq!(published_texts(&host), ["t | sender | first"]);
    append_raw(&mut host, "\n");
    assert_eq!(published_texts(&host), ["t | sender | first", "t | sender | second"]);
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_refuses_a_record_beyond_its_byte_limit_without_advancing
#[test]
fn port_append_reader_refuses_a_record_beyond_its_byte_limit_without_advancing() {
    let mut host = host_with_room();
    let record = format!("- t | sender | {}\n", "x".repeat(MAX_RECORD_BYTES));
    append_raw(&mut host, &record);
    assert!(published_texts(&host).is_empty(), "an oversized record is explicit, not published");
    assert!(!host.is_disabled(), "the refusal never disables the plugin");
    assert!(host.fs().read(ROOM).expect("room").ends_with(record.as_bytes()), "the file is left as written");
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_refuses_a_record_beyond_its_byte_limit_without_advancing
#[test]
fn port_append_reader_accepts_a_record_of_exactly_the_byte_limit() {
    let mut host = host_with_room();
    let prefix = "- t | sender | ";
    let content = "x".repeat(MAX_RECORD_BYTES - prefix.len());
    append_raw(&mut host, &format!("{prefix}{content}\n"));
    assert_eq!(published_texts(&host).len(), 1);
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::append_reader_rejects_a_non_regular_path_without_reading_it
#[test]
fn port_append_reader_rejects_a_non_regular_path_without_reading_it() {
    let mut host = server_host();
    host.fs().mkdir(ROOM);
    host.settle();
    assert!(published_texts(&host).is_empty());
    assert!(!host.is_disabled());
    assert!(host.fs().tail_log().iter().all(|call| call.returned_cursor == 0), "{:?}", host.fs().tail_log());
}

/// REQ: PORT:ilium-server/src/chatroom_router.rs::chatroom_read_job_uses_the_finite_io_bank_and_returns_a_retained_batch
#[test]
fn port_chatroom_read_job_returns_a_bounded_batch_with_the_record_and_its_cursor() {
    let mut host = host_with_room();
    let before = room_text(&mut host).len();
    append_raw(&mut host, "- t | sender | bounded\n");
    let tails = host.fs().tail_log();
    assert!(!tails.is_empty());
    assert!(tails.iter().all(|call| call.max_bytes <= MAX_BATCH_BYTES), "{tails:?}");
    assert_eq!(published_texts(&host), ["t | sender | bounded"]);
    assert_eq!(tails.last().map(|call| call.returned_cursor), Some((before + "- t | sender | bounded\n".len()) as u64));
}
