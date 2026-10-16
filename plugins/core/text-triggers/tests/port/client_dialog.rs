//! Twins of the client dialog, app and ui tests: the Text Triggers tab rows for delay and target, saving, and the match marking.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, Event, Outcome, ViewTarget};
use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/text_trigger_dialog.rs::a_new_trigger_starts_at_sixty_seconds
#[test]
fn port_001_a_new_trigger_starts_at_sixty_seconds() {
    let mut host = client_host();
    set_rules(&mut host, &["a"], &["r"]);
    assert_eq!(host.settings().get("delays"), Some(json!([60])));
    assert!(render_tab(&mut host, 80).contains("60"));
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog.rs::an_existing_trigger_keeps_its_delay_and_blank_means_default
#[test]
fn port_002_an_existing_trigger_keeps_its_delay_and_blank_means_default() {
    let mut host = client_host();
    set_rules(&mut host, &["a"], &["r"]);
    host.settings().set("delays", json!([90])).expect("delay");
    host.send_action(tab(), "delay-0", ActionKind::Changed, ActionValue::Text(String::new()));
    host.settle();
    assert_eq!(host.settings().get("delays"), Some(json!([DEFAULT_DELAY_SECONDS])));
    host.settings().set("delays", json!([90])).expect("delay");
    assert_eq!(host.settings().get("delays"), Some(json!([90])));
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog.rs::delay_buttons_preserve_authored_rule_and_stop_at_domain_bounds
#[test]
fn port_003_delay_buttons_preserve_authored_rule_and_stop_at_domain_bounds() {
    let mut host = client_host();
    set_rules(&mut host, &["a"], &["r"]);
    host.send_action(tab(), "delay-0", ActionKind::Changed, ActionValue::Number(59.0));
    host.settle();
    assert_eq!(host.settings().get("delays"), Some(json!([59])));
    host.send_action(tab(), "delay-0", ActionKind::Changed, ActionValue::Number(-1.0));
    host.settle();
    assert_eq!(host.settings().get("delays"), Some(json!([0])));
    host.send_action(tab(), "delay-0", ActionKind::Changed, ActionValue::Number(MAX_DELAY_SECONDS as f64 + 1.0));
    host.settle();
    assert_eq!(host.settings().get("delays"), Some(json!([MAX_DELAY_SECONDS])));
    assert_eq!(host.settings().get("patterns"), Some(json!(["a"])));
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog.rs::delay_star_retains_exact_draft_and_blank_steps_from_native_default
#[test]
fn port_004_delay_star_retains_exact_draft_and_blank_steps_from_native_default() {
    let mut host = client_host();
    set_rules(&mut host, &["a"], &["r"]);
    host.send_action(tab(), "delay-0", ActionKind::Changed, ActionValue::Text("7".to_string()));
    host.settle();
    assert_eq!(host.settings().get("delays"), Some(json!([7])));
    host.send_action(tab(), "delay-0", ActionKind::Changed, ActionValue::Text(String::new()));
    host.settle();
    assert_eq!(host.settings().get("delays"), Some(json!([60])));
}

/// REQ: PORT:ilium-client/src/text_trigger_dialog.rs::target_selector_paints_and_hits_both_directions_and_catalogue
#[test]
fn port_005_target_selector_paints_and_hits_both_directions_and_catalogue() {
    let mut host = client_host();
    set_rules(&mut host, &["a"], &["r"]);
    for (index, name) in ["agents", "terminals", "both"].into_iter().enumerate() {
        host.send_action(tab(), "target-0", ActionKind::Selected, ActionValue::Index(index as u32));
        host.settle();
        assert_eq!(host.settings().get("targets"), Some(json!([name])));
        assert!(render_tab(&mut host, 80).to_lowercase().contains(name));
    }
}

/// REQ: PORT:ilium-client/src/app.rs::acknowledged_exact_text_trigger_save_still_closes_only_its_dialog
#[test]
fn port_006_acknowledged_exact_text_trigger_save_still_closes_only_its_dialog() {
    let mut host = client_host();
    set_rules(&mut host, &["a", "b"], &["r", "s"]);
    host.send_action(tab(), "rule-1", ActionKind::Toggled, ActionValue::Flag(false));
    host.settle();
    assert_eq!(host.settings().get("enabled"), Some(json!([true, false])));
    assert_eq!(host.settings().get("patterns"), Some(json!(["a", "b"])));
    assert!(host.outcomes().iter().all(|o| matches!(o, Outcome::Ok)));
}

/// REQ: PORT:ilium-client/src/app.rs::editing_text_trigger_preserves_its_id_persists_and_queues_live_update
#[test]
fn port_007_editing_text_trigger_preserves_its_id_persists_and_queues_live_update() {
    let mut host = client_host();
    set_rules(&mut host, &["a"], &["r"]);
    host.settings().set("ids", json!(["authored-rule"])).expect("id");
    host.settings().set("patterns", json!(["edited"])).expect("edit");
    host.settle();
    assert_eq!(host.settings().get("ids"), Some(json!(["authored-rule"])));
    assert_eq!(host.kernel_settings().tab("text_triggers").expect("table")["patterns"], json!(["edited"]));
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::text_trigger_modal_waits_for_real_durable_writer_and_server_request
#[test]
fn port_008_text_trigger_modal_waits_for_real_durable_writer_and_server_request() {
    let mut host = client_host();
    host.revoke("settings.write-kernel:text-triggers");
    host.send_action(tab(), "rule-0", ActionKind::Toggled, ActionValue::Flag(false));
    host.settle();
    assert!(host.outcomes().iter().any(|o| matches!(o, Outcome::Refused(e) if e.code == ErrorCode::NotPermitted)));
    assert_eq!(host.kernel_settings().tab("text_triggers"), None);
}

/// REQ: PORT:ilium-client/src/ui.rs::text_trigger_dialog_visually_marks_matches_and_loop_risk
#[test]
fn port_009_text_trigger_dialog_visually_marks_matches_and_loop_risk() {
    let mut host = client_host();
    set_rules(&mut host, &["y"], &["y"]);
    let reply = preview(&mut host, "y", "y", "yes\nno");
    let text = reply["preview"].as_str().expect("preview text");
    assert!(text.contains("\u{2713} [y]es"), "{text}");
    assert!(text.contains("\u{b7} no"), "{text}");
    assert!(text.contains("\u{26a0} Reply also matches this regexp; echoed input can loop."), "{text}");
}

