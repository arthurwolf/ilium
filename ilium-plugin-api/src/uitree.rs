//! Size checks of a `ui-tree` returned by `views.render` (reference-errors-and-limits.md
//! "Requestable limits" `ui_tree_nodes`, `ui_tree_kib`; reference-wit.md section 4.10).

use crate::error::Code;
use crate::generated::types::UiTree;

/// The caps a rendered tree must fit (reference-errors-and-limits.md "Requestable limits").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiCaps {
    /// Most nodes, the `ui_tree_nodes` limit.
    pub max_nodes: u32,
    /// Most encoded KiB, the `ui_tree_kib` limit.
    pub max_kib: u32,
    /// Deepest nesting. Plan 1.3 field; the manual sets no ui-tree depth cap yet
    /// (reference-errors-and-limits.md lists none).
    pub max_depth: u32,
}

/// Checks a tree against the caps and the tree rules; a breach is `trap.invalid-ui-tree`
/// (reference-errors-and-limits.md "Trap codes").
pub fn check_ui_tree(_tree: &UiTree, _caps: UiCaps) -> Result<(), Code> {
    unimplemented!("api::uitree::check_ui_tree")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::types::{
        Constraint, Direction, Flex, LayoutSpec, Margin, NodeFlags, Spacing, UiNode, Widget,
    };

    /// The caps the manual states as defaults (reference-errors-and-limits.md "Requestable limits").
    const DEFAULT_CAPS: UiCaps = UiCaps {
        max_nodes: 4096,
        max_kib: 1024,
        max_depth: 32,
    };

    fn node(key: Option<&str>, children: Vec<u32>) -> UiNode {
        UiNode {
            key: key.map(str::to_string),
            widget: Widget::Clear,
            children,
            flags: NodeFlags(0),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }
    }

    fn layout_node(constraint_count: usize, children: Vec<u32>) -> UiNode {
        let constraints: Vec<Constraint> =
            (0..constraint_count).map(|_| Constraint::Fill(1)).collect();
        UiNode {
            widget: Widget::Layout(LayoutSpec {
                direction: Direction::Horizontal,
                constraints,
                flex: Flex::Legacy,
                spacing: Spacing::Space(0),
                margin: Margin::default(),
            }),
            ..node(None, children)
        }
    }

    fn tree_of(nodes: Vec<UiNode>) -> UiTree {
        UiTree {
            version: 1,
            nodes,
            root: 0,
        }
    }

    /// A flat tree: node 0 is the root and has every other node as a child.
    fn flat_tree(node_count: u32) -> UiTree {
        let children: Vec<u32> = (1..node_count).collect();
        let mut nodes = vec![node(None, children)];
        nodes.extend((1..node_count).map(|_| node(None, vec![])));
        tree_of(nodes)
    }

    /// A line of `depth` nodes, node i the only child of node i-1 (root is level 1).
    fn chain_tree(depth: u32) -> UiTree {
        tree_of(
            (0..depth)
                .map(|index| {
                    node(
                        None,
                        if index + 1 < depth {
                            vec![index + 1]
                        } else {
                            vec![]
                        },
                    )
                })
                .collect(),
        )
    }

    /// The refusal code of a tree that must be refused; panics with the message if it is accepted.
    fn refusal_of(tree: &UiTree, caps: UiCaps) -> Code {
        check_ui_tree(tree, caps).expect_err("the tree must be refused")
    }

    /// REQ: WID-002
    #[test]
    fn single_root_node_is_a_valid_empty_tree() {
        assert_eq!(
            check_ui_tree(&tree_of(vec![node(None, vec![])]), DEFAULT_CAPS),
            Ok(())
        );
    }

    /// REQ: WID-002
    #[test]
    fn flat_tree_with_children_in_range_is_accepted() {
        assert_eq!(check_ui_tree(&flat_tree(3), DEFAULT_CAPS), Ok(()));
    }

    /// REQ: WID-002
    #[test]
    fn empty_node_list_has_no_root_and_is_refused_as_invalid_tree() {
        assert_eq!(
            refusal_of(&tree_of(vec![]), DEFAULT_CAPS),
            Code::from_static("ui.invalid-tree")
        );
    }

    /// REQ: WID-002
    #[test]
    fn nonzero_root_is_refused_as_invalid_tree() {
        let tree = UiTree {
            version: 1,
            nodes: vec![node(None, vec![1]), node(None, vec![])],
            root: 1,
        };
        assert_eq!(
            refusal_of(&tree, DEFAULT_CAPS),
            Code::from_static("ui.invalid-tree")
        );
    }

    /// REQ: WID-002
    #[test]
    fn child_index_out_of_range_is_refused_as_invalid_tree() {
        // The manual's example: child index 99 in a 5-node tree.
        let mut nodes: Vec<UiNode> = (0..5).map(|_| node(None, vec![])).collect();
        nodes[0].children = vec![99];
        assert_eq!(
            refusal_of(&tree_of(nodes), DEFAULT_CAPS),
            Code::from_static("ui.invalid-tree")
        );
    }

    /// REQ: WID-002
    #[test]
    fn backward_child_index_is_refused_as_invalid_tree() {
        let nodes = vec![
            node(None, vec![1]),
            node(None, vec![2]),
            node(None, vec![1]),
        ];
        assert_eq!(
            refusal_of(&tree_of(nodes), DEFAULT_CAPS),
            Code::from_static("ui.invalid-tree")
        );
    }

    /// REQ: WID-002
    #[test]
    fn node_with_two_parents_is_refused_as_invalid_tree() {
        let nodes = vec![
            node(None, vec![1, 2]),
            node(None, vec![3]),
            node(None, vec![3]),
            node(None, vec![]),
        ];
        assert_eq!(
            refusal_of(&tree_of(nodes), DEFAULT_CAPS),
            Code::from_static("ui.invalid-tree")
        );
    }

    /// REQ: WID-002
    #[test]
    fn unreachable_node_is_refused_as_invalid_tree() {
        // Node 2 appears in no child list.
        let nodes = vec![node(None, vec![1]), node(None, vec![]), node(None, vec![])];
        assert_eq!(
            refusal_of(&tree_of(nodes), DEFAULT_CAPS),
            Code::from_static("ui.invalid-tree")
        );
    }

    /// REQ: WID-002
    #[test]
    fn cycle_is_refused_as_invalid_tree() {
        let nodes = vec![node(None, vec![1]), node(None, vec![0])];
        assert_eq!(
            refusal_of(&tree_of(nodes), DEFAULT_CAPS),
            Code::from_static("ui.invalid-tree")
        );
    }

    /// REQ: WID-003
    #[test]
    fn duplicate_key_in_different_branches_is_refused() {
        let nodes = vec![
            node(None, vec![1, 2]),
            node(Some("left"), vec![3]),
            node(Some("right"), vec![4]),
            node(Some("save"), vec![]),
            node(Some("save"), vec![]),
        ];
        assert_eq!(
            refusal_of(&tree_of(nodes), DEFAULT_CAPS),
            Code::from_static("ui.duplicate-key")
        );
    }

    /// REQ: WID-003
    #[test]
    fn duplicate_key_between_parent_and_child_is_refused() {
        let nodes = vec![node(Some("title"), vec![1]), node(Some("title"), vec![])];
        assert_eq!(
            refusal_of(&tree_of(nodes), DEFAULT_CAPS),
            Code::from_static("ui.duplicate-key")
        );
    }

    /// REQ: WID-003
    #[test]
    fn unkeyed_nodes_never_count_as_duplicates() {
        assert_eq!(check_ui_tree(&flat_tree(50), DEFAULT_CAPS), Ok(()));
    }

    /// REQ: WID-003
    #[test]
    fn distinct_keys_across_branches_are_accepted() {
        let nodes = vec![
            node(None, vec![1, 2]),
            node(Some("left"), vec![3]),
            node(Some("right"), vec![4]),
            node(Some("save-left"), vec![]),
            node(Some("save-right"), vec![]),
        ];
        assert_eq!(check_ui_tree(&tree_of(nodes), DEFAULT_CAPS), Ok(()));
    }

    /// REQ: WID-005
    #[test]
    fn layout_with_one_constraint_per_child_is_accepted() {
        let nodes = vec![
            layout_node(2, vec![1, 2]),
            node(None, vec![]),
            node(None, vec![]),
        ];
        assert_eq!(check_ui_tree(&tree_of(nodes), DEFAULT_CAPS), Ok(()));
    }

    /// REQ: WID-005
    #[test]
    fn layout_with_more_children_than_constraints_is_refused_as_invalid_tree() {
        let nodes = vec![
            layout_node(2, vec![1, 2, 3]),
            node(None, vec![]),
            node(None, vec![]),
            node(None, vec![]),
        ];
        assert_eq!(
            refusal_of(&tree_of(nodes), DEFAULT_CAPS),
            Code::from_static("ui.invalid-tree")
        );
    }

    /// REQ: WID-005
    #[test]
    fn layout_with_no_constraints_and_children_is_refused_as_invalid_tree() {
        let nodes = vec![
            layout_node(0, vec![1, 2]),
            node(None, vec![]),
            node(None, vec![]),
        ];
        assert_eq!(
            refusal_of(&tree_of(nodes), DEFAULT_CAPS),
            Code::from_static("ui.invalid-tree")
        );
    }

    /// REQ: WID-004
    #[test]
    fn node_count_at_default_cap_is_accepted() {
        assert_eq!(check_ui_tree(&flat_tree(4096), DEFAULT_CAPS), Ok(()));
    }

    /// REQ: WID-004
    #[test]
    fn node_count_over_default_cap_is_refused_as_too_large() {
        assert_eq!(
            refusal_of(&flat_tree(4097), DEFAULT_CAPS),
            Code::from_static("ui.too-large")
        );
    }

    /// REQ: WID-004
    #[test]
    fn node_limit_follows_the_caps_passed_in() {
        let caps = UiCaps {
            max_nodes: 10,
            max_kib: 1024,
            max_depth: 32,
        };
        assert_eq!(check_ui_tree(&flat_tree(10), caps), Ok(()));
        assert_eq!(
            refusal_of(&flat_tree(11), caps),
            Code::from_static("ui.too-large")
        );
    }

    /// REQ: WID-004
    #[test]
    fn zero_node_cap_refuses_even_a_single_root() {
        let caps = UiCaps {
            max_nodes: 0,
            max_kib: 1024,
            max_depth: 32,
        };
        assert_eq!(
            refusal_of(&tree_of(vec![node(None, vec![])]), caps),
            Code::from_static("ui.too-large")
        );
    }

    /// REQ: WID-004
    #[test]
    fn encoded_size_over_kib_cap_is_refused_as_too_large() {
        // 31 keyed nodes of 64 characters each: about 2 KiB encoded, over the 1 KiB cap.
        let keys: Vec<String> = (0..31).map(|index| format!("{index:064}")).collect();
        let mut nodes = vec![node(None, (1..=31).collect())];
        nodes.extend(keys.iter().map(|key| node(Some(key), vec![])));
        let caps = UiCaps {
            max_nodes: 4096,
            max_kib: 1,
            max_depth: 32,
        };
        assert_eq!(
            refusal_of(&tree_of(nodes), caps),
            Code::from_static("ui.too-large")
        );
    }

    /// REQ: WID-004
    #[test]
    fn same_tree_fits_the_default_kib_cap() {
        let keys: Vec<String> = (0..31).map(|index| format!("{index:064}")).collect();
        let mut nodes = vec![node(None, (1..=31).collect())];
        nodes.extend(keys.iter().map(|key| node(Some(key), vec![])));
        assert_eq!(check_ui_tree(&tree_of(nodes), DEFAULT_CAPS), Ok(()));
    }

    /// REQ: WID-017
    #[test]
    fn depth_of_32_levels_is_accepted() {
        assert_eq!(check_ui_tree(&chain_tree(32), DEFAULT_CAPS), Ok(()));
    }

    /// REQ: WID-017
    #[test]
    fn depth_of_33_levels_is_refused_as_too_deep() {
        assert_eq!(
            refusal_of(&chain_tree(33), DEFAULT_CAPS),
            Code::from_static("ui.too-deep")
        );
    }
}
