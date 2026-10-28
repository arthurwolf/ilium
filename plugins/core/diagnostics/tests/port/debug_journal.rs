//! Twins of `ilium-server/tests/debug_logging_process.rs`, `ilium-server/tests/smoke.rs` (resize cause) and
//! `ilium/tests/pty_tui_smoke.rs` (debug log view and save). The service `ilium.core.diagnostics/agent-debug`
//! is assumed as in the feature tests; the new method shapes are listed in API_GAPS_ports.md.

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::ErrorCode;
use serde_json::{json, Value};

use crate::common::{agent_debug, session};

fn enable(session: &mut FakeSession) {
    agent_debug(session, "enable", json!({ "enabled": true })).expect("enable");
}

fn request_line(session: &mut FakeSession, request: &str, fields: Value) -> Value {
    let mut body = fields;
    body["request"] = json!(request);
    agent_debug(session, "log_request", body).expect("log_request")
}

fn log_text(session: &mut FakeSession) -> String {
    agent_debug(session, "read_log", json!({})).expect("read_log")["text"]
        .as_str()
        .expect("text")
        .to_string()
}

/// REQ: PORT:ilium-server/tests/debug_logging_process.rs::detached_server_process_logs_major_ipc_actions_and_errors
#[test]
fn port_001_server_log_records_major_ipc_actions_and_errors_privately() {
    let mut session = session();
    enable(&mut session);
    request_line(
        &mut session,
        "attach",
        json!({ "session": "debug-session", "ok": true }),
    );
    request_line(
        &mut session,
        "new_group",
        json!({ "name": "logged group", "ok": true }),
    );
    let refused = request_line(
        &mut session,
        "attach",
        json!({ "session": "wrong-session", "ok": false, "error": "session mismatch: wrong-session" }),
    );
    assert_eq!(refused["logged"], true);
    request_line(&mut session, "kill_session", json!({ "ok": true }));
    let text = log_text(&mut session);
    for expected in [
        "ilium-server starting",
        "detached session server listening",
        "request_name=\"attach\"",
        "request_name=\"new_group\"",
        "request_name=\"kill_session\"",
        "request failed",
        "wrong-session",
    ] {
        assert!(text.contains(expected), "log lacks {expected:?}: {text}");
    }
    let directory = agent_debug(
        &mut session,
        "file_mode",
        json!({ "kind": "log-directory" }),
    )
    .expect("directory mode");
    let file =
        agent_debug(&mut session, "file_mode", json!({ "kind": "log-file" })).expect("file mode");
    assert_eq!(
        (directory["mode"].clone(), file["mode"].clone()),
        (json!(0o700), json!(0o600))
    );
}

