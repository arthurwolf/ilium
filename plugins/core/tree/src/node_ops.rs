//! The four context-menu node operations of the tree (move up, move down, bookmark, lock closed),
//! the indent and outdent keys, and the tree snapshot the move rules read.
//!
//! The kernel menu used to call `Tree::move_node_one_step`, `set_bookmarked` and
//! `set_locked_closed` directly. The menu now invokes the plugin commands
//! `ilium.core.tree/{move-up,move-down,set-bookmark,set-locked-closed}` with `{"node", "on"}`
//! arguments; this module parses those arguments and turns each operation into one intent.

use std::collections::HashMap;

use ilium_plugin_sdk::types::{
    ContainerKind, Intent, MoveNodeSpec, NodeClass, NodeId, NodeView, Placement, SetBookmarkedSpec,
    SetNodeLockedSpec,
};
use serde_json::{json, Value};

use crate::move_mode::{self, Direction, TreeFacts};

/// The plugin id, the prefix of every command id.
pub const PLUGIN_ID: &str = "ilium.core.tree";

/// One operation of the tree context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOp {
    /// One visible step towards the top.
    MoveUp,
    /// One visible step towards the bottom.
    MoveDown,
    /// Bookmark (`true`) or un-bookmark (`false`) the node.
    Bookmark(bool),
    /// Lock the node closed (`true`) or unlock it (`false`).
    LockClosed(bool),
}

impl NodeOp {
    /// The bare command ids of the four operations.
    pub const COMMANDS: [&'static str; 4] =
        ["move-up", "move-down", "set-bookmark", "set-locked-closed"];

    /// The bare command id of the operation.
    pub const fn command(self) -> &'static str {
        match self {
            Self::MoveUp => "move-up",
            Self::MoveDown => "move-down",
            Self::Bookmark(_) => "set-bookmark",
            Self::LockClosed(_) => "set-locked-closed",
        }
    }

    /// The command arguments the kernel menu sends: `{"node": id}` plus `"on"` for the two switches.
    pub fn to_arguments(self, node: u64) -> String {
        match self {
            Self::MoveUp | Self::MoveDown => json!({ "node": node }),
            Self::Bookmark(on) | Self::LockClosed(on) => json!({ "node": node, "on": on }),
        }
        .to_string()
    }

    /// Parses a command id (bare or `ilium.core.tree/` prefixed) with its JSON arguments into the
    /// operation and the node it applies to. A switch without `on` is an error, not a guess.
    pub fn from_command(command: &str, arguments_json: &str) -> Result<(Self, u64), String> {
        let bare = command
            .strip_prefix(PLUGIN_ID)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(command);
        let arguments: Value =
            serde_json::from_str(arguments_json).map_err(|error| format!("arguments: {error}"))?;
        let node = arguments
            .get("node")
            .and_then(Value::as_u64)
            .ok_or_else(|| "arguments need an integer `node`".to_string())?;
        let on = || {
            arguments
                .get("on")
                .and_then(Value::as_bool)
                .ok_or_else(|| "arguments need a boolean `on`".to_string())
        };
        let operation = match bare {
            "move-up" => Self::MoveUp,
            "move-down" => Self::MoveDown,
            "set-bookmark" => Self::Bookmark(on()?),
            "set-locked-closed" => Self::LockClosed(on()?),
            other => return Err(format!("unknown node command `{other}`")),
        };
        Ok((operation, node))
    }
}

/// The intent an operation submits for a node, or `None` when the node cannot move that way
/// (an outer boundary, an unknown node).
pub fn intent_for(operation: NodeOp, node: u64, facts: &impl TreeFacts) -> Option<Intent> {
    match operation {
        NodeOp::MoveUp => move_intent(node, move_mode::one_step_target(facts, node, Direction::Up)),
        NodeOp::MoveDown => move_intent(
            node,
            move_mode::one_step_target(facts, node, Direction::Down),
        ),
        NodeOp::Bookmark(bookmarked) => Some(Intent::SetBookmarked(SetBookmarkedSpec {
            node: NodeId(node),
            bookmarked,
        })),
        NodeOp::LockClosed(locked) => Some(Intent::SetNodeLocked(SetNodeLockedSpec {
            node: NodeId(node),
            locked,
        })),
    }
}

/// The `move-node` intent of an indent (`outdent == false`) or outdent key.
pub fn indent_intent(node: u64, outdent: bool, facts: &impl TreeFacts) -> Option<Intent> {
    let target = if outdent {
        move_mode::outdent_target(facts, node)
    } else {
        move_mode::indent_target(facts, node)
    };
    move_intent(node, target)
}

fn move_intent(node: u64, target: Option<(u64, Placement)>) -> Option<Intent> {
    let (target, placement) = target?;
    Some(Intent::MoveNode(MoveNodeSpec {
        node: NodeId(node),
        target: NodeId(target),
        placement,
    }))
}

/// One node of a [`Snapshot`].
#[derive(Debug, Clone)]
struct Fact {
    parent: Option<u64>,
    class: NodeClass,
    container: Option<ContainerKind>,
}

/// The tree as the move rules need it: parents, ordered children and node kinds.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    facts: HashMap<u64, Fact>,
    children: HashMap<Option<u64>, Vec<u64>>,
}

