//! Host implementation of `ilium:host/keys` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostKeys` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::keys::{GrabSpec, HostKeys};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::GrabId;

/// Host side of the `keys` interface.
#[derive(Debug)]
pub struct KeysImport;

impl HostKeys for KeysImport {
    fn grab(&mut self, spec: GrabSpec) -> Result<GrabId, PluginError> {
        unimplemented!("host::imports::keys::grab")
    }

    fn release(&mut self, grab: GrabId) {
        unimplemented!("host::imports::keys::release")
    }
}
