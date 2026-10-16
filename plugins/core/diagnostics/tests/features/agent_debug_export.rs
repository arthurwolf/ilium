//! Ported tests of the agent debug report and its viewer text (ilium-client agent_debug_export and
//! agent_debug_ui, F-032). Assumed methods: report, save, sanitize, view, filter. See API_GAPS.md item 1.

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::{Value, json};

use crate::common::{agent_debug, session};

type Session = ilium_plugin_sdk::testing::FakeSession;

fn detection_entry() -> Value {
    json!({
        "sequence": 7, "at": 1_700_000_000_000_i64, "severity": "information", "source": "detector", "kind": "detection-cycle",
        "summary": "Detected Codex and classified it as working",
        "fields": [{ "label": "Activity decision", "value": "Working because the visible terminal contains a live status line.", "sensitive": false }],
        "correlation_id": "submission-7",
        "context": { "class": "codex", "activity": "working", "process_id": 42, "session_id": "session-a", "title_generation": 2 }
    })
}

fn report(session: &mut Session, pane_name: &str, entries: Vec<Value>) -> String {
    let reply = agent_debug(session, "report", json!({ "pane": 12, "pane_name": pane_name, "now": 1_700_000_001_000_i64, "entries": entries })).expect("report");
    reply["text"].as_str().expect("text").to_string()
}

