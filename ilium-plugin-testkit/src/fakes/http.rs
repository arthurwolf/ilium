//! Fake network (reference-test-harness.md "Fake network"). No real call ever happens.

use ilium_plugin_sdk_core::generated::host::{HttpRequest, HttpResponse};

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
}
