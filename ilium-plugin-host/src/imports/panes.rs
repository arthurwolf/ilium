//! Host implementation of `ilium:host/panes` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostPanes` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::panes::HostPanes;
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::tree::PaneInfo;

/// Host side of the `panes` interface.
#[derive(Debug)]
pub struct PanesImport;

impl HostPanes for PanesImport {
    fn info(&mut self, _pane: NodeId) -> Option<PaneInfo> {
        unimplemented!("host::imports::panes::info")
    }

    fn focused(&mut self) -> Option<NodeId> {
        unimplemented!("host::imports::panes::focused")
    }
}

#[cfg(test)]
mod tests {
    use super::PanesImport;
    use ilium_plugin_api::generated::host::panes::HostPanes;
    use ilium_plugin_api::generated::types::ids::NodeId;

    /// Boundary: node id 0 is never a pane, so `info` returns `None` rather than an error.
    /// REQ: EVT-013
    #[test]
    fn info_returns_none_for_node_zero() {
        let mut panes_import = PanesImport;
        assert_eq!(panes_import.info(NodeId(0)), None);
    }

    /// Boundary: the largest node id names no node in an empty tree, so `info` returns `None`.
    /// REQ: EVT-013
    #[test]
    fn info_returns_none_for_maximum_node_id() {
        let mut panes_import = PanesImport;
        assert_eq!(panes_import.info(NodeId(u64::MAX)), None);
    }

    /// Empty input: with no pane in the tree, a lookup of the first ordinary node id returns `None`.
    /// REQ: EVT-013
    #[test]
    fn info_returns_none_for_unknown_node_in_empty_tree() {
        let mut panes_import = PanesImport;
        assert_eq!(panes_import.info(NodeId(1)), None);
    }
}
