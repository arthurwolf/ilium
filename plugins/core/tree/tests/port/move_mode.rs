//! Twins of the Move mode tests of `ilium-client/src/keys.rs` (indent and outdent). The plugin expresses a
//! move as one `move-node` intent (page core-plugin-tree.md, "Moving, reordering and renaming entries").
//! Assumed command ids `tree.indent` and `tree.outdent` carry the focused node in the command context.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent, MoveNodeSpec, NodeClass,
    NodeId, Placement,
};

use crate::common::{add_node, server_host};

fn invoke(host: &mut FakeHost, command: &str, node: u64) -> Vec<MoveNodeSpec> {
    let submissions = host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Palette,
        context: CommandContext {
            pane: Some(NodeId(node)),
            ..CommandContext::default()
        },
    }));
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::MoveNode(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// REQ: PORT:ilium-client/src/keys.rs::indent_moves_into_the_nearest_preceding_sibling_group
#[test]
fn port_001_indent_moves_into_the_nearest_preceding_sibling_group() {
    let mut host = server_host();
    let top = add_node(&mut host, None, "top", NodeClass::Container);
    let sibling = add_node(&mut host, Some(top), "sibling", NodeClass::Container);
    let pane = add_node(&mut host, Some(top), "pane", NodeClass::Pane);
    let moves = invoke(&mut host, "tree.indent", pane);
    assert_eq!(
        moves,
        vec![MoveNodeSpec {
            node: NodeId(pane),
            target: NodeId(sibling),
            placement: Placement::InsideLast
        }]
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::outdent_moves_out_into_the_parent_group_right_after_the_current_group
#[test]
fn port_002_outdent_moves_out_into_the_parent_group_right_after_the_current_group() {
    let mut host = server_host();
    let top = add_node(&mut host, None, "top", NodeClass::Container);
    let inner = add_node(&mut host, Some(top), "inner", NodeClass::Container);
    let pane = add_node(&mut host, Some(inner), "pane", NodeClass::Pane);
    add_node(&mut host, Some(top), "other", NodeClass::Pane);
    let moves = invoke(&mut host, "tree.outdent", pane);
    assert_eq!(
        moves,
        vec![MoveNodeSpec {
            node: NodeId(pane),
            target: NodeId(inner),
            placement: Placement::After
        }]
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::outdent_at_the_top_level_is_a_no_op
#[test]
fn port_003_outdent_at_the_top_level_is_a_no_op() {
    let mut host = server_host();
    let top = add_node(&mut host, None, "top", NodeClass::Container);
    assert_eq!(
        invoke(&mut host, "tree.outdent", top),
        Vec::<MoveNodeSpec>::new()
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::outdent_rejects_leaving_a_pane_parentless_at_the_top_level
#[test]
fn port_004_outdent_rejects_leaving_a_pane_parentless_at_the_top_level() {
    let mut host = server_host();
    let top = add_node(&mut host, None, "top", NodeClass::Container);
    let pane = add_node(&mut host, Some(top), "pane", NodeClass::Pane);
    assert_eq!(
        invoke(&mut host, "tree.outdent", pane),
        Vec::<MoveNodeSpec>::new()
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::indent_moves_into_the_nearest_preceding_sibling_group
#[test]
fn indent_skips_a_preceding_pane_to_reach_the_nearest_group() {
    let mut host = server_host();
    let top = add_node(&mut host, None, "top", NodeClass::Container);
    let group = add_node(&mut host, Some(top), "group", NodeClass::Container);
    add_node(&mut host, Some(top), "pane before", NodeClass::Pane);
    let pane = add_node(&mut host, Some(top), "pane", NodeClass::Pane);
    let moves = invoke(&mut host, "tree.indent", pane);
    assert_eq!(
        moves,
        vec![MoveNodeSpec {
            node: NodeId(pane),
            target: NodeId(group),
            placement: Placement::InsideLast
        }]
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::outdent_at_the_top_level_is_a_no_op
#[test]
fn outdent_of_a_second_top_level_group_is_a_no_op() {
    let mut host = server_host();
    add_node(&mut host, None, "first", NodeClass::Container);
    let second = add_node(&mut host, None, "second", NodeClass::Container);
    assert_eq!(
        invoke(&mut host, "tree.outdent", second),
        Vec::<MoveNodeSpec>::new()
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::outdent_moves_out_into_the_parent_group_right_after_the_current_group
#[test]
fn outdent_moves_only_the_focused_node_and_leaves_its_sibling_in_place() {
    let mut host = server_host();
    let top = add_node(&mut host, None, "top", NodeClass::Container);
    let inner = add_node(&mut host, Some(top), "inner", NodeClass::Container);
    let pane = add_node(&mut host, Some(inner), "pane", NodeClass::Pane);
    add_node(&mut host, Some(inner), "sibling", NodeClass::Pane);
    let moves = invoke(&mut host, "tree.outdent", pane);
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].node, NodeId(pane));
    assert_eq!(moves[0].target, NodeId(inner));
    assert_eq!(moves[0].placement, Placement::After);
}

/// REQ: PORT:ilium-client/src/keys.rs::outdent_rejects_leaving_a_pane_parentless_at_the_top_level
#[test]
fn rejected_outdent_submits_no_move_intent_at_all() {
    let mut host = server_host();
    let top = add_node(&mut host, None, "top", NodeClass::Container);
    let pane = add_node(&mut host, Some(top), "pane", NodeClass::Pane);
    let moves = invoke(&mut host, "tree.outdent", pane);
    assert!(moves.is_empty());
    assert!(host
        .submitted()
        .iter()
        .all(|submission| !matches!(submission.intent, Intent::MoveNode(_))));
}
