//! The `ilium.core.editor/buffers` service: list, read, replace-range (core-plugin-editor.md
//! "Extension points", "Be the editor provider"). Payload field names follow the WIT records
//! (kebab-case); see API_GAPS.md for the wire shapes the page leaves open.

use crate::support::{call_buffers, direct, direct_open, done_json, read_lines, seed_text};
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::types::Event;
use ilium_plugin_sdk::Handler;
use proptest::prelude::*;
use serde_json::json;

fn edit(start: (u32, u32), end: (u32, u32), replacement: &str) -> serde_json::Value {
    json!({
        "range": {
            "start": {"line": start.0, "column": start.1},
            "end": {"line": end.0, "column": end.1},
        },
        "replacement": replacement,
    })
}

/// REQ: F-019
#[test]
fn f_019_list_is_empty_before_any_buffer_opens() {
    let mut editor = direct("{}");
    let listed = done_json(call_buffers(&mut editor, "list", json!({})));
    assert_eq!(listed, json!([]));
}

/// REQ: F-019
#[test]
fn f_019_list_returns_one_buffer_ref_per_open_buffer() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, Some("rust"), 6);
    direct_open(&mut editor, 2, Some("markdown"), 3);
    let listed = done_json(call_buffers(&mut editor, "list", json!({})));
    let entries = listed.as_array().cloned().unwrap_or_default();
    assert_eq!(entries.len(), 2);
    let mut ids: Vec<u64> = entries.iter().filter_map(|entry| entry["buffer"].as_u64()).collect();
    ids.sort();
    assert_eq!(ids, vec![1, 2]);
    let first = entries.iter().find(|entry| entry["buffer"] == 1).cloned().unwrap_or_default();
    assert_eq!(first["language"], "rust");
    assert_eq!(first["line-count"], 6);
    assert_eq!(first["dirty"], false);
}

/// REQ: F-019
#[test]
fn f_019_closed_buffer_leaves_the_list() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    direct_open(&mut editor, 2, None, 1);
    Handler::on_events(&mut editor, vec![Event::EditorClosed(crate::support::buffer_ref(1, None, None, 1))]);
    let listed = done_json(call_buffers(&mut editor, "list", json!({})));
    let entries = listed.as_array().cloned().unwrap_or_default();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["buffer"], 2);
}

/// REQ: F-019
#[test]
fn f_019_read_returns_the_requested_lines() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    seed_text(&mut editor, 1, "alpha\nbeta\ngamma");
    assert_eq!(read_lines(&mut editor, 1, 1, 2), json!(["beta", "gamma"]));
    assert_eq!(read_lines(&mut editor, 1, 0, 1), json!(["alpha"]));
}

/// REQ: F-019
#[test]
fn f_019_read_past_the_end_returns_the_lines_that_exist() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    seed_text(&mut editor, 1, "alpha\nbeta\ngamma");
    assert_eq!(read_lines(&mut editor, 1, 2, 10), json!(["gamma"]));
    assert_eq!(read_lines(&mut editor, 1, 3, 10), json!([]));
}

/// REQ: F-019
#[test]
fn f_019_read_of_zero_lines_is_empty() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    seed_text(&mut editor, 1, "alpha");
    assert_eq!(read_lines(&mut editor, 1, 0, 0), json!([]));
}

/// REQ: F-019
#[test]
fn f_019_replace_range_replaces_the_named_range() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    seed_text(&mut editor, 1, "alpha\nbeta");
    let reply = call_buffers(
        &mut editor,
        "replace-range",
        json!({"buffer": 1, "base-revision": 2, "edits": [edit((0, 0), (0, 5), "ALPHA")]}),
    );
    assert!(matches!(reply, ServiceReply::Done(_)), "replace-range answers at once");
    assert_eq!(read_lines(&mut editor, 1, 0, 2), json!(["ALPHA", "beta"]));
}

/// REQ: F-019
#[test]
fn f_019_replace_range_with_equal_start_and_end_inserts() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    seed_text(&mut editor, 1, "ac");
    call_buffers(
        &mut editor,
        "replace-range",
        json!({"buffer": 1, "base-revision": 2, "edits": [edit((0, 1), (0, 1), "b")]}),
    );
    assert_eq!(read_lines(&mut editor, 1, 0, 1), json!(["abc"]));
}

/// REQ: F-019
#[test]
fn f_019_replace_range_columns_count_unicode_scalar_values() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    seed_text(&mut editor, 1, "\u{e9}1");
    call_buffers(
        &mut editor,
        "replace-range",
        json!({"buffer": 1, "base-revision": 2, "edits": [edit((0, 1), (0, 2), "2")]}),
    );
    assert_eq!(read_lines(&mut editor, 1, 0, 1), json!(["\u{e9}2"]));
}

/// REQ: F-019
#[test]
fn f_019_replace_range_across_lines_joins_them() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    seed_text(&mut editor, 1, "one\ntwo\nthree");
    call_buffers(
        &mut editor,
        "replace-range",
        json!({"buffer": 1, "base-revision": 2, "edits": [edit((0, 3), (1, 0), " ")]}),
    );
    assert_eq!(read_lines(&mut editor, 1, 0, 2), json!(["one two", "three"]));
}

/// REQ: F-019
#[test]
fn f_019_replace_range_with_a_stale_revision_changes_nothing() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    seed_text(&mut editor, 1, "alpha");
    call_buffers(
        &mut editor,
        "replace-range",
        json!({"buffer": 1, "base-revision": 1, "edits": [edit((0, 0), (0, 5), "STALE")]}),
    );
    assert_eq!(read_lines(&mut editor, 1, 0, 1), json!(["alpha"]));
}

/// REQ: F-019
#[test]
fn f_019_replace_range_with_overlapping_edits_changes_nothing() {
    let mut editor = direct("{}");
    direct_open(&mut editor, 1, None, 1);
    seed_text(&mut editor, 1, "abcdef");
    call_buffers(
        &mut editor,
        "replace-range",
        json!({"buffer": 1, "base-revision": 2, "edits": [edit((0, 0), (0, 4), "X"), edit((0, 2), (0, 5), "Y")]}),
    );
    assert_eq!(read_lines(&mut editor, 1, 0, 1), json!(["abcdef"]));
}

/// REQ: F-019
#[test]
fn f_019_read_of_an_unknown_buffer_gives_no_lines() {
    let mut editor = direct("{}");
    assert_eq!(read_lines(&mut editor, 77, 0, 5), json!([]));
}

proptest! {
    // REQ: F-019
    #[test]
    fn f_019_inserted_lines_read_back_unchanged(lines in proptest::collection::vec("[a-z ]{0,20}", 1..6)) {
        let mut editor = direct("{}");
        direct_open(&mut editor, 1, None, 1);
        seed_text(&mut editor, 1, &lines.join("\n"));
        let back = read_lines(&mut editor, 1, 0, lines.len() as u32);
        prop_assert_eq!(back, json!(lines));
    }
}
