//! Ported tests of the client side of the agent debug history (ilium-client app, render_cache, ui,
//! F-032). Assumed methods: record, open, mirror, save_prompt. See API_GAPS.md item 1.

use ilium_plugin_sdk::testing::FakeSession;
use serde_json::{Value, json};

use crate::common::{PLUGIN_ID, agent_debug, session};

fn entry(sequence: u64, summary: &str) -> Value {
    json!({ "sequence": sequence, "at": sequence as i64, "source": "detector", "kind": "detection-cycle", "summary": summary })
}

fn record(session: &mut FakeSession, menu_enabled: bool, file_logging: bool) -> Value {
    agent_debug(session, "record", json!({
        "pane": 31, "menu_enabled": menu_enabled, "file_logging_enabled": file_logging,
        "kind": "trigger-evaluated", "summary": "Automatic-work trigger evaluated"
    })).expect("record")
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/app.rs::client_owned_agent_events_are_suppressed_when_both_debug_sinks_are_disabled
#[test]
fn f_032_client_owned_agent_events_are_suppressed_when_both_debug_sinks_are_disabled() {
    let mut session = session();
    let reply = record(&mut session, false, false);
    assert_eq!(reply["forwarded"], false);
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/app.rs::file_logging_alone_forwards_client_owned_agent_events
#[test]
fn f_032_file_logging_alone_forwards_client_owned_agent_events() {
    let mut session = session();
    let reply = record(&mut session, false, true);
    assert_eq!(reply["forwarded"], true);
    assert_eq!(reply["pane"], 31);
    assert_eq!(reply["kind"], "trigger-evaluated");
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/app.rs::opening_debug_log_fetches_retained_history_before_using_incremental_replay
#[test]
fn f_032_opening_debug_log_fetches_retained_history_before_using_incremental_replay() {
    let mut session = session();
    let first = agent_debug(&mut session, "open", json!({ "pane": 7, "through_sequence": 41, "has_loaded_retained_history": false })).expect("open");
    assert_eq!(first["request"], json!({ "pane": 7, "after_sequence": null }));
    let second = agent_debug(&mut session, "open", json!({ "pane": 7, "through_sequence": 41, "has_loaded_retained_history": true })).expect("open");
    assert_eq!(second["request"], json!({ "pane": 7, "after_sequence": 41 }));
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/app.rs::debug_log_save_prompt_uses_an_editable_absolute_path_only_after_full_replay
#[test]
fn f_032_debug_log_save_prompt_uses_an_editable_absolute_path_only_after_full_replay() {
    let mut session = session();
    let prompt = agent_debug(&mut session, "save_prompt", json!({ "pane": 7, "pane_name": "Codex: payment audit", "has_loaded_retained_history": true })).expect("prompt");
    let path = prompt["path"].as_str().expect("path");
    assert!(path.starts_with('/'), "{path}");
    assert!(path.contains("ilium-codex-payment-audit-debug-"), "{path}");
    assert!(path.ends_with(".log"), "{path}");
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/app.rs::saving_agent_debug_log_resolves_relative_path_and_writes_the_complete_report
#[test]
fn f_032_saving_agent_debug_log_resolves_relative_path_and_writes_the_complete_report() {
    let mut session = session();
    let missing = agent_debug(&mut session, "save_log", json!({ "pane": 7, "pane_name": "Codex audit", "path": "exports/codex.log", "entries": [entry(1, "Detected Codex and classified it as working")] })).expect("save_log");
    assert_eq!(missing["saved"], false);
    let message = missing["message"].as_str().unwrap();
    assert!(message.starts_with("Save agent debug log failed for") && message.contains("codex.log"), "{message}");
    agent_debug(&mut session, "make_directory", json!({ "path": "exports" })).expect("mkdir");
    let saved = agent_debug(&mut session, "save_log", json!({ "pane": 7, "pane_name": "Codex audit", "path": "exports/codex.log", "entries": [entry(1, "Detected Codex and classified it as working")] })).expect("save_log");
    assert_eq!(saved["saved"], true);
    assert!(saved["message"].as_str().unwrap().starts_with("Saving agent debug log to"));
    let text = agent_debug(&mut session, "read_file", json!({ "path": "exports/codex.log" })).expect("read")["text"].as_str().unwrap().to_string();
    assert!(text.contains("ILIUM AGENT DEBUG LOG"));
    assert!(text.contains("Pane: Codex audit"));
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/app.rs::oldest_debug_log_anchor_can_page_toward_newer_events
#[test]
fn f_032_oldest_debug_log_anchor_can_page_toward_newer_events() {
    let mut session = session();
    let reply = agent_debug(&mut session, "scroll", json!({ "from_newest": 28, "show_oldest": true, "newer": 10 })).expect("scroll");
    assert_eq!(reply["position"], json!({ "from_oldest": 10 }));
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/render_cache.rs::debug_snapshot_and_live_events_merge_in_sequence_order_without_duplicates
#[test]
fn f_032_debug_snapshot_and_live_events_merge_in_sequence_order_without_duplicates() {
    let mut session = session();
    agent_debug(&mut session, "mirror_snapshot", json!({ "pane": 7, "through_sequence": 3, "retained_from": 1, "dropped": 0, "entries": [entry(1, "first"), entry(3, "third")] })).expect("snapshot");
    agent_debug(&mut session, "mirror_append", json!({ "pane": 7, "entry": entry(2, "second") })).expect("append");
    agent_debug(&mut session, "mirror_append", json!({ "pane": 7, "entry": entry(3, "third replay copy") })).expect("append");
    let cache = agent_debug(&mut session, "mirror", json!({ "pane": 7 })).expect("mirror");
    let sequences: Vec<u64> = cache["entries"].as_array().unwrap().iter().map(|item| item["sequence"].as_u64().unwrap()).collect();
    assert_eq!(sequences, [1, 2, 3]);
    assert_eq!(cache["entries"][2]["summary"], "third replay copy");
    assert_eq!(cache["through_sequence"], 3);
    assert_eq!(cache["is_loading"], false);
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/render_cache.rs::closing_a_pane_closes_its_nested_debug_save_flow_and_cache
#[test]
fn f_032_closing_a_pane_closes_its_nested_debug_save_flow_and_cache() {
    let mut session = session();
    agent_debug(&mut session, "mirror_snapshot", json!({ "pane": 7, "through_sequence": 0, "retained_from": 1, "dropped": 0, "entries": [] })).expect("snapshot");
    agent_debug(&mut session, "pane_closed", json!({ "pane": 7 })).expect("closed");
    let cache = agent_debug(&mut session, "mirror", json!({ "pane": 7 })).expect_err("cache dropped");
    assert_eq!(cache.code, ilium_plugin_sdk::types::ErrorCode::NotFound);
    let _ = PLUGIN_ID;
}

/// REQ: F-032
/// REQ: PORT:ilium-client/src/ui.rs::agent_debug_mode_replaces_the_complete_right_panel_with_aerated_history
#[test]
fn f_032_agent_debug_mode_replaces_the_complete_right_panel_with_aerated_history() {
    let mut session = session();
    agent_debug(&mut session, "mirror_snapshot", json!({ "pane": 7, "through_sequence": 1, "retained_from": 1, "dropped": 0, "entries": [entry(1, "Detected Codex")] })).expect("snapshot");
    let screen = session.client(PLUGIN_ID).render(ilium_plugin_sdk::types::ViewTarget::Pane(ilium_plugin_sdk::types::NodeId(7)), 80, 24).text();
    assert!(screen.contains("Detected Codex"), "{screen}");
}
