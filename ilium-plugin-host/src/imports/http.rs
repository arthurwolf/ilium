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
    fn fetch(&mut self, _request: HttpRequest) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::http::fetch")
    }

    fn take_response(&mut self, _request: RequestId) -> Option<HttpResponse> {
        unimplemented!("host::imports::http::take_response")
    }

    fn open_stream(&mut self, _request: StreamRequest) -> Result<StreamId, PluginError> {
        unimplemented!("host::imports::http::open_stream")
    }

    fn close_stream(&mut self, _stream: StreamId) {
        unimplemented!("host::imports::http::close_stream")
    }

    fn open_websocket(
        &mut self,
        _url: String,
        _protocols: Vec<String>,
    ) -> Result<StreamId, PluginError> {
        unimplemented!("host::imports::http::open_websocket")
    }

    fn send_websocket(&mut self, _stream: StreamId, _text: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::http::send_websocket")
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    use crate::testing::imports_fixture::ImportsFixture;
    use ilium_plugin_api::generated::host::{HttpRequest, StreamFraming, StreamRequest};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::{Header, Outcome, RequestId};
    use ilium_plugin_api::Tier;

    /// Default `timeout_millis` for requests that are not testing the timeout.
    const DEFAULT_TIMEOUT_MILLIS: u32 = 5_000;

    /// Largest and smallest accepted `timeout_millis` (api-http, Limits).
    const MAX_TIMEOUT_MILLIS: u32 = 300_000;

    /// Largest URL accepted by `fetch`, in bytes (api-http, Limits).
    const MAX_URL_BYTES: usize = 8 * 1024;

    /// Largest number of headers accepted by `fetch` (api-http, Limits).
    const MAX_HEADERS: usize = 64;

    /// Largest request body accepted by `fetch`, in bytes (SEC-082, API-062).
    const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

    /// Pending requests a plugin may hold before `busy` (ABI-013).
    const PENDING_REQUEST_LIMIT: usize = 64;

    /// Open streams a plugin may hold before `quota-exceeded` (api-http-streams, Limits).
    const STREAMS_PER_PLUGIN_LIMIT: usize = 4;

    /// Largest `max_chunk_bytes` accepted by `open_stream` (api-http-streams, WIT excerpt).
    const MAX_CHUNK_BYTES: u32 = 64 * 1024;

    /// How long a test waits for a completion event before failing.
    const COMPLETION_WAIT: Duration = Duration::from_secs(5);

    /// A minimal well-formed HTTP/1.1 response served by the loopback test server.
    const OK_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

    /// Binds a loopback listener on a free port. The caller keeps it alive while requests run.
    fn bind_loopback() -> (TcpListener, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
        let port = listener.local_addr().expect("listener address").port();
        (listener, port)
    }

    /// A T2 fixture granting the loopback port, plus any extra host grants.
    fn fixture_granting(port: u16, extra_grants: &[&str]) -> ImportsFixture {
        let loopback_grant = format!("http:127.0.0.1:{port}");
        let mut grants: Vec<&str> = extra_grants.to_vec();
        grants.push(loopback_grant.as_str());
        ImportsFixture::builder()
            .tier(Tier::T2)
            .grants(&grants)
            .build()
    }

    /// A `GET` request with the default timeout and no headers or body.
    fn get(url: String) -> HttpRequest {
        HttpRequest {
            method: "GET".to_owned(),
            url,
            headers: Vec::new(),
            body: None,
            timeout_millis: DEFAULT_TIMEOUT_MILLIS,
        }
    }

    /// A plain-HTTP URL on the loopback test server.
    fn loopback_url(port: u16, path: &str) -> String {
        format!("http://127.0.0.1:{port}{path}")
    }

    /// Unwraps a successful result, failing with the error code otherwise.
    fn expect_ok<T: std::fmt::Debug>(result: Result<T, PluginError>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected success, got {:?} ({})", error.code, error.message),
        }
    }

    /// Asserts that the call failed with the expected error code.
    fn expect_code<T: std::fmt::Debug>(result: Result<T, PluginError>, expected: ErrorCode) {
        match result {
            Err(error) => assert_eq!(error.code, expected, "message: {}", error.message),
            Ok(value) => panic!("expected {expected:?}, got Ok({value:?})"),
        }
    }

    /// Accepts one connection, reads the request head and answers with `response`.
    fn serve_once(listener: TcpListener, response: &'static [u8]) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(response);
            }
        })
    }

    /// A URL of exactly `length` bytes on the loopback server, padded with `a` in the query.
    fn url_of_length(port: u16, length: usize) -> String {
        let prefix = loopback_url(port, "/?q=");
        let padding = length - prefix.len();
        format!("{prefix}{}", "a".repeat(padding))
    }

    /// REQ: ABI-013
    #[test]
    fn fetch_granted_loopback_get_returns_request_id() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let id = expect_ok(fixture.http().fetch(get(loopback_url(port, "/"))));
        assert!(id.0 > 0, "request id should be a fresh non-zero id, got {id:?}");
    }

    /// REQ: API-062
    #[test]
    fn fetch_empty_body_and_no_headers_are_valid() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.body = Some(Vec::new());
        request.headers = Vec::new();
        let _ = expect_ok(fixture.http().fetch(request));
    }

    /// REQ: API-062
    #[test]
    fn fetch_body_at_exactly_8_mib_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.method = "POST".to_owned();
        request.body = Some(vec![0u8; MAX_BODY_BYTES]);
        let _ = expect_ok(fixture.http().fetch(request));
    }

    /// REQ: API-062
    #[test]
    fn fetch_body_one_byte_over_8_mib_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.method = "POST".to_owned();
        request.body = Some(vec![0u8; MAX_BODY_BYTES + 1]);
        expect_code(fixture.http().fetch(request), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_url_of_exactly_8_kib_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let url = url_of_length(port, MAX_URL_BYTES);
        assert_eq!(url.len(), MAX_URL_BYTES);
        let _ = expect_ok(fixture.http().fetch(get(url)));
    }

    /// REQ: API-062
    #[test]
    fn fetch_url_one_byte_over_8_kib_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let url = url_of_length(port, MAX_URL_BYTES + 1);
        assert_eq!(url.len(), MAX_URL_BYTES + 1);
        expect_code(fixture.http().fetch(get(url)), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_empty_url_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        expect_code(fixture.http().fetch(get(String::new())), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_unparsable_url_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &["http:api.example.com"]);
        expect_code(
            fixture.http().fetch(get("not a url".to_owned())),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: SEC-077
    #[test]
    fn fetch_plain_http_to_public_host_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &["http:api.example.com"]);
        expect_code(
            fixture.http().fetch(get("http://api.example.com/v1/status".to_owned())),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: SEC-079
    #[test]
    fn fetch_forbidden_host_header_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.headers = vec![Header {
            name: "Host".to_owned(),
            value: "api.example.com".to_owned(),
        }];
        expect_code(fixture.http().fetch(request), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_exactly_64_headers_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.headers = (0..MAX_HEADERS)
            .map(|index| Header {
                name: format!("x-test-{index}"),
                value: "v".to_owned(),
            })
            .collect();
        let _ = expect_ok(fixture.http().fetch(request));
    }

    /// REQ: API-062
    #[test]
    fn fetch_65_headers_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.headers = (0..=MAX_HEADERS)
            .map(|index| Header {
                name: format!("x-test-{index}"),
                value: "v".to_owned(),
            })
            .collect();
        expect_code(fixture.http().fetch(request), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_timeout_of_1_ms_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.timeout_millis = 1;
        let _ = expect_ok(fixture.http().fetch(request));
    }

    /// REQ: API-062
    #[test]
    fn fetch_timeout_of_300_000_ms_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.timeout_millis = MAX_TIMEOUT_MILLIS;
        let _ = expect_ok(fixture.http().fetch(request));
    }

    /// REQ: API-062
    #[test]
    fn fetch_timeout_of_0_ms_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.timeout_millis = 0;
        expect_code(fixture.http().fetch(request), ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn fetch_timeout_over_300_000_ms_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut request = get(loopback_url(port, "/"));
        request.timeout_millis = MAX_TIMEOUT_MILLIS + 1;
        expect_code(fixture.http().fetch(request), ErrorCode::InvalidArgument);
    }

    /// REQ: SEC-013
    #[test]
    fn fetch_host_without_grant_is_not_permitted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        expect_code(
            fixture.http().fetch(get("https://api.example.com/v1/status".to_owned())),
            ErrorCode::NotPermitted,
        );
    }

    /// REQ: ABI-013
    #[test]
    fn fetch_65th_pending_request_is_busy() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        for _ in 0..PENDING_REQUEST_LIMIT {
            let mut request = get(loopback_url(port, "/"));
            request.timeout_millis = MAX_TIMEOUT_MILLIS;
            let _ = expect_ok(fixture.http().fetch(request));
        }
        let mut request = get(loopback_url(port, "/"));
        request.timeout_millis = MAX_TIMEOUT_MILLIS;
        expect_code(fixture.http().fetch(request), ErrorCode::Busy);
    }

    /// REQ: ABI-041
    #[test]
    fn take_response_for_unknown_request_id_returns_none() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        assert!(fixture.http().take_response(RequestId(u64::MAX)).is_none());
    }

    /// REQ: ABI-041
    #[test]
    fn take_response_returns_completed_response_once() {
        let (listener, port) = bind_loopback();
        let _responder = serve_once(listener, OK_RESPONSE);
        let mut fixture = fixture_granting(port, &[]);
        let id = expect_ok(fixture.http().fetch(get(loopback_url(port, "/"))));
        let outcome = fixture.wait_outcome(id, COMPLETION_WAIT);
        assert!(
            matches!(outcome, Some(Outcome::Ok)),
            "expected the request to complete ok, got {outcome:?}"
        );
        let response = fixture
            .http()
            .take_response(id)
            .expect("completed response should be held for one take");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok".to_vec());
        assert!(
            fixture.http().take_response(id).is_none(),
            "a second take of the same request must return none"
        );
    }

    /// REQ: ABI-041
    #[test]
    fn take_response_keeps_http_error_status_as_a_normal_response() {
        let (listener, port) = bind_loopback();
        let _responder = serve_once(
            listener,
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nConnection: close\r\n\r\nno",
        );
        let mut fixture = fixture_granting(port, &[]);
        let id = expect_ok(fixture.http().fetch(get(loopback_url(port, "/missing"))));
        let outcome = fixture.wait_outcome(id, COMPLETION_WAIT);
        assert!(
            matches!(outcome, Some(Outcome::Ok)),
            "an HTTP 404 is not a transport failure, got {outcome:?}"
        );
        let response = fixture
            .http()
            .take_response(id)
            .expect("404 response should still be taken");
        assert_eq!(response.status, 404);
    }

    /// REQ: API-070
    #[test]
    fn open_stream_granted_raw_returns_stream_id() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let stream = StreamRequest {
            request: get(loopback_url(port, "/events")),
            framing: StreamFraming::Raw,
            max_chunk_bytes: 1024,
        };
        let _ = expect_ok(fixture.http().open_stream(stream));
    }

    /// REQ: API-070
    #[test]
    fn open_stream_max_chunk_of_64_kib_is_accepted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let stream = StreamRequest {
            request: get(loopback_url(port, "/events")),
            framing: StreamFraming::Lines,
            max_chunk_bytes: MAX_CHUNK_BYTES,
        };
        let _ = expect_ok(fixture.http().open_stream(stream));
    }

    /// REQ: API-070
    #[test]
    fn open_stream_max_chunk_over_64_kib_is_invalid_argument() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let stream = StreamRequest {
            request: get(loopback_url(port, "/events")),
            framing: StreamFraming::Lines,
            max_chunk_bytes: MAX_CHUNK_BYTES + 1,
        };
        expect_code(fixture.http().open_stream(stream), ErrorCode::InvalidArgument);
    }

    /// REQ: SEC-013
    #[test]
    fn open_stream_host_without_grant_is_not_permitted() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let stream = StreamRequest {
            request: get("https://feed.example.com/v1/events".to_owned()),
            framing: StreamFraming::ServerSentEvents,
            max_chunk_bytes: 1024,
        };
        expect_code(fixture.http().open_stream(stream), ErrorCode::NotPermitted);
    }

    /// REQ: API-070
    #[test]
    fn open_stream_fifth_stream_is_quota_exceeded() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        for _ in 0..STREAMS_PER_PLUGIN_LIMIT {
            let stream = StreamRequest {
                request: get(loopback_url(port, "/events")),
                framing: StreamFraming::Raw,
                max_chunk_bytes: 1024,
            };
            let _ = expect_ok(fixture.http().open_stream(stream));
        }
        let stream = StreamRequest {
            request: get(loopback_url(port, "/events")),
            framing: StreamFraming::Raw,
            max_chunk_bytes: 1024,
        };
        expect_code(fixture.http().open_stream(stream), ErrorCode::QuotaExceeded);
    }

    /// REQ: API-070
    #[test]
    fn close_stream_frees_a_slot_for_a_new_stream() {
        let (_listener, port) = bind_loopback();
        let mut fixture = fixture_granting(port, &[]);
        let mut first_stream = None;
        for _ in 0..STREAMS_PER_PLUGIN_LIMIT {
            let stream = StreamRequest {
                request: get(loopback_url(port, "/events")),
                framing: StreamFraming::Raw,
                max_chunk_bytes: 1024,
            };
            let id = expect_ok(fixture.http().open_stream(stream));
            if first_stream.is_none() {
                first_stream = Some(id);
            }
        }
        let first_stream = first_stream.expect("the first stream id");
        fixture.http().close_stream(first_stream);
        let stream = StreamRequest {
            request: get(loopback_url(port, "/events")),
            framing: StreamFraming::Raw,
            max_chunk_bytes: 1024,
        };
        let _ = expect_ok(fixture.http().open_stream(stream));
    }
}
