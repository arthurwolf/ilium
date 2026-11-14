//! Host implementation of `ilium:host/http-api`.
//!
//! `HttpApiImport` is the standalone import with its own open-call set (the test harness and
//! single-process embedders). `RoutedHttpApi` is the import of a real server: a response is
//! handed to an `HttpResponseSink`, which completes the HTTP request that the route
//! dispatcher queued as `http-called` (REQ API-005, api-http-api.md "Surface" and "Limits").

use ilium_plugin_api::generated::host::http_api::HostHttpApi;
use ilium_plugin_api::generated::types::errors::PluginError;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

/// Longest response body `respond` accepts: 8 MiB.
const MAX_RESPONSE_BODY_BYTES: usize = 8 * 1024 * 1024;

/// Hop-by-hop header names a response may not carry (compared case-insensitively).
const HOP_BY_HOP_HEADERS: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// One answer a plugin gave to an open call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status, 100 to 599.
    pub status: u16,
    /// Response headers, none of them hop-by-hop.
    pub headers: Vec<(String, String)>,
    /// Response body, at most 8 MiB.
    pub body: Vec<u8>,
}

/// Where a server completes the HTTP request behind an open call.
pub trait HttpResponseSink: Send + Sync {
    /// Completes `call` of `plugin` with `response`. False when the plugin has no such open
    /// call: it never existed, another plugin owns it, it was answered already, or the
    /// request timed out.
    fn complete(&self, plugin: &str, call: u64, response: HttpResponse) -> bool;
}