impl Snapshot {
    /// Builds a snapshot from node views listed parent-before-child, siblings in stored order.
    pub fn from_views(views: &[NodeView]) -> Self {
        let mut snapshot = Self::default();
        for view in views {
            let parent = view.parent.map(|parent| parent.0);
            snapshot.facts.insert(
                view.id.0,
                Fact {
                    parent,
                    class: view.class,
                    container: view.container,
                },
            );
            snapshot.children.entry(parent).or_default().push(view.id.0);
        }
        snapshot
    }

    fn container(&self, node: u64) -> Option<ContainerKind> {
        self.facts.get(&node).and_then(|fact| fact.container)
    }
}

impl TreeFacts for Snapshot {
    fn parent(&self, node: u64) -> Option<u64> {
        self.facts.get(&node).and_then(|fact| fact.parent)
    }
    fn children(&self, parent: Option<u64>) -> Vec<u64> {
        self.children.get(&parent).cloned().unwrap_or_default()
    }
    fn is_pane(&self, node: u64) -> bool {
        self.facts
            .get(&node)
            .is_some_and(|fact| fact.class == NodeClass::Pane)
    }
    fn accepts_normal_children(&self, node: u64) -> bool {
        // A container without a stated kind is a plain group.
        self.facts.get(&node).is_some_and(|fact| {
            fact.class == NodeClass::Container
                && crate::ordering::accepts_normal_children(
                    fact.container.or(Some(ContainerKind::Group)),
                )
        })
    }
    fn is_group(&self, node: u64) -> bool {
        self.facts.get(&node).is_some_and(|fact| {
            fact.class == NodeClass::Container
                && matches!(fact.container, Some(ContainerKind::Group) | None)
        })
    }
    fn is_split_view(&self, node: u64) -> bool {
        matches!(
            self.container(node),
            Some(ContainerKind::SplitHorizontal | ContainerKind::SplitVertical)
        )
    }
    fn is_project(&self, node: u64) -> bool {
        self.container(node) == Some(ContainerKind::Project)
    }
    fn in_worktree(&self, node: u64) -> bool {
        let mut current = Some(node);
        for _ in 0..4096 {
            let Some(id) = current else { return false };
            if self.container(id) == Some(ContainerKind::Worktree) {
                return true;
            }
            current = self.parent(id);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn view(
        id: u64,
        parent: Option<u64>,
        class: NodeClass,
        container: Option<ContainerKind>,
    ) -> NodeView {
        NodeView {
            id: NodeId(id),
            parent: parent.map(NodeId),
            class,
            content_kind: None,
            title: format!("n{id}"),
            cwd: None,
            agent: None,
            locked: false,
            revision: 0,
            container,
        }
    }

    fn sample() -> Snapshot {
        Snapshot::from_views(&[
            view(1, None, NodeClass::Container, Some(ContainerKind::Group)),
            view(2, Some(1), NodeClass::Pane, None),
            view(3, Some(1), NodeClass::Pane, None),
        ])
    }

    proptest! {
        #[test]
        fn arguments_round_trip_through_the_command_parser(node in any::<u64>(), on in any::<bool>()) {
            for operation in [
                NodeOp::MoveUp,
                NodeOp::MoveDown,
                NodeOp::Bookmark(on),
                NodeOp::LockClosed(on),
            ] {
                let arguments = operation.to_arguments(node);
                let parsed = NodeOp::from_command(operation.command(), &arguments);
                prop_assert_eq!(parsed, Ok((operation, node)));
                let prefixed = format!("{PLUGIN_ID}/{}", operation.command());
                prop_assert_eq!(NodeOp::from_command(&prefixed, &arguments), Ok((operation, node)));
            }
        }
    }

    #[test]
    fn a_switch_without_on_and_an_unknown_command_are_refused() {
        assert!(NodeOp::from_command("set-bookmark", r#"{"node": 3}"#).is_err());
        assert!(NodeOp::from_command("move-up", r#"{"on": true}"#).is_err());
        assert!(NodeOp::from_command("frobnicate", r#"{"node": 3}"#).is_err());
        assert!(NodeOp::from_command("move-up", "not json").is_err());
    }

    #[test]
    fn move_down_swaps_with_the_next_sibling() {
        let intent = intent_for(NodeOp::MoveDown, 2, &sample());
        assert_eq!(
            intent,
            Some(Intent::MoveNode(MoveNodeSpec {
                node: NodeId(2),
                target: NodeId(3),
                placement: Placement::After
            }))
        );
    }

    #[test]
    fn move_up_at_the_outer_boundary_submits_nothing() {
        assert_eq!(intent_for(NodeOp::MoveUp, 2, &sample()), None);
    }

    #[test]
    fn the_switches_map_to_their_intents() {
        assert_eq!(
            intent_for(NodeOp::Bookmark(true), 2, &sample()),
            Some(Intent::SetBookmarked(SetBookmarkedSpec {
                node: NodeId(2),
                bookmarked: true
            }))
        );
        assert_eq!(
            intent_for(NodeOp::LockClosed(false), 2, &sample()),
            Some(Intent::SetNodeLocked(SetNodeLockedSpec {
                node: NodeId(2),
                locked: false
            }))
        );
    }

    #[test]
    fn a_worktree_blocks_indent_and_outdent() {
        let snapshot = Snapshot::from_views(&[
            view(1, None, NodeClass::Container, Some(ContainerKind::Worktree)),
            view(2, Some(1), NodeClass::Pane, None),
        ]);
        assert!(snapshot.in_worktree(2));
        assert_eq!(indent_intent(2, false, &snapshot), None);
        assert_eq!(indent_intent(2, true, &snapshot), None);
    }
}
