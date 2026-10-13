//! Host implementation of `ilium:host/kernel-settings` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostKernelSettings` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::kernel_settings::HostKernelSettings;
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `kernel-settings` interface.
#[derive(Debug)]
pub struct KernelSettingsImport;

impl HostKernelSettings for KernelSettingsImport {
    fn read(&mut self, tab: String) -> Result<String, PluginError> {
        unimplemented!("host::imports::kernel_settings::read")
    }
}