/// Builds an `invalid-argument` error with `message`.
fn invalid_argument(message: &str) -> PluginError {
    PluginError {
        code: ilium_plugin_api::generated::types::errors::ErrorCode::InvalidArgument,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// Checks the status, body size and headers of one response (api-http-api.md "Limits").
pub fn validate_response(
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<(), PluginError> {
    if !(100..=599).contains(&status) {
        return Err(invalid_argument("response status must be 100 to 599"));
    }
    if body.len() > MAX_RESPONSE_BODY_BYTES {
        return Err(invalid_argument("response body exceeds 8 MiB"));
    }
    let hop_by_hop = headers.iter().any(|(name, _)| {
        HOP_BY_HOP_HEADERS
            .iter()
            .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
    });
    if hop_by_hop {
        return Err(invalid_argument("hop-by-hop response headers are refused"));
    }
    Ok(())
}

/// The `not-found` error of an unknown, foreign or already answered call.
fn unknown_call() -> PluginError {
    PluginError {
        code: ilium_plugin_api::generated::types::errors::ErrorCode::NotFound,
        message: "no open request has this call id".to_owned(),
        retry_after_ms: None,
    }
}

/// Host side of the `http-api` interface over the open-call set of one plugin instance.
/// Cloning shares the set, so the dispatcher that opens a call and the instance that answers it
/// see the same calls.
#[derive(Debug, Clone, Default)]
pub struct HttpApiImport {
    /// Route calls the router delivered and the plugin has not yet answered.
    open_calls: Arc<Mutex<HashSet<u64>>>,
}

impl HttpApiImport {
    /// Registers `call` as an open request awaiting exactly one `respond` (called by the route
    /// dispatcher).
    pub fn open_call(&self, call: u64) {
        self.open_calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(call);
    }
}

impl HostHttpApi for HttpApiImport {
    fn respond(
        &mut self,
        call: u64,
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Result<(), PluginError> {
        validate_response(status, &headers, &body)?;
        let was_open = self
            .open_calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&call);
        if was_open {
            Ok(())
        } else {
            Err(unknown_call())
        }
    }
}

/// The `http-api` import of one plugin instance in a server: `respond` completes the HTTP
/// request through the server's sink.
pub struct RoutedHttpApi {
    plugin: String,
    sink: Arc<dyn HttpResponseSink>,
}

impl RoutedHttpApi {
    /// An import for `plugin` that answers through `sink`.
    pub fn new(plugin: &str, sink: Arc<dyn HttpResponseSink>) -> Self {
        Self {
            plugin: plugin.to_owned(),
            sink,
        }
    }
}

impl HostHttpApi for RoutedHttpApi {
    fn respond(
        &mut self,
        call: u64,
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Result<(), PluginError> {
        validate_response(status, &headers, &body)?;
        let response = HttpResponse {
            status,
            headers,
            body,
        };
        if self.sink.complete(&self.plugin, call, response) {
            Ok(())
        } else {
            Err(unknown_call())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HttpApiImport;
    use ilium_plugin_api::generated::host::http_api::HostHttpApi;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

    /// A call id no open request holds. The WIT doc says an unknown or answered call gets `not-found`.
    const UNKNOWN_CALL: u64 = u64::MAX;

    /// Response body limit: 8 MiB (api-http-api.md, Limits; capability-http-api-route.md, Limits).
    const MAX_RESPONSE_BODY_BYTES: usize = 8 * 1024 * 1024;

    /// Hop-by-hop headers `respond` must refuse (api-http-api.md, Limits; capability-http-api-route.md).
    const HOP_BY_HOP_HEADER_NAMES: [&str; 8] = [
        "Connection",
        "Keep-Alive",
        "Proxy-Authenticate",
        "Proxy-Authorization",
        "TE",
        "Trailer",
        "Transfer-Encoding",
        "Upgrade",
    ];

    /// Returns the error code of a failed call, or `None` when the call succeeded.
    fn error_code_of(result: Result<(), PluginError>) -> Option<ErrorCode> {
        result.err().map(|error| error.code)
    }

    /// Builds a one-entry response header list.
    fn single_header(name: &str) -> Vec<(String, String)> {
        vec![(name.to_string(), "value".to_string())]
    }

    /// True when `respond` did not refuse the input as `invalid-argument` (success or any other code).
    fn is_not_invalid_argument(result: Result<(), PluginError>) -> bool {
        error_code_of(result) != Some(ErrorCode::InvalidArgument)
    }

    #[test]
    fn an_opened_call_is_answered_once_by_its_instance_only() {
        let api = HttpApiImport::default();
        api.open_call(7);
        let mut other_instance = HttpApiImport::default();
        assert_eq!(
            error_code_of(other_instance.respond(7, 200, Vec::new(), Vec::new())),
            Some(ErrorCode::NotFound)
        );
        let mut answering = api.clone();
        assert_eq!(answering.respond(7, 200, Vec::new(), Vec::new()), Ok(()));
        assert_eq!(
            error_code_of(answering.respond(7, 200, Vec::new(), Vec::new())),
            Some(ErrorCode::NotFound)
        );
    }

    /// Source: api-http-api.md "Surface" and reference-wit.md 5.24: each respond answers one open call; an unknown call gets not-found.
    #[test]
    fn respond_to_unknown_call_returns_not_found() {
        let mut api = HttpApiImport::default();
        let result = api.respond(UNKNOWN_CALL, 200, Vec::new(), Vec::new());
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// Source: reference-wit.md 5.24: `not-found` covers an unknown call id; a valid status must not be reported as invalid.
    #[test]
    fn respond_to_call_zero_without_open_request_returns_not_found() {
        let mut api = HttpApiImport::default();
        let result = api.respond(0, 204, Vec::new(), Vec::new());
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// Source: api-http-api.md "Limits": response status 100 to 599 is accepted (lower boundary 100).
    #[test]
    fn respond_status_100_lower_boundary_is_not_rejected() {
        let mut api = HttpApiImport::default();
        let result = api.respond(UNKNOWN_CALL, 100, Vec::new(), Vec::new());
        assert!(is_not_invalid_argument(result));
    }

    /// Source: api-http-api.md "Limits": response status 100 to 599 is accepted (upper boundary 599).
    #[test]
    fn respond_status_599_upper_boundary_is_not_rejected() {
        let mut api = HttpApiImport::default();
        let result = api.respond(UNKNOWN_CALL, 599, Vec::new(), Vec::new());
        assert!(is_not_invalid_argument(result));
    }

    /// Source: api-http-api.md "Limits": status 99 is below 100 and gives invalid-argument.
    #[test]
    fn respond_status_99_one_below_range_is_invalid_argument() {
        let mut api = HttpApiImport::default();
        let result = api.respond(UNKNOWN_CALL, 99, Vec::new(), Vec::new());
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// Source: api-http-api.md "Limits": status 600 is above 599 and gives invalid-argument.
    #[test]
    fn respond_status_600_one_above_range_is_invalid_argument() {
        let mut api = HttpApiImport::default();
        let result = api.respond(UNKNOWN_CALL, 600, Vec::new(), Vec::new());
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// Source: api-http-api.md "Limits": status 0 is outside 100 to 599 and gives invalid-argument.
    #[test]
    fn respond_status_zero_is_invalid_argument() {
        let mut api = HttpApiImport::default();
        let result = api.respond(UNKNOWN_CALL, 0, Vec::new(), Vec::new());
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// Source: api-http-api.md "Limits": the largest u16 status is outside 100 to 599 and gives invalid-argument.
    #[test]
    fn respond_status_u16_max_is_invalid_argument() {
        let mut api = HttpApiImport::default();
        let result = api.respond(UNKNOWN_CALL, u16::MAX, Vec::new(), Vec::new());
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// Source: api-http-api.md "Limits": response body is at most 8 MiB; exactly 8 MiB is within the limit.
    #[test]
    fn respond_body_exactly_8_mib_is_not_rejected() {
        let mut api = HttpApiImport::default();
        let body = vec![0u8; MAX_RESPONSE_BODY_BYTES];
        let result = api.respond(UNKNOWN_CALL, 200, Vec::new(), body);
        assert!(is_not_invalid_argument(result));
    }

    /// Source: api-http-api.md "Limits": a response body one byte over 8 MiB gives invalid-argument.
    #[test]
    fn respond_body_one_byte_over_8_mib_is_invalid_argument() {
        let mut api = HttpApiImport::default();
        let body = vec![0u8; MAX_RESPONSE_BODY_BYTES + 1];
        let result = api.respond(UNKNOWN_CALL, 200, Vec::new(), body);
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// Source: api-http-api.md "Limits" and "Errors from respond": each hop-by-hop header gives invalid-argument.
    #[test]
    fn respond_each_hop_by_hop_header_is_invalid_argument() {
        for header_name in HOP_BY_HOP_HEADER_NAMES {
            let mut api = HttpApiImport::default();
            let result = api.respond(UNKNOWN_CALL, 200, single_header(header_name), Vec::new());
            assert_eq!(
                error_code_of(result),
                Some(ErrorCode::InvalidArgument),
                "hop-by-hop header {header_name} must be refused"
            );
        }
    }

    /// Source: capability-http-api-route.md "Limits": the hop-by-hop check ignores case.
    #[test]
    fn respond_hop_by_hop_header_name_in_upper_case_is_invalid_argument() {
        let mut api = HttpApiImport::default();
        let result = api.respond(
            UNKNOWN_CALL,
            200,
            single_header("TRANSFER-ENCODING"),
            Vec::new(),
        );
        assert_eq!(error_code_of(result), Some(ErrorCode::InvalidArgument));
    }

    /// Source: capability-http-api-route.md "Limits": a header that is not hop-by-hop, such as content-type, is accepted.
    #[test]
    fn respond_content_type_header_is_not_rejected() {
        let mut api = HttpApiImport::default();
        let result = api.respond(
            UNKNOWN_CALL,
            200,
            single_header("content-type"),
            br#"{"id":"n-1"}"#.to_vec(),
        );
        assert!(is_not_invalid_argument(result));
    }

    /// Source: api-http-api.md "Example": a 201 JSON reply with a content-type header is a valid response shape.
    #[test]
    fn respond_201_json_reply_is_not_rejected() {
        let mut api = HttpApiImport::default();
        let headers = vec![("content-type".to_string(), "application/json".to_string())];
        let result = api.respond(UNKNOWN_CALL, 201, headers, br#"{"id":"n-0142"}"#.to_vec());
        assert!(is_not_invalid_argument(result));
    }

    /// Source: api-http-api.md "Limits": empty headers and an empty body are valid input; the status alone decides.
    #[test]
    fn respond_empty_headers_and_empty_body_are_not_rejected() {
        let mut api = HttpApiImport::default();
        let result = api.respond(UNKNOWN_CALL, 404, Vec::new(), Vec::new());
        assert!(is_not_invalid_argument(result));
    }

    /// Source: reference-wit.md 5.24: a 404 reply with an empty body is valid for an open call; unknown call gives not-found.
    #[test]
    fn respond_404_with_empty_body_to_unknown_call_returns_not_found() {
        let mut api = HttpApiImport::default();
        let result = api.respond(UNKNOWN_CALL, 404, Vec::new(), Vec::new());
        assert_eq!(error_code_of(result), Some(ErrorCode::NotFound));
    }

    /// A sink that records every completed call and knows one open call.
    struct RecordingSink {
        owner: &'static str,
        open_call: u64,
        seen: std::sync::Mutex<Vec<(String, u64, super::HttpResponse)>>,
    }

    impl super::HttpResponseSink for RecordingSink {
        fn complete(&self, plugin: &str, call: u64, response: super::HttpResponse) -> bool {
            let known = plugin == self.owner && call == self.open_call;
            if known {
                self.seen
                    .lock()
                    .expect("sink lock")
                    .push((plugin.to_owned(), call, response));
            }
            known
        }
    }

    fn sink(owner: &'static str, open_call: u64) -> std::sync::Arc<RecordingSink> {
        std::sync::Arc::new(RecordingSink {
            owner,
            open_call,
            seen: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// REQ: API-005
    #[test]
    fn routed_respond_hands_the_response_to_the_sink() {
        let sink = sink("example.notes", 7);
        let mut api = super::RoutedHttpApi::new("example.notes", sink.clone());
        let headers = vec![("content-type".to_owned(), "text/plain".to_owned())];
        api.respond(7, 201, headers.clone(), b"made".to_vec())
            .expect("the open call is answered");
        let seen = sink.seen.lock().expect("sink lock");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].1, 7);
        assert_eq!(seen[0].2.status, 201);
        assert_eq!(seen[0].2.headers, headers);
        assert_eq!(seen[0].2.body, b"made".to_vec());
    }

    /// REQ: API-005
    #[test]
    fn routed_respond_to_a_foreign_or_unknown_call_is_not_found() {
        let sink = sink("example.notes", 7);
        let mut other = super::RoutedHttpApi::new("example.other", sink.clone());
        let mut owner = super::RoutedHttpApi::new("example.notes", sink.clone());
        assert_eq!(
            error_code_of(other.respond(7, 200, Vec::new(), Vec::new())),
            Some(ErrorCode::NotFound),
            "another plugin cannot answer the call"
        );
        assert_eq!(
            error_code_of(owner.respond(8, 200, Vec::new(), Vec::new())),
            Some(ErrorCode::NotFound)
        );
        assert!(sink.seen.lock().expect("sink lock").is_empty());
    }

    /// REQ: API-005
    #[test]
    fn routed_respond_validates_before_it_reaches_the_sink() {
        let sink = sink("example.notes", 7);
        let mut api = super::RoutedHttpApi::new("example.notes", sink.clone());
        assert_eq!(
            error_code_of(api.respond(7, 99, Vec::new(), Vec::new())),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            error_code_of(api.respond(7, 200, single_header("Transfer-Encoding"), Vec::new())),
            Some(ErrorCode::InvalidArgument)
        );
        assert!(sink.seen.lock().expect("sink lock").is_empty());
    }
}
