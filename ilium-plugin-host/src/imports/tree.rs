//! Host implementation of `ilium:host/tree` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostTree` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::tree::HostTree;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::tree::{NodeQuery, NodeView};

/// Host side of the `tree` interface.
#[derive(Debug)]
pub struct TreeImport;

impl HostTree for TreeImport {
    fn get(&mut self, _node: NodeId) -> Option<NodeView> {
        unimplemented!("host::imports::tree::get")
    }

    fn children(&mut self, _node: NodeId) -> Vec<NodeId> {
        unimplemented!("host::imports::tree::children")
    }

    fn roots(&mut self) -> Vec<NodeId> {
        unimplemented!("host::imports::tree::roots")
    }

    fn find(&mut self, _query: NodeQuery) -> Vec<NodeId> {
        unimplemented!("host::imports::tree::find")
    }

    fn metadata(&mut self, _node: NodeId) -> Option<Vec<u8>> {
        unimplemented!("host::imports::tree::metadata")
    }

    fn set_metadata(&mut self, _node: NodeId, _bytes: Vec<u8>) -> Result<(), PluginError> {
        unimplemented!("host::imports::tree::set_metadata")
    }
}
