//! Twins of ilium-client/src/filesystem/configuration_app.rs and filesystem/tests.rs: a typed
//! number saves 600 ms after the last edit or at once on Enter; a failed write keeps the draft
//! and is retried (src/docs/settings.md "Auto-save behaviour", F-090, F-091).

use ilium_plugin_sdk::types::{ErrorCode, PluginError};
use serde_json::json;

use crate::common::*;

const KEY: &str = "inference.restructure_prompt_token_limit";

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::typed_budget_debounces_draft_and_enter_upgrades_same_pending_write, F-090
#[test]
fn port_090_a_typed_budget_is_not_saved_one_millisecond_before_the_deadline() {
    let mut host = client_host();
    let before = value_of(&mut host, KEY);
    host.send_action(tab("inference"), KEY, ilium_plugin_sdk::types::ActionKind::Changed, ilium_plugin_sdk::types::ActionValue::Number(123456.0));
    host.advance_clock(599);
    host.settle();
    assert_eq!(value_of(&mut host, KEY), before);
}

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::typed_budget_debounces_draft_and_enter_upgrades_same_pending_write, F-090
#[test]
fn port_090_a_typed_budget_is_saved_at_the_deadline() {
    let mut host = client_host();
    type_number(&mut host, "inference", KEY, 123456.0);
    assert_eq!(value_of(&mut host, KEY), Some(json!(123456)));
}

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::typed_budget_debounces_draft_and_enter_upgrades_same_pending_write, F-090
#[test]
fn port_090_enter_saves_a_typed_budget_without_waiting_and_without_a_second_write() {
    let mut host = client_host();
    host.send_action(tab("inference"), KEY, ilium_plugin_sdk::types::ActionKind::Changed, ilium_plugin_sdk::types::ActionValue::Number(123456.0));
    submit(&mut host, "inference", KEY);
    assert_eq!(value_of(&mut host, KEY), Some(json!(123456)));
    host.advance_clock(600);
    host.settle();
    assert_eq!(value_of(&mut host, KEY), Some(json!(123456)));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::typed_budget_waits_for_one_own_write_retains_failure_and_retries, F-090
#[test]
fn port_090_a_failed_budget_write_keeps_the_old_value_and_shows_an_error() {
    let mut host = client_host();
    let before = value_of(&mut host, KEY);
    host.inject_failure("settings.set", PluginError::new(ErrorCode::Io, "disk full"));
    type_number(&mut host, "inference", KEY, 123456.0);
    assert_eq!(value_of(&mut host, KEY), before);
    let text = tab_text(&mut host, "inference", 100, 40);
    assert!(text.contains("123456"), "the exact draft stays visible: {text}");
}

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::typed_budget_waits_for_one_own_write_retains_failure_and_retries, F-090
#[test]
fn port_090_the_budget_is_saved_when_the_user_retries_after_the_failure() {
    let mut host = client_host();
    host.inject_failure("settings.set", PluginError::new(ErrorCode::Io, "disk full"));
    type_number(&mut host, "inference", KEY, 123456.0);
    submit(&mut host, "inference", KEY);
    assert_eq!(value_of(&mut host, KEY), Some(json!(123456)));
}

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::latest_typed_budget_lost_receipt_keeps_exact_draft_and_retry_notice, F-090
#[test]
fn port_090_a_lost_write_receipt_keeps_the_exact_draft_visible() {
    let mut host = client_host();
    host.inject_failure("settings.set", PluginError::new(ErrorCode::TimedOut, "receipt lost"));
    type_number(&mut host, "inference", KEY, 123456.0);
    let text = tab_text(&mut host, "inference", 100, 40);
    assert!(text.contains("123456"), "{text}");
}

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::latest_typed_budget_lost_receipt_keeps_exact_draft_and_retry_notice, F-090
#[test]
fn port_090_a_lost_write_receipt_names_the_failure_in_the_page() {
    let mut host = client_host();
    host.inject_failure("settings.set", PluginError::new(ErrorCode::TimedOut, "receipt lost"));
    type_number(&mut host, "inference", KEY, 123456.0);
    let text = tab_text(&mut host, "inference", 100, 40);
    assert!(text.contains("receipt lost"), "{text}");
}

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::superseded_budget_operation_cannot_settle_its_still_visible_save_token, F-090
#[test]
fn port_090_a_newer_edit_wins_over_an_older_pending_save() {
    let mut host = client_host();
    host.send_action(tab("inference"), KEY, ilium_plugin_sdk::types::ActionKind::Changed, ilium_plugin_sdk::types::ActionValue::Number(1000.0));
    host.advance_clock(300);
    type_number(&mut host, "inference", KEY, 2000.0);
    host.advance_clock(1000);
    host.settle();
    assert_eq!(value_of(&mut host, KEY), Some(json!(2000)));
}

/// REQ: PORT:ilium-client/src/filesystem/configuration_app.rs::superseded_budget_operation_cannot_settle_its_still_visible_save_token, F-090
#[test]
fn port_090_a_superseded_failure_does_not_replace_the_newer_status() {
    let mut host = client_host();
    host.send_action(tab("inference"), KEY, ilium_plugin_sdk::types::ActionKind::Changed, ilium_plugin_sdk::types::ActionValue::Number(1000.0));
    type_number(&mut host, "inference", KEY, 2000.0);
    host.settle();
    assert_eq!(value_of(&mut host, KEY), Some(json!(2000)));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::configuration_command_normalizes_spare_capacity_before_retention, F-090, F-091
#[test]
fn port_090_a_git_setup_command_is_stored_as_the_exact_text() {
    let mut host = client_host();
    host.settings().set("git.setup_command", json!("true")).expect("accepted");
    assert_eq!(value_of(&mut host, "git.setup_command"), Some(json!("true")));
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::configuration_command_normalizes_spare_capacity_before_retention, F-090, F-091
#[test]
fn port_090_a_git_setup_command_with_spare_capacity_keeps_only_its_text() {
    let mut host = client_host();
    let mut command = String::with_capacity(2 * 1024 * 1024);
    command.push_str("true");
    host.settings().set("git.setup_command", json!(command)).expect("accepted");
    let stored = value_of(&mut host, "git.setup_command").expect("stored");
    assert_eq!(stored, json!("true"));
}

/// REQ: PORT:ilium-client/src/filesystem/terminal_pool_number_tests.rs::terminal_pool_number_dialog_queue_refusal_is_unsaved_and_does_not_apply_locally
#[test]
fn port_090_a_refused_engine_budget_write_is_not_applied_and_the_draft_stays_visible() {
    const BUDGET: &str = "terminal.engine_memory_budget_mib";
    let mut host = client_host();
    let before = value_of(&mut host, BUDGET);
    host.inject_failure("settings.set", PluginError::new(ErrorCode::Busy, "write queue closed"));
    type_number(&mut host, "terminal", BUDGET, 257.0);
    assert_eq!(value_of(&mut host, BUDGET), before, "the refused value is not applied locally");
    let text = tab_text(&mut host, "terminal", 100, 40);
    assert!(text.contains("257"), "the dialog keeps the draft: {text}");
}
