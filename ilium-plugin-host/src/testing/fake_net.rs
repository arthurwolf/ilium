//! A fake network every egress of a host under test goes through (NET-* egress tests). The
//! manual does not document this fake; it stores the configured DNS, HTTP, TLS and port rules
//! and the traffic the egress path records.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

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

/// Configuration and recorded traffic of the fake network.
#[derive(Debug, Default)]
struct NetState {
    /// Host name to the addresses it resolves to.
    dns: BTreeMap<String, Vec<String>>,
    /// Host name to (first answer, answer from the second resolution on).
    dns_rebinding: BTreeMap<String, (Vec<String>, Vec<String>)>,
    /// Exact URL to the response served for it.
    responses: BTreeMap<String, FakeResponse>,
    /// Exact URL to (status, location).
    redirects: BTreeMap<String, (u16, String)>,
    tls_failures: BTreeSet<String>,
    /// Host name to the TLS version it negotiates.
    tls_versions: BTreeMap<String, String>,
    /// Host name to the root name its certificate must chain to.
    required_roots: BTreeMap<String, String>,
    /// Trust bundle path to the root name it holds.
    trust_bundles: BTreeMap<String, String>,
    reserved_ports: BTreeSet<u16>,
    other_session_ports: BTreeSet<u16>,
    seen: Vec<SeenRequest>,
    resolutions: Vec<String>,
}

/// The fake network; clones share one state so a harness and its test see the same traffic.
#[derive(Debug, Clone, Default)]
pub struct FakeNet {
    state: Arc<Mutex<NetState>>,
}

impl FakeNet {
    /// An empty network that resolves nothing and serves nothing.
    pub fn new() -> FakeNet {
        FakeNet::default()
    }

    /// Locks the shared state; a poisoned lock still holds consistent maps, so it is reused.
    fn state(&self) -> MutexGuard<'_, NetState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Resolves a host name to these addresses.
    pub fn dns(&self, host: &str, addresses: &[&str]) {
        self.state().dns.insert(host.to_owned(), owned_list(addresses));
    }

    /// Resolves a host name to the first addresses once, then to the second (NET DNS rebinding).
    pub fn dns_then(&self, host: &str, first: &[&str], then: &[&str]) {
        self.state()
            .dns_rebinding
            .insert(host.to_owned(), (owned_list(first), owned_list(then)));
    }

    /// Serves this response for a URL.
    pub fn serve(&self, url: &str, response: FakeResponse) {
        self.state().responses.insert(url.to_owned(), response);
    }

    /// Redirects a URL with this status to another URL.
    pub fn redirect(&self, from: &str, status: u16, to: &str) {
        self.state()
            .redirects
            .insert(from.to_owned(), (status, to.to_owned()));
    }

    /// Makes the TLS handshake with a host fail.
    pub fn fail_tls(&self, host: &str) {
        self.state().tls_failures.insert(host.to_owned());
    }

    /// The TLS version a host negotiates.
    pub fn set_tls_version(&self, host: &str, version: &str) {
        self.state()
            .tls_versions
            .insert(host.to_owned(), version.to_owned());
    }

    /// A host's certificate chains to this root name.
    pub fn require_root(&self, host: &str, root: &str) {
        self.state()
            .required_roots
            .insert(host.to_owned(), root.to_owned());
    }

    /// A trust bundle file at this path holds this root name.
    pub fn trust_bundle(&self, path: &str, root: &str) {
        self.state()
            .trust_bundles
            .insert(path.to_owned(), root.to_owned());
    }

    /// Marks a loopback port as this Ilium session's own (NET reserved ports).
    pub fn reserve_ilium_port(&self, port: u16) {
        self.state().reserved_ports.insert(port);
    }

    /// Marks a loopback port as another Ilium session's (NET reserved ports).
    pub fn reserve_ilium_port_of_other_session(&self, port: u16) {
        self.state().other_session_ports.insert(port);
    }

    /// Every request seen, in order.
    pub fn seen(&self) -> Vec<SeenRequest> {
        self.state().seen.clone()
    }

    /// Host names resolved so far, in order.
    pub fn resolutions(&self) -> Vec<String> {
        self.state().resolutions.clone()
    }

    /// How often a host name was resolved.
    pub fn resolve_count(&self, host: &str) -> u64 {
        let count = self
            .state()
            .resolutions
            .iter()
            .filter(|resolved| resolved.as_str() == host)
            .count();
        u64::try_from(count).unwrap_or(u64::MAX)
    }
}

