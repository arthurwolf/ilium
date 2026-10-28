//! Host implementation of `ilium:host/http` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostHttp` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::http::{HostHttp, HttpRequest, HttpResponse, StreamRequest};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{RequestId, StreamId};

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::Header;
use ilium_plugin_api::generated::types::errors::ErrorCode;

/// Longest URL, in bytes.
const MAX_URL_BYTES: usize = 8 * 1024;
/// Most request headers.
const MAX_HEADERS: usize = 64;
/// Longest request body, in bytes.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
/// Accepted `timeout_millis` range.
const MIN_TIMEOUT_MILLIS: u32 = 1;
const MAX_TIMEOUT_MILLIS: u32 = 300_000;
/// Pending requests one plugin may hold before `busy`.
const PENDING_REQUEST_LIMIT: usize = 64;
/// Open streams one plugin may hold before `quota-exceeded`.
const STREAMS_PER_PLUGIN_LIMIT: usize = 4;
/// Largest `max_chunk_bytes` of a stream.
const MAX_CHUNK_BYTES: u32 = 64 * 1024;
/// Largest response the host reads into memory, in bytes.
const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
/// How long `take_response` waits for an in-flight request before reporting nothing.
const TAKE_RESPONSE_PATIENCE: Duration = Duration::from_secs(2);
/// Request headers a plugin may not set (SEC-079).
const FORBIDDEN_HEADERS: [&str; 5] = ["host", "content-length", "connection", "transfer-encoding", "upgrade"];

/// Completion slot shared between a request thread and `take_response`.
type ResponseSlot = Arc<(Mutex<Option<Result<HttpResponse, PluginError>>>, Condvar)>;

#[derive(Default)]
struct HttpState {
    next_id: u64,
    requests: HashMap<u64, ResponseSlot>,
    streams: HashSet<u64>,
}

thread_local! {
    /// Per-thread because `HttpImport` is a stateless handle.
    static HTTP_STATE: RefCell<HttpState> = RefCell::new(HttpState::default());
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// A parsed `scheme://host[:port]/path` URL.
struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path_and_query: String,
}

impl ParsedUrl {
    /// True for the loopback hosts that may use plain `http` (SEC-077).
    fn is_loopback(&self) -> bool {
        matches!(self.host.as_str(), "127.0.0.1" | "localhost" | "[::1]" | "::1")
    }
}

/// Parses `url`, refusing anything that is not `scheme://host...` with a valid port.
fn parse_url(url: &str, allowed_schemes: &[&str]) -> Result<ParsedUrl, PluginError> {
    let bad = |message: &str| error(ErrorCode::InvalidArgument, message);
    if url.is_empty() || url.len() > MAX_URL_BYTES {
        return Err(bad("url must be 1 to 8192 bytes"));
    }
    if url.chars().any(|character| character.is_whitespace() || character.is_control()) {
        return Err(bad("url holds whitespace or control characters"));
    }
    let (scheme, rest) = url.split_once("://").ok_or_else(|| bad("url has no scheme"))?;
    let scheme = scheme.to_ascii_lowercase();
    if !allowed_schemes.contains(&scheme.as_str()) {
        return Err(bad("url scheme is not allowed here"));
    }
    let (authority, path_and_query) = match rest.find(['/', '?']) {
        Some(index) if rest.as_bytes()[index] == b'/' => (&rest[..index], rest[index..].to_owned()),
        Some(index) => (&rest[..index], format!("/{}", &rest[index..])),
        None => (rest, "/".to_owned()),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(bad("url host is missing or carries credentials"));
    }
    let default_port = if scheme == "https" || scheme == "wss" { 443 } else { 80 };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => {
            let port = port.parse::<u16>().map_err(|_| bad("url port is invalid"))?;
            (host, port)
        }
        _ => (authority, default_port),
    };
    if host.is_empty() {
        return Err(bad("url host is empty"));
    }
    Ok(ParsedUrl {
        scheme,
        host: host.to_ascii_lowercase(),
        port,
        path_and_query,
    })
}

