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
