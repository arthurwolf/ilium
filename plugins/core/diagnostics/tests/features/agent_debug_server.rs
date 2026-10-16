//! Ported tests of the server-side recorder (ilium-server agent_debug, F-032). The service surface is
//! assumed: API_GAPS.md item 1. Methods: enable, observe, append, replay, snapshot, restore, remove, payload.

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::{Value, json};

use crate::agent_debug_history::{append, entries};
use crate::common::{agent_debug, session};

type Session = ilium_plugin_sdk::testing::FakeSession;

fn enable(session: &mut Session, enabled: bool) {
    agent_debug(session, "enable", json!({ "enabled": enabled })).expect("enable");
}

fn context() -> Value {
    json!({ "class": "codex", "activity": "idle", "process_id": 42, "session_id": "session-a", "title_generation": 3 })
}

fn observe(session: &mut Session, source: &str, kind: &str, summary: &str) -> bool {
    let reply = agent_debug(session, "observe", json!({ "pane": 19, "source": source, "kind": kind, "summary": summary, "context": context() })).expect("observe");
    reply["accepted"].as_bool().expect("accepted flag")
}

/// REQ: F-032
/// REQ: PORT:ilium-server/src/agent_debug.rs::disabled_recorder_does_not_create_a_journal
#[test]
fn f_032_disabled_recorder_does_not_create_a_journal() {
    let mut session = session();
    enable(&mut session, false);
    let reply = append(&mut session, 1, "pty", "prompt-submitted", "hidden prompt");
    assert_eq!(reply["ignored"], true);
    assert!(agent_debug(&mut session, "snapshot", json!({})).expect("snapshot")["panes"].as_array().unwrap().is_empty());
}

/// REQ: F-032
/// REQ: PORT:ilium-server/src/agent_debug.rs::replay_supports_initial_and_incremental_reads
#[test]
fn f_032_replay_supports_initial_and_incremental_reads() {
    let mut session = session();
    enable(&mut session, true);
    for summary in ["first", "second", "third"] {
        append(&mut session, 1, "pty", "prompt-submitted", summary);
    }
    let initial = agent_debug(&mut session, "replay", json!({ "pane": 1 })).expect("replay");
    assert_eq!((initial["through_sequence"].clone(), initial["retained_from"].clone(), initial["dropped"].clone()), (json!(3), json!(1), json!(0)));
    assert_eq!(initial["entries"].as_array().unwrap().len(), 3);
    let incremental = agent_debug(&mut session, "replay", json!({ "pane": 1, "after": 2 })).expect("replay");
    assert_eq!(incremental["entries"].as_array().unwrap().len(), 1);
    assert_eq!(incremental["entries"][0]["summary"], "third");
}

/// REQ: F-032
/// REQ: PORT:ilium-server/src/agent_debug.rs::snapshot_restore_and_remove_follow_the_pane_lifecycle
#[test]
fn f_032_snapshot_restore_and_remove_follow_the_pane_lifecycle() {
    let mut session = session();
    enable(&mut session, true);
    append(&mut session, 1, "server", "agent-detected", "persist me");
    let snapshot = agent_debug(&mut session, "snapshot", json!({})).expect("snapshot");
    agent_debug(&mut session, "remove", json!({ "panes": [1] })).expect("remove");
    let gone = agent_debug(&mut session, "replay", json!({ "pane": 1 })).expect_err("journal removed");
    assert_eq!(gone.code, ErrorCode::NotFound);
    agent_debug(&mut session, "restore", json!({ "snapshot": snapshot })).expect("restore");
    assert_eq!(entries(&mut session)["entries"][0]["summary"], "persist me");
}

/// REQ: F-032
/// REQ: PORT:ilium-server/src/agent_debug.rs::snapshot_preflight_rejects_spare_capacity_without_losing_journal
#[test]
fn f_032_snapshot_preflight_rejects_a_too_small_limit_without_losing_the_journal() {
    let mut session = session();
    enable(&mut session, true);
    agent_debug(&mut session, "append", json!({ "pane": 7, "at": 1, "source": "pty", "kind": "prompt-submitted", "summary": "kept", "reserve": 8192 })).expect("append");
    let refused = agent_debug(&mut session, "snapshot", json!({ "limit": 1024 })).expect_err("limit too small");
    assert_eq!(refused.code, ErrorCode::ResourceExhausted);
    let snapshot = agent_debug(&mut session, "snapshot", json!({ "limit": 16384 })).expect("larger allowance");
    assert_eq!(snapshot["panes"][0]["entries"][0]["summary"], "kept");
    assert_eq!(snapshot["panes"][0]["next_sequence"], 2);
    let replay = agent_debug(&mut session, "replay", json!({ "pane": 7 })).expect("journal kept");
    assert_eq!(replay["entries"].as_array().unwrap().len(), 1);
}

/// REQ: F-032
/// REQ: PORT:ilium-server/src/agent_debug.rs::change_only_filter_ignores_repeated_detection_across_other_events
#[test]
fn f_032_change_only_filter_ignores_repeated_detection_across_other_events() {
    let mut session = session();
    enable(&mut session, false);
    assert!(observe(&mut session, "detector", "detection-cycle", "Codex is idle"));
    assert!(observe(&mut session, "pty", "prompt-submitted", "independent prompt"));
    assert!(!observe(&mut session, "detector", "detection-cycle", "Codex is idle"));
    assert!(observe(&mut session, "detector", "detection-cycle", "Codex is working"));
}

/// REQ: F-032
/// REQ: PORT:ilium-server/src/agent_debug.rs::enabling_the_ui_journal_after_file_logging_still_records_a_baseline
#[test]
fn f_032_enabling_the_ui_journal_after_file_logging_still_records_a_baseline() {
    let mut session = session();
    enable(&mut session, false);
    assert!(observe(&mut session, "detector", "detection-cycle", "Codex is idle"));
    enable(&mut session, true);
    let reply = agent_debug(&mut session, "append", json!({ "pane": 19, "at": 1, "source": "detector", "kind": "detection-cycle", "summary": "Codex is idle", "context": context() })).expect("append");
    assert!(reply.get("sequence").is_some(), "{reply}");
}

/// REQ: F-032
/// REQ: SEC-249
/// REQ: PORT:ilium-server/src/agent_debug.rs::process_log_payload_keeps_context_and_exact_lifecycle_evidence
#[test]
fn f_032_process_log_payload_keeps_context_and_exact_lifecycle_evidence() {
    let mut session = session();
    let reply = agent_debug(&mut session, "payload", json!({
        "pane": 9, "source": "pty", "kind": "prompt-submitted", "summary": "Prompt accepted by the agent PTY",
        "context": { "class": "codex", "activity": "working", "process_id": 77, "session_id": "session-new", "title_generation": 4 },
        "fields": [{ "label": "submitted input", "value": "/clear", "sensitive": true }, { "label": "API key", "value": "field-secret", "sensitive": true }]
    })).expect("payload");
    let payload = reply["payload"].as_str().expect("payload text");
    let parsed: Value = serde_json::from_str(payload).expect("valid JSON payload");
    assert_eq!(parsed["pane_id"], 9);
    assert_eq!(parsed["kind"], "prompt_submitted");
    assert_eq!(parsed["context"]["process_id"], 77);
    assert_eq!(parsed["context"]["session_id"], "session-new");
    assert_eq!(parsed["fields"][0]["value"], "/clear");
    assert_eq!(parsed["fields"][1]["value"], "<redacted>");
    assert!(payload.contains("/clear"));
    assert!(!payload.contains("field-secret"));
}
