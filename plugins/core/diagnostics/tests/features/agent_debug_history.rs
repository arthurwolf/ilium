//! Ported tests of the per-pane agent debug history (crate ilium-agent-debug, F-032). The service
//! surface is assumed: see API_GAPS.md item 1. Constants come from the original: 5000 entries, 4 MiB.

use proptest::prelude::*;
use serde_json::{json, Value};

use crate::common::{agent_debug, session};

const MAXIMUM_ENTRIES: u64 = 5_000;
const MAXIMUM_BYTES: u64 = 4 * 1024 * 1024;

pub fn append(
    session: &mut ilium_plugin_sdk::testing::FakeSession,
    at: i64,
    source: &str,
    kind: &str,
    summary: &str,
) -> Value {
    agent_debug(
        session,
        "append",
        json!({ "pane": 1, "at": at, "source": source, "kind": kind, "summary": summary }),
    )
    .expect("append")
}

pub fn entries(session: &mut ilium_plugin_sdk::testing::FakeSession) -> Value {
    agent_debug(session, "entries", json!({ "pane": 1 })).expect("entries")
}

fn merge(session: &mut ilium_plugin_sdk::testing::FakeSession, sequence: u64, summary: &str) {
    agent_debug(session, "merge", json!({ "pane": 1, "entry": { "sequence": sequence, "at": sequence as i64, "source": "detector", "kind": "activity-changed", "summary": summary } })).expect("merge");
}

/// REQ: F-032
#[test]
fn f_032_append_assigns_monotonic_sequences_and_round_trips() {
    let mut session = session();
    let first = append(
        &mut session,
        1_700_000_000_000,
        "server",
        "agent-detected",
        "Codex detected",
    );
    let second = append(
        &mut session,
        1_700_000_000_001,
        "pty",
        "prompt-submitted",
        "Prompt submitted",
    );
    assert_eq!(
        (first["sequence"].as_u64(), second["sequence"].as_u64()),
        (Some(1), Some(2))
    );
    let saved = agent_debug(&mut session, "snapshot", json!({ "pane": 1 })).expect("snapshot");
    let before = entries(&mut session);
    agent_debug(
        &mut session,
        "restore",
        json!({ "pane": 1, "snapshot": saved }),
    )
    .expect("restore");
    let after = entries(&mut session);
    assert_eq!(before["entries"], after["entries"]);
    assert_eq!(after["next_sequence"], 3);
}

/// REQ: F-032
#[test]
fn f_032_changed_detection_decision_appends_a_new_entry() {
    let mut session = session();
    for summary in ["Codex is working", "Codex is waiting for approval"] {
        let reply = append(&mut session, 1, "detector", "detection-cycle", summary);
        assert!(reply.get("ignored").is_none(), "{reply}");
    }
    let listed = entries(&mut session);
    assert_eq!(listed["entries"].as_array().unwrap().len(), 2);
    assert_eq!(listed["entries"][1]["sequence"], 2);
}

/// REQ: F-032
#[test]
fn f_032_unchanged_detection_polls_are_ignored_without_mutating_history() {
    let mut session = session();
    let first = append(
        &mut session,
        1_700_000_000_000,
        "detector",
        "detection-cycle",
        "Agent remains idle",
    );
    let repeated = append(
        &mut session,
        1_700_000_001_000,
        "detector",
        "detection-cycle",
        "Agent remains idle",
    );
    assert!(first.get("sequence").is_some());
    assert_eq!(repeated["ignored"], true);
    let listed = entries(&mut session);
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    assert_eq!(listed["entries"][0]["at"], 1_700_000_000_000_i64);
    assert_eq!(listed["next_sequence"], 2);
}

/// REQ: F-032
#[test]
fn f_032_unchanged_detection_is_ignored_across_intervening_non_detection_events() {
    let mut session = session();
    assert!(append(
        &mut session,
        1,
        "detector",
        "detection-cycle",
        "Codex is working"
    )
    .get("sequence")
    .is_some());
    assert!(append(
        &mut session,
        2,
        "pty",
        "prompt-submitted",
        "Prompt submitted"
    )
    .get("sequence")
    .is_some());
    let repeated = append(
        &mut session,
        3,
        "detector",
        "detection-cycle",
        "Codex is working",
    );
    assert_eq!(repeated["ignored"], true);
    let listed = entries(&mut session);
    assert_eq!(listed["entries"].as_array().unwrap().len(), 2);
    assert_eq!(listed["next_sequence"], 3);
}

/// REQ: F-032
#[test]
fn f_032_retention_drops_oldest_entries_without_rescanning_the_journal() {
    let mut session = session();
    for sequence in 0..=MAXIMUM_ENTRIES {
        append(
            &mut session,
            sequence as i64,
            "detector",
            "activity-changed",
            &format!("event {sequence}"),
        );
    }
    let listed = entries(&mut session);
    assert_eq!(
        listed["entries"].as_array().unwrap().len() as u64,
        MAXIMUM_ENTRIES
    );
    assert_eq!(listed["dropped"], 1);
    assert_eq!(listed["entries"][0]["sequence"], 2);
}