fn resize_entry(sequence: u64, summary: &str, cause: Option<&str>) -> Value {
    json!({ "sequence": sequence, "at": 1_700_000_000_000_i64, "severity": "information", "source": "pty", "kind": "pane-resized", "summary": summary, "resize_cause": cause })
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/agent_debug_export.rs::report_is_complete_human_readable_and_omits_internal_heartbeat_counters
#[test]
fn f_032_report_is_complete_human_readable_and_omits_internal_heartbeat_counters() {
    let mut session = session();
    let text = report(&mut session, "Codex pane", vec![detection_entry()]);
    assert!(text.contains("Detection policy: a detection decision is recorded only when"));
    assert!(text.contains("[DETECTION]  Detected Codex"));
    assert!(text.contains("Observed by: Agent detection engine"));
    assert!(text.contains("Context: Codex \u{b7} working \u{b7} process 42 \u{b7} session session-a"));
    assert!(text.contains("Correlation: submission-7"));
    assert!(text.contains("Activity decision: Working because"));
    for forbidden in ["phase 1", "request generation", "repetition_count"] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/agent_debug_export.rs::report_keeps_untrusted_text_on_one_line_and_strips_escapes
#[test]
fn f_032_report_keeps_untrusted_text_on_one_line_and_strips_escapes() {
    let mut session = session();
    let entry = json!({
        "sequence": 1, "at": 1_700_000_000_000_i64, "severity": "information", "source": "detector", "kind": "detection-cycle",
        "summary": "line one\nline two", "fields": [],
        "context": { "class_other": "evil\u{1b}[31m\nagent", "session_id": "id\nwith newline", "title_generation": 0 }
    });
    let text = report(&mut session, "pane\nname\u{1b}[2J", vec![entry]);
    assert!(text.contains("Pane: pane name\u{fffd}[2J\n"));
    assert!(text.contains("[DETECTION]  line one line two\n"));
    assert!(text.contains("Context: evil\u{fffd}[31m agent \u{b7} session id with newline\n"));
    assert!(!text.contains('\u{1b}'));
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/agent_debug_export.rs::report_applies_the_same_panel_resize_filter_as_the_viewer
#[test]
fn f_032_report_applies_the_same_panel_resize_filter_as_the_viewer() {
    let mut session = session();
    let entries = vec![resize_entry(1, "tree animation resize", Some("tree-panel-animation")), resize_entry(2, "host terminal resize", Some("host-terminal")), resize_entry(3, "legacy resize", None)];
    let text = report(&mut session, "pane", entries);
    assert!(!text.contains("tree animation resize"));
    assert!(text.contains("host terminal resize"));
    assert!(text.contains("legacy resize"));
}

fn save(session: &mut Session, path: &str, text: &str) -> Result<Value, ilium_plugin_sdk::types::PluginError> {
    agent_debug(session, "save", json!({ "scope": "path:/tmp/agent-debug-test", "path": path, "text": text }))
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/agent_debug_export.rs::saved_report_is_private
#[test]
fn f_032_saved_report_is_private() {
    let mut session = session();
    save(&mut session, "agent-debug.log", "new report\n").expect("save report");
    let mode = agent_debug(&mut session, "file_mode", json!({ "scope": "path:/tmp/agent-debug-test", "path": "agent-debug.log" })).expect("mode");
    assert_eq!(mode["mode"], 0o600);
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/agent_debug_export.rs::saved_report_refuses_a_symlink_destination
#[test]
fn f_032_saved_report_refuses_a_symlink_destination() {
    let mut session = session();
    session.client("ilium.core.diagnostics").fs().write("path:/tmp/agent-debug-test:target.log", b"must remain unchanged");
    agent_debug(&mut session, "link", json!({ "scope": "path:/tmp/agent-debug-test", "path": "agent-debug.log", "target": "target.log" })).expect("link");
    let error = save(&mut session, "agent-debug.log", "private evidence\n").expect_err("symlink refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let target = session.client("ilium.core.diagnostics").fs().read("path:/tmp/agent-debug-test:target.log").expect("target");
    assert_eq!(target, b"must remain unchanged");
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/agent_debug_export.rs::saved_report_replaces_existing_content
#[test]
fn f_032_saved_report_replaces_existing_content() {
    let mut session = session();
    save(&mut session, "agent-debug.log", "older and longer content").expect("seed");
    save(&mut session, "agent-debug.log", "new report\n").expect("save report");
    let read = agent_debug(&mut session, "read_file", json!({ "scope": "path:/tmp/agent-debug-test", "path": "agent-debug.log" })).expect("read");
    assert_eq!(read["text"], "new report\n");
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/agent_debug_ui.rs::default_filter_hides_only_typed_tree_panel_animation_resizes
#[test]
fn f_032_default_filter_hides_only_typed_tree_panel_animation_resizes() {
    let mut session = session();
    let entries = vec![resize_entry(1, "tree animation resize", Some("tree-panel-animation")), resize_entry(2, "host terminal resize", Some("host-terminal")), resize_entry(3, "legacy resize", None)];
    let filtered = agent_debug(&mut session, "view", json!({ "entries": entries, "hide_tree_animation": true })).expect("view");
    let text = filtered["text"].as_str().unwrap();
    assert!(!text.contains("tree animation resize"));
    assert!(text.contains("host terminal resize") && text.contains("legacy resize"));
    assert_eq!((filtered["visible"].clone(), filtered["hidden"].clone()), (json!(2), json!(1)));
    let unfiltered = agent_debug(&mut session, "view", json!({ "entries": entries, "hide_tree_animation": false })).expect("view");
    assert!(unfiltered["text"].as_str().unwrap().contains("tree animation resize"));
}

fn sanitize(session: &mut Session, mode: &str, text: &str) -> String {
    agent_debug(session, "sanitize", json!({ "mode": mode, "text": text })).expect("sanitize")["text"].as_str().unwrap().to_string()
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/agent_debug_ui.rs::single_line_sanitizer_collapses_line_breaks_and_tabs
#[test]
fn f_032_single_line_sanitizer_collapses_line_breaks_and_tabs() {
    let mut session = session();
    assert_eq!(sanitize(&mut session, "single-line", "multi\nline\tsummary\u{1b}[31m"), "multi line summary\u{fffd}[31m");
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/agent_debug_ui.rs::terminal_control_sequences_are_rendered_inert
#[test]
fn f_032_terminal_control_sequences_are_rendered_inert() {
    let mut session = session();
    assert_eq!(sanitize(&mut session, "terminal", "safe\u{1b}[31m red"), "safe\u{fffd}[31m red");
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/agent_debug_ui.rs::timestamps_use_european_date_order
#[test]
fn f_032_timestamps_use_european_date_order() {
    let mut session = session();
    let reply = agent_debug(&mut session, "format_timestamp", json!({ "at": 1_700_000_000_000_i64 })).expect("format");
    assert!(reply["text"].as_str().unwrap().starts_with("2023-11-"));
}
