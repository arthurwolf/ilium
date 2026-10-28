//! Twins of the tree key tests of ilium-client/src/keys.rs (indent, outdent and the Order By context menu).
//! Assumed commands `ilium.core.tree/indent` and `ilium.core.tree/outdent` (context pane = the node) that submit
//! one `move-node`, and the context menu items `order` (opens the submenu) and `order-type`; see API_GAPS.md.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent,
    MoveNodeSpec, NodeClass, NodeId, Placement, UiAction, ViewTarget,
};

use crate::common::*;

fn command(name: &str, node: u64) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: name.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Keybinding,
        context: CommandContext {
            pane: Some(NodeId(node)),
            ..CommandContext::default()
        },
    })
}

fn moves(submissions: &[ilium_plugin_sdk::types::Submission]) -> Vec<MoveNodeSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::MoveNode(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// REQ: F-004, PORT:ilium-client/src/keys.rs::indent_with_no_preceding_sibling_group_is_a_no_op
#[test]
fn port_020_indent_with_no_preceding_sibling_group_is_a_no_op() {
    let mut host = server_host();
    let top = add_node(&mut host, None, "top", NodeClass::Container);
    let sibling_group = add_node(&mut host, Some(top), "sibling group", NodeClass::Container);
    add_node(&mut host, Some(top), "pane", NodeClass::Pane);
    add_node(&mut host, None, "other", NodeClass::Container);
    host.settle();
    let submissions = host.deliver_one(command("ilium.core.tree/indent", sibling_group));
    host.settle();
    assert_eq!(moves(&submissions), Vec::<MoveNodeSpec>::new());
    assert_eq!(moves(&host.submitted()), Vec::<MoveNodeSpec>::new());
}

/// REQ: F-004, PORT:ilium-client/src/keys.rs::outdent_allows_a_split_view_out_of_a_top_level_group_to_the_root
#[test]
fn port_021_outdent_moves_a_split_view_out_of_a_top_level_group_to_the_root() {
    let mut host = server_host();
    let top = add_node(&mut host, None, "top", NodeClass::Container);
    let split_view = add_node(&mut host, Some(top), "split view", NodeClass::Container);
    host.settle();
    let submissions = host.deliver_one(command("ilium.core.tree/outdent", split_view));
    host.settle();
    assert_eq!(
        moves(&submissions),
        vec![MoveNodeSpec {
            node: NodeId(split_view),
            target: NodeId(top),
            placement: Placement::After
        }]
    );
}

/// REQ: F-006, PORT:ilium-client/src/keys.rs::context_menu_keyboard_opens_order_submenu_and_applies_selection
#[test]
fn port_022_context_menu_keyboard_opens_the_order_submenu_and_applies_the_selection() {
    let mut host = client_host();
    let node = add_node(&mut host, None, "work", NodeClass::Container);
    let key = format!("row-{node}");
    let send = |host: &mut ilium_plugin_sdk::testing::FakeHost,
                kind: ActionKind,
                value: Option<ActionValue>| {
        host.deliver_one(Event::UiAction(UiAction {
            view: ViewTarget::Panel("tree".to_string()),
            key: key.clone(),
            action_id: String::new(),
            kind,
            value,
        }))
    };
    send(&mut host, ActionKind::ContextRequested, None);
    let opened = send(
        &mut host,
        ActionKind::Activated,
        Some(ActionValue::Text("order".to_string())),
    );
    assert_eq!(opened, Vec::new(), "opening the submenu submits nothing");
    send(
        &mut host,
        ActionKind::Activated,
        Some(ActionValue::Text("order-type".to_string())),
    );
    host.settle();
    assert_eq!(
        host.settings().get("tree_order"),
        Some(serde_json::json!("type"))
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::indent_with_no_preceding_sibling_group_is_a_no_op
#[test]
fn indent_of_a_pane_preceded_only_by_panes_is_a_no_op() {
    let mut host = server_host();
    let top = add_node(&mut host, None, "top", NodeClass::Container);
    add_node(&mut host, Some(top), "first pane", NodeClass::Pane);
    let second = add_node(&mut host, Some(top), "second pane", NodeClass::Pane);
    host.settle();
    let submissions = host.deliver_one(command("ilium.core.tree/indent", second));
    host.settle();
    assert_eq!(moves(&submissions), Vec::<MoveNodeSpec>::new());
}

/// REQ: F-006, PORT:ilium-client/src/keys.rs::context_menu_keyboard_opens_order_submenu_and_applies_selection
#[test]
fn opening_the_context_menu_submits_nothing_and_does_not_change_the_order() {
    let mut host = client_host();
    let node = add_node(&mut host, None, "work", NodeClass::Container);
    let opened = host.deliver_one(Event::UiAction(UiAction {
        view: ViewTarget::Panel("tree".to_string()),
        key: format!("row-{node}"),
        action_id: String::new(),
        kind: ActionKind::ContextRequested,
        value: None,
    }));
    host.settle();
    assert_eq!(opened, Vec::new(), "opening the menu submits nothing");
    assert_ne!(
        host.settings().get("tree_order"),
        Some(serde_json::json!("type"))
    );
}
