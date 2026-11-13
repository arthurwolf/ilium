//! A fake network every egress of a host under test goes through (NET-* egress tests). The
//! manual does not document this fake; bodies are `unimplemented!()` placeholders.

use std::net::IpAddr;

/// A canned HTTP response the fake network serves for one URL.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FakeResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    size: Option<u64>,
}

impl FakeResponse {
    /// A 200 response with this body.
    pub fn ok(body: Vec<u8>) -> FakeResponse {
        FakeResponse {
            status: 200,
            headers: Vec::new(),
            body,
            size: None,
        }
    }

    /// A 200 response whose body is this many bytes, streamed (NET response-size limits).
    pub fn sized(bytes: u64) -> FakeResponse {
        FakeResponse {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
            size: Some(bytes),
        }
    }

    /// Adds a response header.
    pub fn header(mut self, name: &str, value: &str) -> FakeResponse {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

/// One request the fake network saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenRequest {
    /// The requested URL.
    pub url: String,
    /// The address the connection went to.
    pub addr: IpAddr,
    /// Request headers in order.
    pub headers: Vec<(String, String)>,
    /// Request body length.
    pub body_len: u64,
    /// The proxy the request went through, if any.
    pub via_proxy: Option<String>,
    /// The negotiated TLS version, if any.
    pub tls_version: Option<String>,
}

/// The fake network; clones share one state so a harness and its test see the same traffic.
#[derive(Debug, Clone, Default)]
pub struct FakeNet;

impl FakeNet {
    /// An empty network that resolves nothing and serves nothing.
    pub fn new() -> FakeNet {
        FakeNet
    }

    /// Resolves a host name to these addresses.
    pub fn dns(&self, _host: &str, _addresses: &[&str]) {
        unimplemented!("host::testing::FakeNet::dns")
    }

    /// Resolves a host name to the first addresses once, then to the second (NET DNS rebinding).
    pub fn dns_then(&self, _host: &str, _first: &[&str], _then: &[&str]) {
        unimplemented!("host::testing::FakeNet::dns_then")
    }

    /// Serves this response for a URL.
    pub fn serve(&self, _url: &str, _response: FakeResponse) {
        unimplemented!("host::testing::FakeNet::serve")
    }

    /// Redirects a URL with this status to another URL.
    pub fn redirect(&self, _from: &str, _status: u16, _to: &str) {
        unimplemented!("host::testing::FakeNet::redirect")
    }

    /// Makes the TLS handshake with a host fail.
    pub fn fail_tls(&self, _host: &str) {
        unimplemented!("host::testing::FakeNet::fail_tls")
    }

    /// The TLS version a host negotiates.
    pub fn set_tls_version(&self, _host: &str, _version: &str) {
        unimplemented!("host::testing::FakeNet::set_tls_version")
    }

    /// A host's certificate chains to this root name.
    pub fn require_root(&self, _host: &str, _root: &str) {
        unimplemented!("host::testing::FakeNet::require_root")
    }

    /// A trust bundle file at this path holds this root name.
    pub fn trust_bundle(&self, _path: &str, _root: &str) {
        unimplemented!("host::testing::FakeNet::trust_bundle")
    }

    /// Marks a loopback port as this Ilium session's own (NET reserved ports).
    pub fn reserve_ilium_port(&self, _port: u16) {
        unimplemented!("host::testing::FakeNet::reserve_ilium_port")
    }

    /// Marks a loopback port as another Ilium session's (NET reserved ports).
    pub fn reserve_ilium_port_of_other_session(&self, _port: u16) {
        unimplemented!("host::testing::FakeNet::reserve_ilium_port_of_other_session")
    }

    /// Every request seen, in order.
    pub fn seen(&self) -> Vec<SeenRequest> {
        unimplemented!("host::testing::FakeNet::seen")
    }

    /// Host names resolved so far, in order.
    pub fn resolutions(&self) -> Vec<String> {
        unimplemented!("host::testing::FakeNet::resolutions")
    }

    /// How often a host name was resolved.
    pub fn resolve_count(&self, _host: &str) -> u64 {
        unimplemented!("host::testing::FakeNet::resolve_count")
    }
}
