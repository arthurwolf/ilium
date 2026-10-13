//! Host implementation of `ilium:host/plugins` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostPlugins` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::plugins::{HostPlugins, PluginInfo};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::PluginId;

/// Host side of the `plugins` interface.
#[derive(Debug)]
pub struct PluginsImport;

impl HostPlugins for PluginsImport {
    fn list(&mut self) -> Vec<PluginInfo> {
        unimplemented!("host::imports::plugins::list")
    }

    fn set_enabled(
        &mut self,
        plugin: PluginId,
        enabled: bool,
        scope: String,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::plugins::set_enabled")
    }
}
