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
    fn info(&mut self, pane: NodeId) -> Option<PaneInfo> {
        unimplemented!("host::imports::panes::info")
    }

    fn focused(&mut self) -> Option<NodeId> {
        unimplemented!("host::imports::panes::focused")
    }
}
