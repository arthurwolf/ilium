//! Host implementation of `ilium:host/sessions` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostSessions` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::sessions::{CrossRequest, HostSessions, LiveSession};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `sessions` interface.
#[derive(Debug)]
pub struct SessionsImport;

impl HostSessions for SessionsImport {
    fn list_live(&mut self) -> Vec<LiveSession> {
        unimplemented!("host::imports::sessions::list_live")
    }

    fn send(&mut self, _request: CrossRequest) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::sessions::send")
    }

    fn take_reply(&mut self, _request: RequestId) -> Option<String> {
        unimplemented!("host::imports::sessions::take_reply")
    }
}
