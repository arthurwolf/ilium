//! Fake network (reference-test-harness.md "Fake network"). No real call ever happens.

use ilium_plugin_sdk_core::generated::host::{HttpRequest, HttpResponse, StreamRequest};
use ilium_plugin_sdk_core::types::StreamId;

/// Handle on the fake HTTP client of a host; the fixture key `http` fills it.
#[derive(Debug, Default)]
pub struct FakeHttp {
    entries: Vec<(String, String, HttpResponse)>,
    calls: Vec<HttpRequest>,
    streams: Vec<(StreamId, StreamRequest)>,
}

impl FakeHttp {
    /// Registers the response for requests with this method and URL.
    pub fn expect(&mut self, method: &str, url: &str, response: HttpResponse) {
        self.entries
            .push((method.to_ascii_uppercase(), url.to_string(), response));
    }

    /// Every request made, in order.
    pub fn calls(&self) -> Vec<HttpRequest> {
        self.calls.clone()
    }

    /// Every stream the plugin opened with `open-stream`, in order (lane hook, see manual_gaps_fakes.md; DEV-462).
    pub fn streams(&self) -> Vec<(StreamId, StreamRequest)> {
        self.streams.clone()
    }

    /// Answers a request from the registered entries and records it; `None` when unregistered.
    pub(crate) fn respond(&mut self, request: &HttpRequest) -> Option<HttpResponse> {
        self.calls.push(request.clone());
        let method = request.method.to_ascii_uppercase();
        self.entries
            .iter()
            .rev()
            .find(|(entry_method, url, _)| *entry_method == method && *url == request.url)
            .map(|(_, _, response)| response.clone())
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::host::FakeHost;

    /// REQ: DEV-462
    #[test]
    fn dev_462_registered_entry_makes_no_network_call() {
        let mut host = FakeHost::new();
        host.http().expect(
            "GET",
            "https://api.example.com/v1/items",
            HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: b"[]".to_vec(),
            },
        );
        assert!(host.http().calls().is_empty());
    }

    /// REQ: DEV-462
    #[test]
    fn dev_462_fresh_host_has_no_calls_and_no_streams() {
        let mut host = FakeHost::new();
        assert!(host.http().calls().is_empty());
        assert!(host.http().streams().is_empty());
    }

    /// REQ: DEV-462
    #[test]
    fn dev_462_several_entries_register_without_calls() {
        let mut host = FakeHost::new();
        host.http().expect(
            "GET",
            "https://api.example.com/v1/items",
            HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: b"[]".to_vec(),
            },
        );
        host.http().expect(
            "POST",
            "https://api.example.com/v1/items",
            HttpResponse {
                status: 201,
                headers: Vec::new(),
                body: Vec::new(),
            },
        );
        assert!(host.http().calls().is_empty());
        assert!(host.http().streams().is_empty());
    }

    /// REQ: DEV-462
    #[test]
    fn dev_462_error_status_entry_registers_without_call() {
        let mut host = FakeHost::new();
        host.http().expect(
            "GET",
            "https://api.example.com/v1/down",
            HttpResponse {
                status: 503,
                headers: Vec::new(),
                body: b"unavailable".to_vec(),
            },
        );
        assert!(host.http().calls().is_empty());
    }

    /// REQ: DEV-462
    #[test]
    fn dev_462_streams_stay_empty_when_only_entries_are_registered() {
        let mut host = FakeHost::new();
        host.http().expect(
            "GET",
            "https://feed.example.com/events",
            HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
            },
        );
        let streams: Vec<(StreamId, StreamRequest)> = host.http().streams();
        assert!(streams.is_empty());
    }
}
