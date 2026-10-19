//! Host implementation of `ilium:host/client-requests` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostClientRequests` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::client_requests::HostClientRequests;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{ClientId, RequestId};

/// Host side of the `client-requests` interface.
#[derive(Debug)]
pub struct ClientRequestsImport;

impl HostClientRequests for ClientRequestsImport {
    fn request(
        &mut self,
        _client: ClientId,
        _kind: String,
        _payload_json: String,
        _timeout_millis: u32,
    ) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::client_requests::request")
    }

    fn take_answer(&mut self, _request: RequestId) -> Option<String> {
        unimplemented!("host::imports::client_requests::take_answer")
    }

    fn respond(
        &mut self,
        _call: u64,
        _outcome: Result<String, PluginError>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::client_requests::respond")
    }
}