fn owned_list(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::{FakeNet, FakeResponse, SeenRequest};

    #[test]
    fn ok_response_is_200_with_body_and_no_declared_size() {
        let response: FakeResponse = FakeResponse::ok(b"hello".to_vec());

        assert_eq!(
            response,
            FakeResponse {
                status: 200,
                headers: Vec::new(),
                body: b"hello".to_vec(),
                size: None,
            }
        );
    }

    #[test]
    fn ok_response_with_empty_body_is_200_and_empty() {
        let response: FakeResponse = FakeResponse::ok(Vec::new());

        assert_eq!(
            response,
            FakeResponse {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
                size: None,
            }
        );
    }

    /// REQ: SEC-082
    #[test]
    fn sized_response_declares_size_and_buffers_no_body() {
        let response: FakeResponse = FakeResponse::sized(8 * 1024 * 1024);

        assert_eq!(
            response,
            FakeResponse {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
                size: Some(8 * 1024 * 1024),
            }
        );
    }

    /// REQ: SEC-082
    #[test]
    fn sized_zero_bytes_is_a_valid_empty_stream() {
        let response: FakeResponse = FakeResponse::sized(0);

        assert_eq!(response.size, Some(0));
        assert!(response.body.is_empty());
    }

    /// REQ: SEC-082
    #[test]
    fn sized_u64_max_keeps_the_full_declared_size() {
        let response: FakeResponse = FakeResponse::sized(u64::MAX);

        assert_eq!(response.size, Some(u64::MAX));
        assert_eq!(response.status, 200);
    }

    #[test]
    fn header_appends_in_call_order() {
        let response: FakeResponse = FakeResponse::ok(Vec::new())
            .header("Content-Type", "text/plain")
            .header("X-Trace", "1");

        assert_eq!(
            response.headers,
            vec![
                ("Content-Type".to_owned(), "text/plain".to_owned()),
                ("X-Trace".to_owned(), "1".to_owned()),
            ]
        );
    }

    #[test]
    fn header_keeps_duplicate_names_and_empty_values() {
        let response: FakeResponse = FakeResponse::ok(Vec::new())
            .header("Set-Cookie", "")
            .header("Set-Cookie", "a=b");

        assert_eq!(
            response.headers,
            vec![
                ("Set-Cookie".to_owned(), String::new()),
                ("Set-Cookie".to_owned(), "a=b".to_owned()),
            ]
        );
    }

    #[test]
    fn new_network_has_no_traffic_and_no_resolutions() {
        let net: FakeNet = FakeNet::new();

        let seen: Vec<SeenRequest> = net.seen();
        assert!(seen.is_empty());
        assert!(net.resolutions().is_empty());
    }

    #[test]
    fn unconfigured_host_has_zero_resolutions() {
        let net: FakeNet = FakeNet::new();

        assert_eq!(net.resolve_count("never-configured.example"), 0);
    }

    #[test]
    fn empty_host_name_has_zero_resolutions() {
        let net: FakeNet = FakeNet::new();

        assert_eq!(net.resolve_count(""), 0);
    }

    #[test]
    fn clone_observes_the_same_traffic_as_the_original() {
        let net: FakeNet = FakeNet::new();
        let copy: FakeNet = net.clone();

        assert_eq!(copy.seen(), net.seen());
        assert_eq!(copy.resolutions(), net.resolutions());
    }

    /// REQ: SEC-072
    #[test]
    fn dns_with_no_addresses_records_no_traffic() {
        let net: FakeNet = FakeNet::new();

        net.dns("empty-answer.example", &[]);

        assert!(net.seen().is_empty());
        assert!(net.resolutions().is_empty());
    }

    /// REQ: SEC-073
    #[test]
    fn dns_then_configuration_does_not_resolve_the_name() {
        let net: FakeNet = FakeNet::new();

        net.dns_then("rebind.example", &["203.0.113.5"], &["127.0.0.1"]);

        assert_eq!(net.resolve_count("rebind.example"), 0);
        assert!(net.resolutions().is_empty());
    }

    /// REQ: SEC-073
    #[test]
    fn dns_then_accepts_empty_first_and_second_answers() {
        let net: FakeNet = FakeNet::new();

        net.dns_then("empty.example", &[], &[]);

        assert_eq!(net.resolve_count("empty.example"), 0);
    }

    #[test]
    fn serve_with_empty_url_records_no_traffic() {
        let net: FakeNet = FakeNet::new();

        net.serve("", FakeResponse::ok(b"body".to_vec()));

        assert!(net.seen().is_empty());
    }

    /// REQ: SEC-075
    #[test]
    fn redirect_configuration_records_no_traffic() {
        let net: FakeNet = FakeNet::new();

        net.redirect("https://a.example/start", 308, "https://b.example/next");

        assert!(net.seen().is_empty());
    }

    /// REQ: SEC-077
    #[test]
    fn fail_tls_records_no_traffic_or_resolution() {
        let net: FakeNet = FakeNet::new();

        net.fail_tls("tls-broken.example");

        assert!(net.seen().is_empty());
        assert_eq!(net.resolve_count("tls-broken.example"), 0);
    }

    /// REQ: SEC-077
    #[test]
    fn set_tls_version_accepts_tls13_without_traffic() {
        let net: FakeNet = FakeNet::new();

        net.set_tls_version("secure.example", "TLS1.3");

        assert!(net.seen().is_empty());
    }

    /// REQ: SEC-077
    #[test]
    fn require_root_and_trust_bundle_leave_traffic_empty() {
        let net: FakeNet = FakeNet::new();

        net.require_root("internal.example", "Example Root CA");
        net.trust_bundle("/etc/ilium/ca_bundle.pem", "Example Root CA");

        assert!(net.seen().is_empty());
        assert!(net.resolutions().is_empty());
    }

    /// REQ: SEC-074
    #[test]
    fn reserve_ilium_port_boundary_ports_record_no_traffic() {
        let net: FakeNet = FakeNet::new();

        net.reserve_ilium_port(0);
        net.reserve_ilium_port(u16::MAX);

        assert!(net.seen().is_empty());
    }

    /// REQ: SEC-074
    #[test]
    fn reserve_other_session_port_records_no_traffic() {
        let net: FakeNet = FakeNet::new();

        net.reserve_ilium_port_of_other_session(7878);

        assert!(net.seen().is_empty());
        assert_eq!(net.resolve_count("localhost"), 0);
    }
}