/// REQ: F-032
#[test]
fn f_032_merge_synced_entry_inserts_out_of_order_and_replaces_duplicates() {
    let mut session = session();
    merge(&mut session, 1, "first");
    merge(&mut session, 3, "third");
    merge(&mut session, 2, "second");
    merge(&mut session, 3, "third replay copy");
    let listed = entries(&mut session);
    let sequences: Vec<u64> = listed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["sequence"].as_u64().unwrap())
        .collect();
    assert_eq!(sequences, [1, 2, 3]);
    assert_eq!(listed["entries"][2]["summary"], "third replay copy");
}

/// REQ: F-032
#[test]
fn f_032_merge_synced_entry_enforces_the_same_retention_cap_as_append_alone() {
    let mut session = session();
    for sequence in 1..=(MAXIMUM_ENTRIES + 50) {
        merge(&mut session, sequence, &format!("event {sequence}"));
    }
    let listed = entries(&mut session);
    assert_eq!(
        listed["entries"].as_array().unwrap().len() as u64,
        MAXIMUM_ENTRIES
    );
    assert_eq!(listed["dropped"], 50);
    assert_eq!(listed["retained_from"], 51);
    assert!(listed["retained_bytes"].as_u64().unwrap() <= MAXIMUM_BYTES);
}

/// REQ: F-032
#[test]
fn f_032_restore_recomputes_missing_byte_metadata_and_enforces_the_size_cap() {
    let mut session = session();
    let large = "x".repeat((MAXIMUM_BYTES / 2) as usize);
    for sequence in 0..3 {
        agent_debug(&mut session, "append", json!({ "pane": 1, "at": sequence, "source": "client", "kind": "title-inference-requested", "summary": "large request", "fields": [{ "label": "request", "value": large, "sensitive": true }] })).expect("append");
    }
    let mut snapshot =
        agent_debug(&mut session, "snapshot", json!({ "pane": 1 })).expect("snapshot");
    snapshot["retained_bytes"] = json!(0);
    agent_debug(
        &mut session,
        "restore",
        json!({ "pane": 1, "snapshot": snapshot }),
    )
    .expect("restore");
    let listed = entries(&mut session);
    assert!(listed["retained_bytes"].as_u64().unwrap() <= MAXIMUM_BYTES);
    assert!(listed["dropped"].as_u64().unwrap() > 0);
}

/// REQ: F-032
#[test]
fn f_032_retain_from_sequence_keeps_byte_accounting_consistent_with_a_trimmed_empty_log() {
    let mut session = session();
    for sequence in 1..=3 {
        merge(&mut session, sequence, &format!("event {sequence}"));
    }
    agent_debug(
        &mut session,
        "retain_from",
        json!({ "pane": 1, "sequence": 100 }),
    )
    .expect("retain_from");
    let listed = entries(&mut session);
    assert!(listed["entries"].as_array().unwrap().is_empty());
    assert_eq!(listed["retained_bytes"], 0);
    let appended = append(
        &mut session,
        1_700_000_000_000,
        "detector",
        "activity-changed",
        "event after trim",
    );
    assert_eq!(appended["sequence"], 4);
    assert_eq!(
        entries(&mut session)["entries"].as_array().unwrap().len(),
        1
    );
}

/// REQ: F-032
#[test]
fn f_032_resize_provenance_round_trips_and_identifies_only_tree_panel_animation_noise() {
    let mut session = session();
    for cause in ["tree-panel-animation", "host-terminal"] {
        agent_debug(&mut session, "append", json!({ "pane": 1, "at": 1_700_000_000_000_i64, "source": "pty", "kind": "pane-resized", "summary": "Agent PTY resized", "resize_cause": cause })).expect("append");
    }
    let saved = agent_debug(&mut session, "snapshot", json!({ "pane": 1 })).expect("snapshot");
    agent_debug(
        &mut session,
        "restore",
        json!({ "pane": 1, "snapshot": saved }),
    )
    .expect("restore");
    let listed = entries(&mut session);
    assert_eq!(listed["entries"][0]["tree_animation_resize"], true);
    assert_eq!(listed["entries"][1]["tree_animation_resize"], false);
}

/// REQ: F-032
#[test]
fn f_032_entries_from_before_resize_provenance_remain_visible() {
    let mut session = session();
    agent_debug(&mut session, "restore", json!({ "pane": 1, "snapshot": { "entries": [{ "sequence": 1, "at": 1_700_000_000_000_i64, "source": "pty", "kind": "pane-resized", "summary": "Legacy PTY resize" }] } })).expect("restore");
    let listed = entries(&mut session);
    assert_eq!(listed["entries"][0]["summary"], "Legacy PTY resize");
    assert_eq!(listed["entries"][0]["tree_animation_resize"], false);
}

proptest! {
    // REQ: F-032
    #[test]
    fn f_032_sequences_stay_strictly_increasing_for_any_distinct_summaries(count in 1usize..40) {
        let mut session = session();
        for index in 0..count {
            append(&mut session, index as i64, "detector", "activity-changed", &format!("distinct {index}"));
        }
        let listed = entries(&mut session);
        let sequences: Vec<u64> = listed["entries"].as_array().unwrap().iter().map(|entry| entry["sequence"].as_u64().unwrap()).collect();
        prop_assert!(sequences.windows(2).all(|pair| pair[0] < pair[1]));
        prop_assert_eq!(sequences.len(), count);
    }
}
