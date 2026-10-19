//! Twins of the modal and paste tests of `ilium-client/src/keys.rs` that concern settings dialogs: the
//! stack of dialogs over a settings tab, atomic paste into a text row, and late completions of dismissed
//! dialogs. A paste reaches a row as one `changed` text action; a dialog is a `ViewTarget::Popup`.

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, PluginError, ViewTarget};
use serde_json::json;

use crate::common::*;

const BUDGET_KEY: &str = "inference.restructure_prompt_token_limit";
const TEXT_KEY: &str = "git.setup_command";

fn open_row(host: &mut ilium_plugin_sdk::testing::FakeHost, tab_id: &str, key: &str) {
    host.send_action(tab(tab_id), key, ActionKind::Activated, ActionValue::None);
    host.settle();
}

/// REQ: PORT:ilium-client/src/keys.rs::atomic_paste_reuses_existing_character_input_for_text_prompt_modes
#[test]
fn port_001_atomic_paste_reuses_existing_character_input_for_text_prompt_modes() {
    let mut host = client_host();
    host.send_action(
        tab("git"),
        TEXT_KEY,
        ActionKind::Changed,
        ActionValue::Text("before-".to_string()),
    );
    host.send_action(
        tab("git"),
        TEXT_KEY,
        ActionKind::Changed,
        ActionValue::Text("before-caf\u{e9} \u{4e16}\u{754c}".to_string()),
    );
    press(&mut host, "git", &["Enter"]);
    assert_eq!(
        value_of(&mut host, TEXT_KEY),
        Some(json!("before-caf\u{e9} \u{4e16}\u{754c}"))
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::modal_stack_restores_arbitrary_depth_in_last_opened_first_closed_order
#[test]
fn port_002_modal_stack_restores_arbitrary_depth_in_last_opened_first_closed_order() {
    let mut host = client_host();
    open_row(&mut host, "inference", BUDGET_KEY);
    let first = host.popups();
    open_row(&mut host, "git", TEXT_KEY);
    let second = host.popups();
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(second.len(), 2, "{second:?}");
    assert_eq!(
        second[0], first[0],
        "the older dialog stays below the newer one"
    );
    // Closing the top dialog restores the one below it, then the tab.
    host.send_action(
        ViewTarget::Popup(second[1].clone()),
        &second[1],
        ActionKind::Closed,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(host.popups(), first);
    host.send_action(
        ViewTarget::Popup(first[0].clone()),
        &first[0],
        ActionKind::Closed,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(host.popups().len(), 0);
}

/// REQ: PORT:ilium-client/src/keys.rs::escaped_reopened_and_covered_dialogs_reject_old_dismissal
#[test]
fn port_003_escaped_reopened_and_covered_dialogs_reject_old_dismissal() {
    let mut host = client_host();
    open_row(&mut host, "inference", BUDGET_KEY);
    let old = host.popups();
    assert_eq!(old.len(), 1);
    host.send_action(
        ViewTarget::Popup(old[0].clone()),
        &old[0],
        ActionKind::Closed,
        ActionValue::None,
    );
    host.settle();
    open_row(&mut host, "inference", BUDGET_KEY);
    let reopened = host.popups();
    assert_eq!(reopened.len(), 1);
    // A late dismissal of the escaped dialog does not close the reopened one.
    host.send_action(
        ViewTarget::Popup(old[0].clone()),
        &old[0],
        ActionKind::Closed,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(host.popups(), reopened);
    // A covered dialog stays on the stack when a dialog above it is dismissed with a stale target.
    open_row(&mut host, "git", TEXT_KEY);
    assert_eq!(host.popups().len(), 2);
    host.send_action(
        ViewTarget::Popup(old[0].clone()),
        &old[0],
        ActionKind::Closed,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(host.popups().len(), 2);
}

/// REQ: PORT:ilium-client/src/keys.rs::stale_success_preserves_newer_error_after_edit_away_and_back
#[test]
fn port_004_stale_success_preserves_newer_error_after_edit_away_and_back() {
    let mut host = client_host();
    host.inject_failure("settings.set", PluginError::new(ErrorCode::Io, "disk full"));
    type_number(&mut host, "inference", BUDGET_KEY, 111111.0);
    let failed = tab_text(&mut host, "inference", 100, 40);
    assert!(failed.contains("disk full"), "{failed}");
    // Edit away and back to the failed value: the older success of the other edit must not clear the error.
    type_number(&mut host, "inference", BUDGET_KEY, 222222.0);
    host.inject_failure(
        "settings.set",
        PluginError::new(ErrorCode::Io, "disk full again"),
    );
    type_number(&mut host, "inference", BUDGET_KEY, 111111.0);
    let text = tab_text(&mut host, "inference", 100, 40);
    assert!(
        text.contains("disk full again"),
        "the newer error stays visible: {text}"
    );
    // The retry saves once the store works.
    submit(&mut host, "inference", BUDGET_KEY);
    assert_eq!(value_of(&mut host, BUDGET_KEY), Some(json!(111111)));
}
