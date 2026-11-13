//! Twin of the execution lease: with two clients only one answers, because the host dedups a send-input by key.

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, Outcome};
use serde_json::json;

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/trigger_execution_lease.rs::only_one_client_executes_and_another_takes_over_after_drop
#[test]
fn port_001_only_one_client_executes_and_another_takes_over_after_drop() {
    let (mut first, pane) = armed("go", "yes");
    let (mut second, second_pane) = armed("go", "yes");
    let one = first.deliver_one(hit(pane, 1));
    let again = first.deliver_one(hit(pane, 1));
    assert_eq!(texts(&one), ["yes"]);
    assert_eq!(
        first
            .outcomes()
            .iter()
            .filter(|o| matches!(o, Outcome::Duplicate(_)))
            .count(),
        1,
        "{again:?}"
    );
    let takeover = second.deliver_one(hit(second_pane, 1));
    assert_eq!(texts(&takeover), ["yes"]);
}

/// REQ: PORT:ilium-client/src/keys.rs::text_trigger_actual_delete_key_persists_an_explicit_empty_list
#[test]
fn port_002_text_trigger_actual_delete_key_persists_an_explicit_empty_list() {
    let mut host = client_host();
    set_rules(&mut host, &["only"], &["reply"]);
    // The Delete chord on the focused last rule removes it.
    host.key(tab(), "delete");
    host.settle();
    assert_eq!(
        host.settings().get("patterns"),
        Some(json!([])),
        "an explicit empty list, not a missing key"
    );
    assert_eq!(
        host.kernel_settings()
            .tab("text_triggers")
            .expect("table written")["patterns"],
        json!([])
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::text_trigger_save_refusal_keeps_the_actual_draft_and_parent_settings
#[test]
fn port_003_text_trigger_save_refusal_keeps_the_actual_draft_and_parent_settings() {
    let mut host = client_host();
    set_rules(&mut host, &["kept"], &["reply"]);
    host.settings().set("ids", json!(["edited"])).expect("id");
    host.settle();
    // The rule disappears elsewhere while the draft is open; saving the draft is refused.
    host.send_action(
        tab(),
        "pattern",
        ActionKind::Changed,
        ActionValue::Text("unsaved-pattern".to_string()),
    );
    host.settings()
        .set("patterns", json!([]))
        .expect("deleted elsewhere");
    host.settings()
        .set("replies", json!([]))
        .expect("deleted elsewhere");
    host.settle();
    host.send_action(tab(), "save", ActionKind::Submitted, ActionValue::None);
    host.settle();
    assert!(host
        .outcomes()
        .iter()
        .any(|o| matches!(o, Outcome::Refused(e) if e.code == ErrorCode::Conflict)));
    assert_eq!(
        host.settings().get("patterns"),
        Some(json!([])),
        "the refused save wrote nothing"
    );
    let text = render_tab(&mut host, 100);
    assert!(
        text.contains("unsaved-pattern"),
        "the draft is kept: {text}"
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::same_id_concurrent_change_refuses_actual_save_and_preserves_both_versions
#[test]
fn port_004_same_id_concurrent_change_refuses_actual_save_and_preserves_both_versions() {
    let mut host = client_host();
    set_rules(&mut host, &["theirs"], &["reply"]);
    host.settings().set("ids", json!(["edited"])).expect("id");
    host.settle();
    host.send_action(
        tab(),
        "pattern",
        ActionKind::Changed,
        ActionValue::Text("mine".to_string()),
    );
    host.settings()
        .set("patterns", json!(["changed-elsewhere"]))
        .expect("concurrent change");
    host.settle();
    host.send_action(tab(), "save", ActionKind::Submitted, ActionValue::None);
    host.settle();
    assert!(host
        .outcomes()
        .iter()
        .any(|o| matches!(o, Outcome::Refused(e) if e.code == ErrorCode::Conflict)));
    assert_eq!(
        host.settings().get("patterns"),
        Some(json!(["changed-elsewhere"])),
        "the stored version survives"
    );
    assert_eq!(host.settings().get("ids"), Some(json!(["edited"])));
    let text = render_tab(&mut host, 100);
    assert!(text.contains("mine"), "the draft version survives: {text}");
}
