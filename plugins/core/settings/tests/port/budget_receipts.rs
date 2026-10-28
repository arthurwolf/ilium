//! Twins of the inference budget receipt tests of ilium-client/src/keys.rs (`inference_receipt_regressions`):
//! a repeated Enter on a typed budget is one write, a late autosave never closes a newer edit, invalid input
//! never writes, and a failed or lost write keeps the exact draft. Writes are observed on the settings store.

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, PluginError};
use serde_json::json;

use crate::common::*;

const KEY: &str = "inference.restructure_prompt_token_limit";

fn writes_of_budget(host: &mut ilium_plugin_sdk::testing::FakeHost) -> Vec<serde_json::Value> {
    host.settings()
        .writes()
        .into_iter()
        .filter(|(key, _)| key == KEY)
        .map(|(_, value)| value)
        .collect()
}

/// REQ: PORT:ilium-client/src/keys.rs::own_durable_receipt_closes_exact_parent_and_repeated_enter_is_finite
#[test]
fn port_005_repeated_enter_on_a_typed_budget_makes_exactly_one_durable_write() {
    let mut host = client_host();
    host.send_action(
        tab("inference"),
        KEY,
        ActionKind::Changed,
        ActionValue::Number(345678.0),
    );
    for _ in 0..1000 {
        host.send_action(
            tab("inference"),
            KEY,
            ActionKind::Submitted,
            ActionValue::None,
        );
    }
    host.settle();
    assert_eq!(writes_of_budget(&mut host), vec![json!(345678)]);
    assert_eq!(value_of(&mut host, KEY), Some(json!(345678)));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/keys.rs::only_latest_explicit_commit_may_close_during_autosave_races
#[test]
fn port_006_an_autosave_followed_by_enter_ends_with_the_typed_value_stored() {
    let mut host = client_host();
    host.send_action(
        tab("inference"),
        KEY,
        ActionKind::Changed,
        ActionValue::Number(123456.0),
    );
    host.advance_clock(600);
    host.send_action(
        tab("inference"),
        KEY,
        ActionKind::Submitted,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(value_of(&mut host, KEY), Some(json!(123456)));
    assert_eq!(writes_of_budget(&mut host).last(), Some(&json!(123456)));
}

/// REQ: PORT:ilium-client/src/keys.rs::only_latest_explicit_commit_may_close_during_autosave_races
#[test]
fn port_007_an_enter_followed_by_a_further_edit_stores_the_latest_value() {
    let mut host = client_host();
    host.send_action(
        tab("inference"),
        KEY,
        ActionKind::Changed,
        ActionValue::Number(123456.0),
    );
    host.send_action(
        tab("inference"),
        KEY,
        ActionKind::Submitted,
        ActionValue::None,
    );
    host.send_action(
        tab("inference"),
        KEY,
        ActionKind::Changed,
        ActionValue::Number(1234567.0),
    );
    host.advance_clock(600);
    host.settle();
    assert_eq!(value_of(&mut host, KEY), Some(json!(1234567)));
    assert_eq!(writes_of_budget(&mut host).last(), Some(&json!(1234567)));
}

/// REQ: PORT:ilium-client/src/keys.rs::invalid_input_cancel_and_admission_failures_keep_the_editor_truthful
#[test]
fn port_008_invalid_budget_input_is_never_written() {
    let mut host = client_host();
    let before = value_of(&mut host, KEY);
    host.send_action(
        tab("inference"),
        KEY,
        ActionKind::Changed,
        ActionValue::Text("12x".to_string()),
    );
    host.advance_clock(600);
    host.send_action(
        tab("inference"),
        KEY,
        ActionKind::Submitted,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(writes_of_budget(&mut host), Vec::<serde_json::Value>::new());
    assert_eq!(value_of(&mut host, KEY), before);
}

/// REQ: PORT:ilium-client/src/keys.rs::failed_write_lost_and_rejected_receipts_preserve_retry_and_latest_status
#[test]
fn port_009_a_rejected_write_keeps_the_draft_and_the_retry_stores_it() {
    let mut host = client_host();
    let before = value_of(&mut host, KEY);
    host.inject_failure(
        "settings.set",
        PluginError::new(ErrorCode::Busy, "write rejected"),
    );
    type_number(&mut host, "inference", KEY, 456789.0);
    assert_eq!(value_of(&mut host, KEY), before);
    let text = tab_text(&mut host, "inference", 100, 40);
    assert!(
        text.contains("456789") && text.contains("write rejected"),
        "{text}"
    );
    submit(&mut host, "inference", KEY);
    assert_eq!(value_of(&mut host, KEY), Some(json!(456789)));
}

/// REQ: PORT:ilium-client/src/keys.rs::inference_field_and_file_action_menu_restore_their_exact_parents
#[test]
fn port_010_closing_the_budget_dialog_restores_the_exact_tab_below_it() {
    let mut host = client_host();
    let before = tab_text(&mut host, "inference", 100, 40);
    host.send_action(
        tab("inference"),
        KEY,
        ActionKind::Activated,
        ActionValue::None,
    );
    host.settle();
    let opened = host.popups();
    assert_eq!(opened.len(), 1, "{opened:?}");
    host.send_action(
        ilium_plugin_sdk::types::ViewTarget::Popup(opened[0].clone()),
        &opened[0],
        ActionKind::Closed,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(host.popups(), Vec::<String>::new());
    assert_eq!(tab_text(&mut host, "inference", 100, 40), before);
    assert_eq!(writes_of_budget(&mut host), Vec::<serde_json::Value>::new());
}

/// REQ: PORT:ilium-client/src/keys.rs::own_durable_receipt_closes_exact_parent_and_repeated_enter_is_finite
#[test]
fn port_011_repeated_enter_without_an_edit_writes_nothing() {
    let mut host = client_host();
    for _ in 0..3 {
        host.send_action(
            tab("inference"),
            KEY,
            ActionKind::Submitted,
            ActionValue::None,
        );
    }
    host.settle();
    assert_eq!(writes_of_budget(&mut host), Vec::<serde_json::Value>::new());
}

/// REQ: PORT:ilium-client/src/keys.rs::invalid_input_cancel_and_admission_failures_keep_the_editor_truthful
#[test]
fn port_012_invalid_budget_text_stays_visible_in_the_editor() {
    let mut host = client_host();
    host.send_action(
        tab("inference"),
        KEY,
        ActionKind::Changed,
        ActionValue::Text("12x".to_string()),
    );
    host.settle();
    let text = tab_text(&mut host, "inference", 100, 40);
    assert!(text.contains("12x"), "{text}");
}

/// REQ: PORT:ilium-client/src/keys.rs::failed_write_lost_and_rejected_receipts_preserve_retry_and_latest_status
#[test]
fn port_013_a_successful_retry_clears_the_earlier_rejection_message() {
    let mut host = client_host();
    host.inject_failure(
        "settings.set",
        PluginError::new(ErrorCode::Busy, "write rejected"),
    );
    type_number(&mut host, "inference", KEY, 456789.0);
    submit(&mut host, "inference", KEY);
    let text = tab_text(&mut host, "inference", 100, 40);
    assert!(!text.contains("write rejected"), "{text}");
}

/// REQ: PORT:ilium-client/src/keys.rs::inference_field_and_file_action_menu_restore_their_exact_parents
#[test]
fn port_014_activating_the_budget_field_twice_opens_one_dialog() {
    let mut host = client_host();
    for _ in 0..2 {
        host.send_action(
            tab("inference"),
            KEY,
            ActionKind::Activated,
            ActionValue::None,
        );
    }
    host.settle();
    assert_eq!(host.popups().len(), 1, "{:?}", host.popups());
}
