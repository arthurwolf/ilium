//! Host implementation of `ilium:host/http` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostHttp` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::http::{HostHttp, HttpRequest, HttpResponse, StreamRequest};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{RequestId, StreamId};

/// Host side of the `http` interface.
#[derive(Debug)]
pub struct HttpImport;

impl HostHttp for HttpImport {
    fn fetch(&mut self, request: HttpRequest) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::http::fetch")
    }

    fn take_response(&mut self, request: RequestId) -> Option<HttpResponse> {
        unimplemented!("host::imports::http::take_response")
    }

    fn open_stream(&mut self, request: StreamRequest) -> Result<StreamId, PluginError> {
        unimplemented!("host::imports::http::open_stream")
    }

    fn close_stream(&mut self, stream: StreamId) {
        unimplemented!("host::imports::http::close_stream")
    }

    fn open_websocket(
        &mut self,
        url: String,
        protocols: Vec<String>,
    ) -> Result<StreamId, PluginError> {
        unimplemented!("host::imports::http::open_websocket")
    }

    fn send_websocket(&mut self, stream: StreamId, text: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::http::send_websocket")
    }
}