/// REQ: PORT:ilium-server/tests/smoke.rs::successful_resize_records_its_typed_client_cause_in_the_debug_journal
#[test]
fn port_002_successful_resize_records_its_typed_client_cause_in_the_journal() {
    let mut session = session();
    enable(&mut session);
    let appended = agent_debug(
        &mut session,
        "append",
        json!({ "pane": 5, "at": 1, "source": "pty", "kind": "pane-resized", "summary": "Pane resized to 100x40",
            "resize_cause": "tree-panel-animation" }),
    )
    .expect("append resize");
    assert_eq!(appended["sequence"], 1);
    let listed = agent_debug(&mut session, "entries", json!({ "pane": 5 })).expect("entries");
    let resized: Vec<&Value> = listed["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter(|entry| entry["kind"] == "pane-resized")
        .collect();
    assert_eq!(resized.len(), 1, "{listed}");
    assert_eq!(resized[0]["resize_cause"], "tree-panel-animation");
    assert_eq!(resized[0]["is_tree_panel_animation_resize"], true);
    let fields = resized[0]["fields"].as_array().expect("fields");
    assert_eq!(
        fields
            .iter()
            .filter(|field| field["label"] == "cause" && field["value"] == "tree-panel-animation")
            .count(),
        1,
        "{fields:?}"
    );
}

fn decision_entry(sequence: u64, label: &str, value: &str) -> Value {
    json!({ "sequence": sequence, "at": 1_700_000_000_000_i64 + sequence as i64, "severity": "information",
        "source": "detector", "kind": "detection-cycle", "summary": format!("{label} recorded"),
        "fields": [{ "label": label, "value": value, "sensitive": false }] })
}

fn history() -> Vec<Value> {
    vec![
        decision_entry(
            1,
            "Agent identity decision",
            "Codex because the process name matched",
        ),
        decision_entry(
            2,
            "Activity decision",
            "Working because a live status line is visible",
        ),
        decision_entry(
            3,
            "Goal decision",
            "No goal because no goal line is visible",
        ),
        json!({ "sequence": 4, "at": 1_700_000_000_004_i64, "severity": "information", "source": "pty", "kind": "pane-resized",
            "summary": "Left panel focus/hover animation", "resize_cause": "tree-panel-animation" }),
    ]
}

fn exported(session: &mut FakeSession, path: &str) -> String {
    agent_debug(session, "read_file", json!({ "path": path })).expect("read export")["text"]
        .as_str()
        .expect("text")
        .to_string()
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::agent_debug_log_filters_panel_resizes_and_saves_the_active_view
#[test]
fn port_003_debug_log_filters_panel_resizes_and_saves_the_active_view() {
    let mut session = session();
    let hidden = agent_debug(
        &mut session,
        "view",
        json!({ "entries": history(), "hide_tree_animation": true }),
    )
    .expect("view");
    let text = hidden["text"].as_str().expect("text");
    for expected in [
        "Agent debug log",
        "Save log",
        "Panel resizes hidden",
        "panel resize events hid",
        "DETECTION",
        "Agent identity decision",
        "Activity decision",
        "Goal decision",
    ] {
        assert!(text.contains(expected), "view lacks {expected:?}: {text}");
    }
    assert!(!text.contains("Left panel focus/hover animation"), "{text}");
    assert_eq!(
        (hidden["visible"].clone(), hidden["hidden"].clone()),
        (json!(3), json!(1))
    );

    agent_debug(&mut session, "make_directory", json!({ "path": "exports" })).expect("mkdir");
    let first = agent_debug(
        &mut session,
        "save_log",
        json!({ "pane": 9, "pane_name": "Codex", "path": "exports/first.log", "entries": history(), "hide_tree_animation": true }),
    )
    .expect("first save");
    assert_eq!(first["saved"], true);
    let first_report = exported(&mut session, "exports/first.log");
    assert_eq!(
        first_report.matches("[DETECTION]").count(),
        3,
        "{first_report}"
    );
    assert!(
        first_report.contains("Filter: left-panel focus/hover animation resize events excluded")
    );
    assert!(!first_report.contains("Left panel focus/hover animation"));

    let shown = agent_debug(
        &mut session,
        "view",
        json!({ "entries": history(), "hide_tree_animation": false }),
    )
    .expect("view");
    let shown_text = shown["text"].as_str().expect("text");
    assert!(shown_text.contains("Panel resizes shown"), "{shown_text}");
    assert!(
        shown_text.contains("Left panel focus/hover animation"),
        "{shown_text}"
    );

    let second = agent_debug(
        &mut session,
        "save_log",
        json!({ "pane": 9, "pane_name": "Codex", "path": "exports/second.log", "entries": history(), "hide_tree_animation": false }),
    )
    .expect("second save");
    assert_eq!(second["saved"], true);
    let second_report = exported(&mut session, "exports/second.log");
    assert!(
        second_report.contains("Filter: left-panel focus/hover animation resize events included")
    );
    assert!(second_report.contains("Left panel focus/hover animation"));
    assert_eq!(second_report.matches("[DETECTION]").count(), 3);
    for forbidden in ["phase 1", "request generation", "repetition_count"] {
        assert!(!second_report.contains(forbidden), "{forbidden}");
    }
    let missing = agent_debug(
        &mut session,
        "read_file",
        json!({ "path": "exports/third.log" }),
    )
    .expect_err("no third export");
    assert_eq!(missing.code, ErrorCode::NotFound);
}

/// REQ: PORT:ilium-server/tests/debug_logging_process.rs::detached_server_process_logs_major_ipc_actions_and_errors
#[test]
fn port_001_refused_attach_is_recorded_at_error_level() {
    let mut session = session();
    enable(&mut session);
    let refused = request_line(
        &mut session,
        "attach",
        json!({ "session": "wrong-session", "ok": false, "error": "session mismatch: wrong-session" }),
    );
    assert_eq!(refused["level"], "error", "{refused}");
}

/// REQ: PORT:ilium-server/tests/smoke.rs::successful_resize_records_its_typed_client_cause_in_the_debug_journal
#[test]
fn port_002_host_terminal_resize_is_not_flagged_as_a_tree_animation_resize() {
    let mut session = session();
    enable(&mut session);
    agent_debug(
        &mut session,
        "append",
        json!({ "pane": 6, "at": 1, "source": "pty", "kind": "pane-resized", "summary": "Pane resized to 80x24", "resize_cause": "host-terminal" }),
    )
    .expect("append resize");
    let listed = agent_debug(&mut session, "entries", json!({ "pane": 6 })).expect("entries");
    assert_eq!(listed["entries"][0]["resize_cause"], "host-terminal");
    assert_eq!(listed["entries"][0]["is_tree_panel_animation_resize"], false);
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::agent_debug_log_filters_panel_resizes_and_saves_the_active_view
#[test]
fn port_003_a_later_save_does_not_rewrite_an_earlier_export() {
    let mut session = session();
    agent_debug(&mut session, "make_directory", json!({ "path": "exports" })).expect("mkdir");
    agent_debug(
        &mut session,
        "save_log",
        json!({ "pane": 9, "pane_name": "Codex", "path": "exports/first.log", "entries": history(), "hide_tree_animation": true }),
    )
    .expect("first save");
    let before = exported(&mut session, "exports/first.log");
    agent_debug(
        &mut session,
        "save_log",
        json!({ "pane": 9, "pane_name": "Codex", "path": "exports/second.log", "entries": history(), "hide_tree_animation": false }),
    )
    .expect("second save");
    assert_eq!(exported(&mut session, "exports/first.log"), before);
}