/// Checks the request shape and destination; returns the parsed URL.
///
/// Grants are not visible to this stateless handle, so only loopback destinations are
/// reachable; every other destination is `not-permitted` until the kernel injects grants.
fn check_request(request: &HttpRequest) -> Result<ParsedUrl, PluginError> {
    let parsed = parse_url(&request.url, &["http", "https"])?;
    if request.headers.len() > MAX_HEADERS {
        return Err(error(ErrorCode::InvalidArgument, "at most 64 headers"));
    }
    let forbidden = request
        .headers
        .iter()
        .any(|header: &Header| FORBIDDEN_HEADERS.contains(&header.name.to_ascii_lowercase().as_str()));
    if forbidden {
        return Err(error(ErrorCode::InvalidArgument, "header name is not settable by a plugin"));
    }
    if request.body.as_ref().is_some_and(|body| body.len() > MAX_BODY_BYTES) {
        return Err(error(ErrorCode::InvalidArgument, "request body exceeds 8 MiB"));
    }
    if !(MIN_TIMEOUT_MILLIS..=MAX_TIMEOUT_MILLIS).contains(&request.timeout_millis) {
        return Err(error(ErrorCode::InvalidArgument, "timeout must be 1 to 300000 ms"));
    }
    if request.method.is_empty() || !request.method.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(error(ErrorCode::InvalidArgument, "method must be an HTTP token"));
    }
    if parsed.scheme == "http" && !parsed.is_loopback() {
        return Err(error(ErrorCode::InvalidArgument, "plain http is only allowed to loopback"));
    }
    if !parsed.is_loopback() {
        return Err(error(ErrorCode::NotPermitted, "no http grant covers this host"));
    }
    Ok(parsed)
}

/// Performs one plain-HTTP/1.1 exchange with `Connection: close`.
fn perform(parsed: &ParsedUrl, request: &HttpRequest) -> Result<HttpResponse, PluginError> {
    let timeout = Duration::from_millis(u64::from(request.timeout_millis));
    let deadline = Instant::now() + timeout;
    if parsed.scheme != "http" {
        return Err(error(ErrorCode::Unsupported, "https transport is provided by the kernel"));
    }
    let address = (parsed.host.trim_matches(['[', ']']), parsed.port)
        .to_socket_addrs()
        .map_err(|_| error(ErrorCode::Network, "host did not resolve"))?
        .next()
        .ok_or_else(|| error(ErrorCode::Network, "host did not resolve"))?;
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|_| error(ErrorCode::Network, "connection failed"))?;
    let remaining = |deadline: Instant| deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
    let io_error = |failure: std::io::Error| match failure.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => error(ErrorCode::TimedOut, "request timed out"),
        _ => error(ErrorCode::Network, "connection failed"),
    };
    stream.set_write_timeout(Some(remaining(deadline))).map_err(io_error)?;
    let body = request.body.clone().unwrap_or_default();
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nContent-Length: {}\r\n",
        request.method.to_ascii_uppercase(),
        parsed.path_and_query,
        parsed.host,
        parsed.port,
        body.len()
    );
    for header in &request.headers {
        head.push_str(&format!("{}: {}\r\n", header.name, header.value));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).map_err(io_error)?;
    stream.write_all(&body).map_err(io_error)?;
    stream.set_read_timeout(Some(remaining(deadline))).map_err(io_error)?;
    let mut raw = Vec::new();
    stream
        .take(MAX_RESPONSE_BYTES)
        .read_to_end(&mut raw)
        .map_err(io_error)?;
    parse_response(&raw)
}

/// Parses a raw HTTP/1.1 response (identity or `Content-Length` bodies).
fn parse_response(raw: &[u8]) -> Result<HttpResponse, PluginError> {
    let malformed = || error(ErrorCode::Network, "malformed http response");
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n").ok_or_else(malformed)?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| malformed())?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(malformed)?;
    let headers: Vec<Header> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| Header {
            name: name.trim().to_owned(),
            value: value.trim().to_owned(),
        })
        .collect();
    let mut body = raw[split + 4..].to_vec();
    let declared = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-length"))
        .and_then(|header| header.value.parse::<usize>().ok());
    if let Some(length) = declared {
        body.truncate(length);
    }
    Ok(HttpResponse { status, headers, body })
}

