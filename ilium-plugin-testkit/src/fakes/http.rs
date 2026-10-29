//! Fake network (reference-test-harness.md "Fake network"). No real call ever happens.

use ilium_plugin_sdk_core::generated::host::{HttpRequest, HttpResponse, StreamRequest};
use ilium_plugin_sdk_core::types::StreamId;

/// Handle on the fake HTTP client of a host; the fixture key `http` fills it.
#[derive(Debug)]
pub struct FakeHttp {
    _private: (),
}

impl FakeHttp {
    /// Registers the response for requests with this method and URL.
    pub fn expect(&mut self, _method: &str, _url: &str, _response: HttpResponse) {
        unimplemented!("DEV-462")
    }

    /// Every request made, in order.
    pub fn calls(&self) -> Vec<HttpRequest> {
        unimplemented!("DEV-462")
    }

    /// Every stream the plugin opened with `open-stream`, in order (lane hook, see manual_gaps_fakes.md; DEV-462).
    pub fn streams(&self) -> Vec<(StreamId, StreamRequest)> {
        unimplemented!("DEV-462")
    }
}
