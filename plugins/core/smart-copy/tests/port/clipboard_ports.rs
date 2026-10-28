//! Ports of terminal_clipboard.rs, smart_copy_selection.rs and smart_copy_light.rs: what reaches the
//! clipboard, in which order, and what the status line says.

use crate::common::{candidates_message, key, open_picker, status_contains};

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::framed_commands_preserve_unicode_and_order
#[test]
fn port_clipboard_framed_commands_preserve_unicode_and_order() {
    let mut host = open_picker(&["雪\u{1b}[31m", "second"]);
    host.deliver_one(key("1"));
    assert_eq!(
        host.clipboard().last_text().as_deref(),
        Some("雪\u{1b}[31m")
    );
    host.deliver_one(key("2"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("second"));
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::original_shared_leaf_is_kept_through_blocking_response_without_text_copy
#[test]
fn port_clipboard_original_text_is_written_exactly() {
    let mut host = open_picker(&["exact original α"]);
    host.deliver_one(key("1"));
    assert_eq!(
        host.clipboard().last_text().as_deref(),
        Some("exact original α")
    );
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::raw_write_protocol_remains_identical_and_bad_ack_is_not_success
#[test]
fn port_clipboard_a_bad_acknowledgement_is_not_success() {
    let mut host = open_picker(&["raw original"]);
    host.clipboard().fail_next_write("Io");
    let out = host.deliver_one(key("1"));
    assert!(status_contains(&out, "Copy failed: Io"));
    assert!(!status_contains(&out, "Copied"));
    assert_eq!(host.clipboard().last_text(), None);
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::truncated_reply_never_acknowledges
#[test]
fn port_clipboard_a_truncated_reply_never_reports_a_copy() {
    let mut host = open_picker(&["partial"]);
    host.clipboard().fail_next_write("Io");
    let out = host.deliver_one(key("1"));
    assert!(!status_contains(&out, "Copied"));
    assert!(status_contains(&out, "Copy failed"));
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::oversized_header_is_rejected_before_payload_read
#[test]
fn port_clipboard_an_oversized_reply_is_a_failed_copy_not_a_success() {
    let mut host = open_picker(&["big"]);
    host.clipboard().fail_next_write("Io");
    let out = host.deliver_one(key("1"));
    assert!(status_contains(&out, "Copy failed"));
    assert_eq!(host.clipboard().last_text(), None);
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::blocked_owned_child_preserves_fifo_and_shutdown_observes_actual_exit
#[test]
fn port_clipboard_copies_land_in_submission_order() {
    let mut host = open_picker(&["seed", "after"]);
    host.deliver_one(key("1"));
    host.deliver_one(key("2"));
    host.settle();
    assert_eq!(host.clipboard().last_text().as_deref(), Some("after"));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy_selection.rs::native_failure_restores_same_session_allocation_and_explicit_retry_uses_new_fifo_slot
#[test]
fn port_selection_a_failed_copy_keeps_the_candidates_and_a_retry_copies_them() {
    let mut host = open_picker(&["β original", "second"]);
    host.clipboard().fail_next_write("Io");
    let failed = host.deliver_one(key("1"));
    assert!(status_contains(&failed, "Copy failed: Io"));
    let retried = host.deliver_one(key("1"));
    assert!(status_contains(&retried, "Copied 10 characters"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("β original"));
}

/// REQ: PORT:ilium-client/src/smart_copy_selection.rs::wrong_duplicate_ack_and_stale_generation_cannot_claim_or_replace_newer_preview
#[test]
fn port_selection_one_copy_reports_exactly_one_copied_status() {
    let mut host = open_picker(&["frozen"]);
    let out = host.deliver_one(key("1"));
    let copied = out
        .iter()
        .filter(|submission| {
            status_contains(std::slice::from_ref(*submission), "Copied 6 characters")
        })
        .count();
    assert_eq!(copied, 1);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::summary_reports_counts_and_clipboard_failure
#[test]
fn port_light_the_summary_reports_the_character_count_and_the_clipboard_failure() {
    let mut host = open_picker(&["abc"]);
    let ok = host.deliver_one(key("1"));
    assert!(status_contains(&ok, "Copied 3 characters"));
    let mut failing = open_picker(&["abc"]);
    failing.clipboard().fail_next_write("Io");
    let out = failing.deliver_one(key("1"));
    assert!(status_contains(&out, "Copy failed: Io"));
}

/// REQ: PORT:ilium-client/src/smart_copy_light.rs::summary_reports_counts_and_clipboard_failure
#[test]
fn port_light_the_count_is_in_characters_not_bytes() {
    let mut host = open_picker(&["héllo wörld"]);
    let out = host.deliver_one(key("1"));
    assert!(status_contains(&out, "Copied 11 characters"));
}

/// REQ: PORT:ilium-client/src/smart_copy_selection.rs::native_failure_restores_same_session_allocation_and_explicit_retry_uses_new_fifo_slot
#[test]
fn port_selection_a_failed_copy_leaves_the_clipboard_untouched() {
    let mut host = open_picker(&["β original", "second"]);
    host.clipboard().fail_next_write("Io");
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text(), None);
}

/// REQ: PORT:ilium-client/src/smart_copy_selection.rs::wrong_duplicate_ack_and_stale_generation_cannot_claim_or_replace_newer_preview
#[test]
fn port_selection_a_newer_candidate_list_replaces_the_older_one() {
    let mut host = open_picker(&["old"]);
    host.deliver_one(candidates_message(&["newer"]));
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("newer"));
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::blocked_owned_child_preserves_fifo_and_shutdown_observes_actual_exit
#[test]
fn port_clipboard_a_copy_after_a_failed_one_still_lands_in_order() {
    let mut host = open_picker(&["one", "two"]);
    host.clipboard().fail_next_write("Io");
    host.deliver_one(key("1"));
    host.deliver_one(key("2"));
    host.settle();
    assert_eq!(host.clipboard().last_text().as_deref(), Some("two"));
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::framed_commands_preserve_unicode_and_order
#[test]
fn port_clipboard_wide_characters_are_counted_once_each() {
    let mut host = open_picker(&["雪雪"]);
    let out = host.deliver_one(key("1"));
    assert!(status_contains(&out, "Copied 2 characters"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("雪雪"));
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::original_shared_leaf_is_kept_through_blocking_response_without_text_copy
#[test]
fn port_clipboard_leading_and_trailing_spaces_are_kept() {
    let mut host = open_picker(&["  padded  "]);
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("  padded  "));
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::raw_write_protocol_remains_identical_and_bad_ack_is_not_success
#[test]
fn port_clipboard_a_retry_after_a_bad_acknowledgement_copies_the_text() {
    let mut host = open_picker(&["raw original"]);
    host.clipboard().fail_next_write("Io");
    host.deliver_one(key("1"));
    let retried = host.deliver_one(key("1"));
    assert!(status_contains(&retried, "Copied 12 characters"));
    assert_eq!(
        host.clipboard().last_text().as_deref(),
        Some("raw original")
    );
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::truncated_reply_never_acknowledges
#[test]
fn port_clipboard_the_next_attempt_after_a_truncated_reply_reports_the_copy() {
    let mut host = open_picker(&["partial"]);
    host.clipboard().fail_next_write("Io");
    host.deliver_one(key("1"));
    let retried = host.deliver_one(key("1"));
    assert!(status_contains(&retried, "Copied 7 characters"));
}

/// REQ: PORT:ilium-client/src/terminal_clipboard.rs::oversized_header_is_rejected_before_payload_read
#[test]
fn port_clipboard_a_thousand_character_line_is_copied_whole() {
    let big = "x".repeat(1000);
    let mut host = open_picker(&[big.as_str()]);
    let out = host.deliver_one(key("1"));
    assert!(status_contains(&out, "Copied 1000 characters"));
    assert_eq!(
        host.clipboard()
            .last_text()
            .map(|text| text.chars().count()),
        Some(1000)
    );
}