/// Host side of the `http` interface.
#[derive(Debug)]
pub struct HttpImport;

impl HostHttp for HttpImport {
    fn fetch(&mut self, request: HttpRequest) -> Result<RequestId, PluginError> {
        let parsed = check_request(&request)?;
        let slot: ResponseSlot = Arc::new((Mutex::new(None), Condvar::new()));
        let id = HTTP_STATE.with(|state| {
            let mut state = state.borrow_mut();
            if state.requests.len() >= PENDING_REQUEST_LIMIT {
                return Err(error(ErrorCode::Busy, "64 http requests are already pending"));
            }
            state.next_id += 1;
            let id = state.next_id;
            state.requests.insert(id, slot.clone());
            Ok(id)
        })?;
        let worker_slot = slot;
        // The worker owns only its slot; it ends by itself when the exchange finishes or times out.
        thread::spawn(move || {
            let outcome = perform(&parsed, &request);
            let (lock, ready) = &*worker_slot;
            if let Ok(mut held) = lock.lock() {
                *held = Some(outcome);
                ready.notify_all();
            }
        });
        Ok(RequestId(id))
    }

    /// A request that is still in flight is waited for up to two seconds, then reported as not ready.
    fn take_response(&mut self, request: RequestId) -> Option<HttpResponse> {
        let slot = HTTP_STATE.with(|state| state.borrow().requests.get(&request.0).cloned())?;
        let (lock, ready) = &*slot;
        let guard = lock.lock().ok()?;
        let (mut guard, _) = ready
            .wait_timeout_while(guard, TAKE_RESPONSE_PATIENCE, |held| held.is_none())
            .ok()?;
        let outcome = guard.take()?;
        HTTP_STATE.with(|state| state.borrow_mut().requests.remove(&request.0));
        outcome.ok()
    }

    fn open_stream(&mut self, request: StreamRequest) -> Result<StreamId, PluginError> {
        if request.max_chunk_bytes == 0 || request.max_chunk_bytes > MAX_CHUNK_BYTES {
            return Err(error(ErrorCode::InvalidArgument, "max chunk must be 1 to 65536 bytes"));
        }
        check_request(&request.request)?;
        HTTP_STATE.with(|state| {
            let mut state = state.borrow_mut();
            if state.streams.len() >= STREAMS_PER_PLUGIN_LIMIT {
                return Err(error(ErrorCode::QuotaExceeded, "4 streams are already open"));
            }
            state.next_id += 1;
            let id = state.next_id;
            state.streams.insert(id);
            Ok(StreamId(id))
        })
    }

    fn close_stream(&mut self, stream: StreamId) {
        HTTP_STATE.with(|state| {
            state.borrow_mut().streams.remove(&stream.0);
        });
    }

    /// The `websocket` feature is unstable and off unless the kernel enables it.
    fn open_websocket(
        &mut self,
        url: String,
        _protocols: Vec<String>,
    ) -> Result<StreamId, PluginError> {
        parse_url(&url, &["ws", "wss"])?;
        Err(error(ErrorCode::Unsupported, "feature websocket is not enabled"))
    }

    fn send_websocket(&mut self, stream: StreamId, _text: String) -> Result<(), PluginError> {
        let is_open = HTTP_STATE.with(|state| state.borrow().streams.contains(&stream.0));
        if is_open {
            Err(error(ErrorCode::Unsupported, "stream is not a websocket"))
        } else {
            Err(error(ErrorCode::NotFound, "no open stream has this id"))
        }
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
    const OK_RESPONSE: &[u8] =
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

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
        assert!(
            id.0 > 0,
            "request id should be a fresh non-zero id, got {id:?}"
        );
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
        expect_code(
            fixture.http().fetch(get(String::new())),
            ErrorCode::InvalidArgument,
        );
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
            fixture
                .http()
                .fetch(get("http://api.example.com/v1/status".to_owned())),
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
            fixture
                .http()
                .fetch(get("https://api.example.com/v1/status".to_owned())),
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
        expect_code(
            fixture.http().open_stream(stream),
            ErrorCode::InvalidArgument,
        );
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
