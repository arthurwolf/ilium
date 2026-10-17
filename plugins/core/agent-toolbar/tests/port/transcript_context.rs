//! Twins of transcript_context.rs: user/assistant/tool entries extracted from agent transcripts and bounded to recent plus earliest windows.
//! Request/reply shape: see API_GAPS_ports.md.
use crate::common::{ask, Place};
use serde_json::Value;

fn check(place: Place, op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(place, op, request);
    let fields = expected.as_object().expect("expected is an object");
    for (name, value) in fields {
        assert_eq!(
            &reply[name.as_str()],
            value,
            "field {name} of reply {reply}"
        );
    }
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::antigravity_missing_history_file_returns_empty_instead_of_erroring
#[test]
fn port_transcript_context_antigravity_missing_history_file_returns_empty_instead_of_erroring() {
    check(
        Place::Client,
        "transcript.entries",
        r####"{"agent": "antigravity", "conversation": "c1", "files": []}"####,
        r####"{"entries": []}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::antigravity_reads_matching_user_history
#[test]
fn port_transcript_context_antigravity_reads_matching_user_history() {
    check(
        Place::Client,
        "transcript.entries",
        r####"{"agent": "antigravity", "conversation": "wanted", "history": [{"conversationId": "wanted", "text": "first task"}, {"conversationId": "other", "text": "ignored"}, {"conversationId": "wanted", "text": "second task"}]}"####,
        r####"{"entries": [{"kind": "user", "content": "first task"}, {"kind": "user", "content": "second task"}]}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::claude_extracts_user_assistant_and_tool_entries_in_order
#[test]
fn port_transcript_context_claude_extracts_user_assistant_and_tool_entries_in_order() {
    check(
        Place::Client,
        "transcript.entries",
        r####"{"agent": "claude", "case": "user fix login; assistant text I found the auth handler; tool_result handler source; thinking, tool_use and sidechain user skipped"}"####,
        r####"{"entries": [{"kind": "user", "content": "fix login"}, {"kind": "assistant", "content": "I found the auth handler."}, {"kind": "tool", "content": "handler source"}]}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::codex_event_notifications_do_not_establish_a_user_task
#[test]
fn port_transcript_context_codex_event_notifications_do_not_establish_a_user_task() {
    check(
        Place::Client,
        "transcript.entries",
        r####"{"agent": "codex", "case": "Ilium progress monitor 42 reports completion; <environment_context> payload"}"####,
        r####"{"entries": []}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::codex_extracts_clean_messages_and_textual_tool_outputs
#[test]
fn port_transcript_context_codex_extracts_clean_messages_and_textual_tool_outputs() {
    check(
        Place::Client,
        "transcript.entries",
        r####"{"agent": "codex", "case": "clean user, assistant and tool output blocks"}"####,
        r####"{"entries": [{"kind": "user", "content": "write tests"}, {"kind": "assistant", "content": "The failing path is isolated."}, {"kind": "tool", "content": "test output\nsecond block"}]}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::codex_response_item_user_turns_supersede_sparse_goal_events
#[test]
fn port_transcript_context_codex_response_item_user_turns_supersede_sparse_goal_events() {
    check(
        Place::Client,
        "transcript.entries",
        r####"{"agent": "codex", "case": "response_item user turns over goal events; AGENTS.md, codex_internal_context and monitor text filtered"}"####,
        r####"{"entries": [{"kind": "user", "content": "extension boards"}, {"kind": "user", "content": "add board folders"}]}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::each_entry_kind_keeps_its_own_recent_window
#[test]
fn port_transcript_context_each_entry_kind_keeps_its_own_recent_window() {
    check(
        Place::Client,
        "transcript.window",
        r####"{"per_kind_pushed": 17, "kinds": ["user", "assistant", "tool"], "recent": 15, "earliest": 3}"####,
        r####"{"retained": 51, "dropped": 0}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::earliest_entries_survive_being_evicted_from_the_recent_window
#[test]
fn port_transcript_context_earliest_entries_survive_being_evicted_from_the_recent_window() {
    check(
        Place::Client,
        "transcript.window",
        r####"{"user_entries": 20, "recent": 15, "earliest": 3}"####,
        r####"{"pinned_first": [0, 1, 2], "evicted": [3], "kept_last": 19, "chronological": true, "gap_marker": "[2 earlier user entries omitted]"}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::malformed_lines_are_skipped_without_hiding_later_entries
#[test]
fn port_transcript_context_malformed_lines_are_skipped_without_hiding_later_entries() {
    check(
        Place::Client,
        "transcript.entries",
        r####"{"agent": "claude", "case": "malformed line then a valid user line"}"####,
        r####"{"skipped_malformed": true, "later_entry_kept": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::no_gap_marker_appears_when_nothing_was_evicted
#[test]
fn port_transcript_context_no_gap_marker_appears_when_nothing_was_evicted() {
    check(
        Place::Client,
        "transcript.window",
        r####"{"user_entries": 5, "recent": 15, "earliest": 3}"####,
        r####"{"gap_marker": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::other_agent_class_never_touches_the_filesystem
#[test]
fn port_transcript_context_other_agent_class_never_touches_the_filesystem() {
    check(
        Place::Client,
        "transcript.entries",
        r####"{"agent": "other", "conversation": "x"}"####,
        r####"{"entries": [], "filesystem_reads": 0}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::textual_value_falls_back_to_content_when_text_is_blank
#[test]
fn port_transcript_context_textual_value_falls_back_to_content_when_text_is_blank() {
    check(
        Place::Client,
        "transcript.textual-value",
        r####"{"item": {"text": "  ", "content": "real text"}}"####,
        r####"{"text": "real text"}"####,
    );
}

/// REQ: PORT:ilium-client/src/transcript_context.rs::textual_value_keeps_bare_scalar_items_inside_an_array
#[test]
fn port_transcript_context_textual_value_keeps_bare_scalar_items_inside_an_array() {
    check(
        Place::Client,
        "transcript.textual-value",
        r####"{"item": ["raw string", 42, true]}"####,
        r####"{"text": "raw string\n42\ntrue"}"####,
    );
}
