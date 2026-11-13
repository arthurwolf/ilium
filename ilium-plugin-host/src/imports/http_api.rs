//! Host implementation of `ilium:host/http-api` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostHttpApi` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::http_api::HostHttpApi;
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `http-api` interface.
#[derive(Debug)]
pub struct HttpApiImport;

impl HostHttpApi for HttpApiImport {
    fn respond(
        &mut self,
        _call: u64,
        _status: u16,
        _headers: Vec<(String, String)>,
        _body: Vec<u8>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::http_api::respond")
    }
}
