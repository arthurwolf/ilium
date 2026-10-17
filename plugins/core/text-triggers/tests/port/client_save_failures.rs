//! Twins of the text trigger save tests of ilium-client/src/keys.rs: a failed durable save keeps the edit
//! and the stored table, a retry keeps the rule id, and a save never overwrites unrelated rules changed on disk.
//! Assumed row key `reply-N` (Changed, text) beside the documented `delay-N`; see API_GAPS.md.

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, PluginError};
use serde_json::json;

use crate::common::*;
use crate::support::*;

fn refuse_next_settings_write(host: &mut ilium_plugin_sdk::testing::FakeHost) {
    host.inject_failure(
        "kernel-settings.write",
        PluginError { code: ErrorCode::Internal, message: "disk full".to_string(), retry_after_ms: None },
    );
}

/// REQ: F-007, PORT:ilium-client/src/keys.rs::text_trigger_disk_save_failure_keeps_draft_and_durable_bytes
#[test]
fn port_010_disk_save_failure_keeps_the_stored_table_and_sends_no_request() {
    let mut host = client_host();
    set_rules(&mut host, &["unsaved-pattern"], &["old-message"]);
    let stored = host.kernel_settings().tab("text_triggers").expect("table stored");
    let requests_before = host.client_requests().sent().len();
    refuse_next_settings_write(&mut host);
    host.send_action(tab(), "reply-0", ActionKind::Changed, ActionValue::Text("unsaved-message".to_string()));
    host.settle();
    assert_eq!(host.kernel_settings().tab("text_triggers"), Some(stored));
    assert_eq!(host.client_requests().sent().len(), requests_before);
    let notice = host.last_notice().expect("a notice tells the save failed");
    assert!(notice.contains("save"), "{notice}");
}

/// REQ: F-007, PORT:ilium-client/src/keys.rs::text_trigger_failed_save_is_painted_and_retry_keeps_the_draft_identity
#[test]
fn port_011_failed_save_is_painted_and_the_retry_keeps_the_rule_identity() {
    let mut host = client_host();
    set_rules(&mut host, &["a"], &["r"]);
    host.settings().set("ids", json!(["authored-rule"])).expect("id");
    refuse_next_settings_write(&mut host);
    host.send_action(tab(), "reply-0", ActionKind::Changed, ActionValue::Text("kept".to_string()));
    host.settle();
    for columns in [80, 120] {
        let text = render_tab(&mut host, columns);
        assert!(text.contains("not saved"), "{columns} columns: {text}");
    }
    host.send_action(tab(), "reply-0", ActionKind::Changed, ActionValue::Text("kept".to_string()));
    host.settle();
    let table = host.kernel_settings().tab("text_triggers").expect("table");
    assert_eq!(table["ids"], json!(["authored-rule"]));
    assert_eq!(table["replies"], json!(["kept"]));
    assert_eq!(table["patterns"], json!(["a"]));
}

/// REQ: F-007, PORT:ilium-client/src/keys.rs::editing_one_rule_preserves_unrelated_concurrent_changes_and_additions
#[test]
fn port_012_editing_one_rule_preserves_unrelated_concurrent_changes_and_additions() {
    let mut host = client_host();
    host.kernel_settings().set_tab(
        "text_triggers",
        json!({ "ids": ["edited", "other"], "patterns": ["edited-pattern", "other-pattern"], "replies": ["edited-message", "old-message"] }),
    );
    host.settle();
    // Another writer changes the file after the tab was opened: other changed, added appended, edited moved last.
    host.kernel_settings().set_tab(
        "text_triggers",
        json!({
            "ids": ["other", "added", "edited"],
            "patterns": ["other-pattern", "added-pattern", "edited-pattern"],
            "replies": ["new-message", "added-message", "edited-message"],
        }),
    );
    host.send_action(tab(), "reply-0", ActionKind::Changed, ActionValue::Text("unsaved-message".to_string()));
    host.settle();
    let table = host.kernel_settings().tab("text_triggers").expect("table");
    assert_eq!(table["ids"], json!(["other", "added", "edited"]));
    assert_eq!(table["patterns"], json!(["other-pattern", "added-pattern", "edited-pattern"]));
    assert_eq!(table["replies"], json!(["new-message", "added-message", "unsaved-message"]));
}
