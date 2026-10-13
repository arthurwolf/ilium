//! Host implementation of `ilium:primitives/basic` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostBasic` trait; hand-owned afterwards.

use ilium_plugin_api::generated::primitives::basic::HostBasic;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::text::StyledText;
use ilium_plugin_api::generated::types::tree::AgentActivity;

/// Host side of the `basic` interface.
#[derive(Debug)]
pub struct BasicImport;

impl HostBasic for BasicImport {
    fn set_title(&mut self, pane: NodeId, title: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::basic::set_title")
    }

    fn set_status(&mut self, pane: NodeId, status: StyledText) -> Result<(), PluginError> {
        unimplemented!("host::imports::basic::set_status")
    }

    fn request_focus(&mut self, pane: NodeId) -> Result<(), PluginError> {
        unimplemented!("host::imports::basic::request_focus")
    }

    fn size(&mut self, pane: NodeId) -> Option<(u16, u16)> {
        unimplemented!("host::imports::basic::size")
    }

    fn report_activity(
        &mut self,
        pane: NodeId,
        activity: AgentActivity,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::basic::report_activity")
    }

    fn set_dirty(&mut self, pane: NodeId, dirty: bool) -> Result<(), PluginError> {
        unimplemented!("host::imports::basic::set_dirty")
    }
}
