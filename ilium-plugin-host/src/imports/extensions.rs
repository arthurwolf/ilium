//! Host implementation of `ilium:host/extensions` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostExtensions` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::extensions::{
    ExtensionEntry, ExtensionError, HostExtensions,
};
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `extensions` interface.
#[derive(Debug)]
pub struct ExtensionsImport;

impl HostExtensions for ExtensionsImport {
    fn list(&mut self, _point: String) -> Result<Vec<ExtensionEntry>, ExtensionError> {
        unimplemented!("host::imports::extensions::list")
    }

    fn run_hook(
        &mut self,
        _point: String,
        _payload_json: String,
    ) -> Result<RequestId, ExtensionError> {
        unimplemented!("host::imports::extensions::run_hook")
    }

    fn take_reply(&mut self, _request: RequestId) -> Option<String> {
        unimplemented!("host::imports::extensions::take_reply")
    }

    fn register(
        &mut self,
        _point: String,
        _extension: String,
        _priority: u8,
        _entry_json: String,
    ) -> Result<(), ExtensionError> {
        unimplemented!("host::imports::extensions::register")
    }

    fn unregister(&mut self, _point: String, _extension: String) -> Result<(), ExtensionError> {
        unimplemented!("host::imports::extensions::unregister")
    }
}
