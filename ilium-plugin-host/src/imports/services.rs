//! Host implementation of `ilium:host/services` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostServices` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::services::HostServices;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `services` interface.
#[derive(Debug)]
pub struct ServicesImport;

impl HostServices for ServicesImport {
    fn call(
        &mut self,
        service: String,
        method: String,
        payload_json: String,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::services::call")
    }

    fn take_reply(&mut self, request: RequestId) -> Option<String> {
        unimplemented!("host::imports::services::take_reply")
    }

    fn reply(
        &mut self,
        call: u64,
        outcome: Result<String, PluginError>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::services::reply")
    }
}
