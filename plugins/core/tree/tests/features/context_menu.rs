//! Page sample "Right-click menu for a node": the client side handles `context-requested` and menu choices.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, Event, Intent, NodeId, SetNodeLockedSpec, Submission, UiAction, ViewTarget};

use crate::common::client_host;

fn action(key: &str, kind: ActionKind, value: Option<ActionValue>) -> Event {
    Event::UiAction(UiAction {
        view: ViewTarget::Panel("tree".to_string()),
        key: key.to_string(),
        action_id: String::new(),
        kind,
        value,
    })
}

fn choose(host: &mut FakeHost, node: u64, item: &str) -> Vec<Submission> {
    let key = format!("row-{node}");
    host.deliver_one(action(&key, ActionKind::ContextRequested, None));
    host.deliver_one(action(&key, ActionKind::Activated, Some(ActionValue::Text(item.to_string()))))
}

/// REQ: F-006
#[test]
fn feature_006_choosing_lock_submits_set_node_locked_true() {
    let mut host = client_host();
    let submissions = choose(&mut host, 14, "lock");
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].intent,
        Intent::SetNodeLocked(SetNodeLockedSpec { node: NodeId(14), locked: true })
    );
}

/// REQ: F-006
#[test]
fn feature_006_choosing_unlock_submits_set_node_locked_false() {
    let mut host = client_host();
    let submissions = choose(&mut host, 14, "unlock");
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].intent,
        Intent::SetNodeLocked(SetNodeLockedSpec { node: NodeId(14), locked: false })
    );
}

/// REQ: F-006
#[test]
fn feature_006_choosing_close_submits_close_node() {
    let mut host = client_host();
    let submissions = choose(&mut host, 21, "close");
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].intent, Intent::CloseNode(NodeId(21)));
}

/// REQ: F-006
#[test]
fn feature_006_the_context_request_itself_submits_nothing() {
    let mut host = client_host();
    let submissions = host.deliver_one(action("row-14", ActionKind::ContextRequested, None));
    assert!(submissions.is_empty());
}

/// REQ: F-006
#[test]
fn feature_006_an_unknown_menu_item_submits_nothing() {
    let mut host = client_host();
    assert!(choose(&mut host, 14, "frobnicate").is_empty());
    // The unknown item left no stale state: a following valid choice still submits exactly one lock.
    let submissions = choose(&mut host, 14, "lock");
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].intent,
        Intent::SetNodeLocked(SetNodeLockedSpec { node: NodeId(14), locked: true })
    );
}

/// REQ: F-006
#[test]
fn feature_006_actions_on_keys_without_the_row_prefix_are_ignored() {
    let mut host = client_host();
    let submissions =
        host.deliver_one(action("menu-14", ActionKind::Activated, Some(ActionValue::Text("lock".to_string()))));
    assert!(submissions.is_empty());
}

/// REQ: F-006
#[test]
fn feature_006_activation_without_a_text_value_is_ignored() {
    let mut host = client_host();
    let submissions = host.deliver_one(action("row-14", ActionKind::Activated, Some(ActionValue::Index(1))));
    assert!(submissions.is_empty());
}

/// REQ: F-006
#[test]
fn feature_006_other_action_kinds_on_a_row_are_ignored() {
    let mut host = client_host();
    for kind in [ActionKind::Selected, ActionKind::Toggled, ActionKind::HoverEnter, ActionKind::FocusGained] {
        let submissions = host.deliver_one(action("row-14", kind, Some(ActionValue::Text("lock".to_string()))));
        assert!(submissions.is_empty());
    }
}

/// REQ: F-006
#[test]
fn feature_006_each_node_gets_its_own_intent() {
    let mut host = client_host();
    let first = choose(&mut host, 3, "lock");
    let second = choose(&mut host, 4, "lock");
    assert_eq!(first[0].intent, Intent::SetNodeLocked(SetNodeLockedSpec { node: NodeId(3), locked: true }));
    assert_eq!(second[0].intent, Intent::SetNodeLocked(SetNodeLockedSpec { node: NodeId(4), locked: true }));
}

/// REQ: F-006
#[test]
fn feature_006_with_lock_closed_off_the_lock_item_submits_nothing() {
    let mut host = client_host();
    host.settings().set("ui.lock_closed_enabled", serde_json::json!(false)).expect("setting accepted");
    assert!(choose(&mut host, 14, "lock").is_empty());
}

/// REQ: F-006
#[test]
fn feature_006_with_lock_closed_off_unlock_stays_available() {
    let mut host = client_host();
    host.settings().set("ui.lock_closed_enabled", serde_json::json!(false)).expect("setting accepted");
    let submissions = choose(&mut host, 14, "unlock");
    assert_eq!(
        submissions.iter().map(|s| s.intent.clone()).collect::<Vec<_>>(),
        vec![Intent::SetNodeLocked(SetNodeLockedSpec { node: NodeId(14), locked: false })]
    );
}

/// REQ: F-006
#[test]
fn feature_006_the_lock_closed_setting_defaults_to_on() {
    let mut host = client_host();
    assert_eq!(host.settings().get("ui.lock_closed_enabled"), Some(serde_json::json!(true)));
}

/// REQ: F-006
#[test]
fn feature_006_lock_closed_rejects_a_non_boolean_with_invalid_argument() {
    use ilium_plugin_sdk::types::ErrorCode;
    let mut host = client_host();
    let error = host.settings().set("ui.lock_closed_enabled", serde_json::json!(3)).expect_err("not a boolean");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}
