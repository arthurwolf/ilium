//! Host implementation of `ilium:host/commands` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostCommands` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::commands::HostCommands;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `commands` interface.
#[derive(Debug)]
pub struct CommandsImport;

impl HostCommands for CommandsImport {
    fn set_state(
        &mut self,
        command: String,
        enabled: bool,
        checked: Option<bool>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::commands::set_state")
    }

    fn invoke(
        &mut self,
        command: String,
        arguments_json: String,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::commands::invoke")
    }
}
