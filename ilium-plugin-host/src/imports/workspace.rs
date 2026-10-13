//! Host implementation of `ilium:host/workspace` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostWorkspace` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::workspace::{HostWorkspace, WorkspaceInfo, WorkspaceSpec};
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `workspace` interface.
#[derive(Debug)]
pub struct WorkspaceImport;

impl HostWorkspace for WorkspaceImport {
    fn register(&mut self, spec: WorkspaceSpec) -> Result<u64, PluginError> {
        unimplemented!("host::imports::workspace::register")
    }

    fn release(&mut self, workspace: u64) -> Result<(), PluginError> {
        unimplemented!("host::imports::workspace::release")
    }

    fn list(&mut self) -> Vec<WorkspaceInfo> {
        unimplemented!("host::imports::workspace::list")
    }
}
