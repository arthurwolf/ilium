//! api-http.md: `fetch` and `take-response`, validation limits, grants, redirects, body cap.

use ilium_plugin_api::generated::host::HttpRequest;
use ilium_plugin_api::generated::types::{ErrorCode, Header, RequestId};
use ilium_plugin_api::Tier;
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;
use proptest::prelude::*;

use crate::support::*;

fn request(method: &str, url: &str) -> HttpRequest {
    HttpRequest {
        method: method.to_owned(),
        url: url.to_owned(),
        headers: Vec::new(),
        body: None,
        timeout_millis: 5000,
    }
}

fn granted(port: u16) -> ImportsFixture {
    let grant = format!("http:127.0.0.1:{port}");
    fixture(Tier::T2, &[grant.as_str()])
}

fn served(response: Vec<u8>) -> (u16, std::sync::mpsc::Receiver<String>, ImportsFixture) {
    let (port, heads) = loopback_server(response, 1);
    (port, heads, granted(port))
}

fn url(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

fn fetch_ok(
    fixture: &mut ImportsFixture,
    req: HttpRequest,
) -> ilium_plugin_api::generated::host::HttpResponse {
    let id = expect_ok(fixture.http().fetch(req));
    assert!(is_ok(&complete(fixture, id)));
    fixture.http().take_response(id).expect("response")
}

// ---- fetch: success --------------------------------------------------------------------------

/// REQ: WIT:ilium:host/http.fetch, WIT:ilium:host/http.take-response
#[test]
fn http_fetch_get_returns_status_headers_and_body() {
    let (port, _heads, mut fixture) =
        served(http_response("200 OK", &[("X-Test", "yes")], b"hello"));
    let response = fetch_ok(&mut fixture, request("GET", &url(port, "/")));
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"hello");
    assert!(response
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("x-test") && h.value == "yes"));
}

/// REQ: WIT:ilium:host/http.take-response
#[test]
fn http_take_response_returns_the_response_once() {
    let (port, _heads, mut fixture) = served(http_response("200 OK", &[], b"x"));
    let id = expect_ok(fixture.http().fetch(request("GET", &url(port, "/"))));
    complete(&mut fixture, id);
    assert!(fixture.http().take_response(id).is_some());
    assert!(fixture.http().take_response(id).is_none());
}

/// REQ: WIT:ilium:host/http.take-response
#[test]
fn http_take_response_for_an_unknown_request_is_none() {
    let mut fixture = fixture(Tier::T2, &[]);
    assert!(fixture.http().take_response(RequestId(55_555)).is_none());
}

/// REQ: WIT:ilium:host/http.take-response
#[test]
fn http_take_response_is_dropped_after_30_seconds_unclaimed() {
    // two connections: one response is claimed at 29 s, the other is left until 31 s
    let (port, _heads) = loopback_server(http_response("200 OK", &[], b"x"), 2);
    let mut fixture = granted(port);
    let early = expect_ok(fixture.http().fetch(request("GET", &url(port, "/"))));
    complete(&mut fixture, early);
    let late = expect_ok(fixture.http().fetch(request("GET", &url(port, "/"))));
    complete(&mut fixture, late);
    fixture.advance(std::time::Duration::from_secs(29));
    assert!(
        fixture.http().take_response(early).is_some(),
        "still available before 30 s"
    );
    fixture.advance(std::time::Duration::from_secs(2));
    assert!(fixture.http().take_response(late).is_none());
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_non_2xx_status_is_a_response_not_an_error() {
    let (port, _heads, mut fixture) = served(http_response("404 Not Found", &[], b"nope"));
    assert_eq!(
        fetch_ok(&mut fixture, request("GET", &url(port, "/missing"))).status,
        404
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_post_sends_the_body_and_headers() {
    let (port, heads, mut fixture) = served(http_response("200 OK", &[], b""));
    let mut req = request("POST", &url(port, "/submit"));
    req.body = Some(b"payload".to_vec());
    req.headers.push(Header {
        name: "X-Custom".to_owned(),
        value: "abc".to_owned(),
    });
    fetch_ok(&mut fixture, req);
    let head = heads.recv_timeout(WAIT).expect("request head");
    assert!(head.starts_with("POST /submit "), "{head}");
    assert!(head.to_ascii_lowercase().contains("x-custom: abc"));
    assert!(head.contains("payload"));
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_every_listed_method_is_accepted() {
    for method in ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        let (port, _heads, mut fixture) = served(http_response("200 OK", &[], b""));
        let id = fixture.http().fetch(request(method, &url(port, "/")));
        assert!(id.is_ok(), "{method}: {id:?}");
    }
}

// ---- fetch: validation -----------------------------------------------------------------------

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_lowercase_or_unknown_method_invalid_argument() {
    let (port, _heads, mut fixture) = served(http_response("200 OK", &[], b""));
    for method in ["get", "Get", "TRACE", "CONNECT", ""] {
        assert_eq!(
            expect_code(fixture.http().fetch(request(method, &url(port, "/")))),
            ErrorCode::InvalidArgument,
            "{method:?}"
        );
    }
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_url_of_exactly_8_kib_is_not_refused_for_its_length() {
    let (port, _heads, mut fixture) = served(http_response("200 OK", &[], b""));
    let base = url(port, "/?q=");
    let long = format!("{base}{}", "a".repeat(8 * KIB - base.len()));
    assert_eq!(long.len(), 8 * KIB);
    let result = fixture.http().fetch(request("GET", &long));
    assert!(result.is_ok(), "{result:?}");
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_url_of_8_kib_plus_one_invalid_argument() {
    let (port, _heads, mut fixture) = served(http_response("200 OK", &[], b""));
    let base = url(port, "/?q=");
    let long = format!("{base}{}", "a".repeat(8 * KIB + 1 - base.len()));
    assert_eq!(
        expect_code(fixture.http().fetch(request("GET", &long))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_64_headers_accepted_and_65_invalid_argument() {
    let (port, _heads, mut fixture) = served(http_response("200 OK", &[], b""));
    let headers = |count: usize| -> Vec<Header> {
        (0..count)
            .map(|index| Header {
                name: format!("X-H{index}"),
                value: "v".to_owned(),
            })
            .collect()
    };
    let mut ok = request("GET", &url(port, "/"));
    ok.headers = headers(64);
    assert!(fixture.http().fetch(ok).is_ok());
    let mut over = request("GET", &url(port, "/"));
    over.headers = headers(65);
    assert_eq!(
        expect_code(fixture.http().fetch(over)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_body_of_8_mib_accepted_and_one_byte_more_invalid_argument() {
    let (port, _heads, mut fixture) = served(http_response("200 OK", &[], b""));
    let mut ok = request("POST", &url(port, "/"));
    ok.body = Some(vec![0u8; 8 * MIB]);
    assert!(fixture.http().fetch(ok).is_ok());
    let mut over = request("POST", &url(port, "/"));
    over.body = Some(vec![0u8; 8 * MIB + 1]);
    assert_eq!(
        expect_code(fixture.http().fetch(over)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_timeout_millis_boundaries_0_1_300000_300001() {
    let (port, _heads, mut fixture) = served(http_response("200 OK", &[], b""));
    for (millis, accepted) in [(0u32, false), (1, true), (300_000, true), (300_001, false)] {
        let mut req = request("GET", &url(port, "/"));
        req.timeout_millis = millis;
        let result = fixture.http().fetch(req);
        assert_eq!(result.is_ok(), accepted, "timeout {millis}: {result:?}");
        if !accepted {
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidArgument);
        }
    }
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_malformed_url_invalid_argument() {
    let mut fixture = fixture(Tier::T2, &["http:127.0.0.1"]);
    for bad in ["", "not a url", "ftp://127.0.0.1/", "http://"] {
        assert_eq!(
            expect_code(fixture.http().fetch(request("GET", bad))),
            ErrorCode::InvalidArgument,
            "{bad:?}"
        );
    }
}

// ---- fetch: grants ---------------------------------------------------------------------------

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_without_any_http_grant_not_permitted() {
    let mut fixture = fixture(Tier::T2, &[]);
    assert_eq!(
        expect_code(fixture.http().fetch(request("GET", "https://example.com/"))),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_host_not_in_the_grants_not_permitted() {
    let mut fixture = fixture(Tier::T2, &["http:example.com"]);
    assert_eq!(
        expect_code(
            fixture
                .http()
                .fetch(request("GET", "https://other.example.org/"))
        ),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_grant_without_a_port_does_not_cover_a_loopback_test_port() {
    let (port, _heads) = loopback_server(http_response("200 OK", &[], b""), 1);
    let mut fixture = fixture(Tier::T2, &["http:127.0.0.1"]);
    assert_eq!(
        expect_code(fixture.http().fetch(request("GET", &url(port, "/")))),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_plain_http_to_a_non_loopback_host_not_permitted() {
    let mut fixture = fixture(Tier::T2, &["http:example.com"]);
    assert_eq!(
        expect_code(fixture.http().fetch(request("GET", "http://example.com/"))),
        ErrorCode::NotPermitted
    );
}

// ---- fetch: redirects, body cap, pending cap -------------------------------------------------

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_redirect_to_an_ungranted_host_fails_not_permitted() {
    let (port, _heads) = loopback_server(
        http_response(
            "302 Found",
            &[("Location", "http://ungranted.invalid/x")],
            b"",
        ),
        1,
    );
    let mut fixture = granted(port);
    let id = expect_ok(fixture.http().fetch(request("GET", &url(port, "/"))));
    assert_eq!(
        outcome_code(&complete(&mut fixture, id)),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_redirect_to_a_granted_host_is_followed() {
    let (port, _heads) = loopback_server(http_response("200 OK", &[], b"final"), 2);
    let mut second_hop = http_response("302 Found", &[("Location", &url(port, "/next"))], b"");
    second_hop.shrink_to_fit();
    let (redirect_port, _redirect_heads) = loopback_server(second_hop, 1);
    let grants = [
        format!("http:127.0.0.1:{redirect_port}"),
        format!("http:127.0.0.1:{port}"),
    ];
    let mut fixture = fixture(Tier::T2, &[grants[0].as_str(), grants[1].as_str()]);
    let response = fetch_ok(&mut fixture, request("GET", &url(redirect_port, "/")));
    assert_eq!(response.body, b"final");
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_sixth_redirect_fails_network() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let reply = http_response("302 Found", &[("Location", &url(port, "/again"))], b"");
        for _ in 0..8 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(&reply);
        }
    });
    let mut fixture = granted(port);
    let id = expect_ok(fixture.http().fetch(request("GET", &url(port, "/"))));
    assert_eq!(
        outcome_code(&complete(&mut fixture, id)),
        Some(ErrorCode::Network)
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_body_over_8_mib_fails_quota_exceeded() {
    let (port, _heads, mut fixture) =
        served(http_response("200 OK", &[], &vec![b'z'; 8 * MIB + 1]));
    let id = expect_ok(fixture.http().fetch(request("GET", &url(port, "/"))));
    assert_eq!(
        outcome_code(&complete(&mut fixture, id)),
        Some(ErrorCode::QuotaExceeded)
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_body_of_exactly_8_mib_is_delivered() {
    let (port, _heads, mut fixture) = served(http_response("200 OK", &[], &vec![b'z'; 8 * MIB]));
    assert_eq!(
        fetch_ok(&mut fixture, request("GET", &url(port, "/")))
            .body
            .len(),
        8 * MIB
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_unanswered_request_fails_timed_out_after_its_timeout() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut fixture = granted(port);
    let mut req = request("GET", &url(port, "/"));
    req.timeout_millis = 300;
    let id = expect_ok(fixture.http().fetch(req));
    fixture.advance(std::time::Duration::from_millis(400));
    assert_eq!(
        outcome_code(&complete(&mut fixture, id)),
        Some(ErrorCode::TimedOut)
    );
    drop(listener);
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_connection_refused_fails_network() {
    let port = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    let mut fixture = granted(port);
    let id = expect_ok(fixture.http().fetch(request("GET", &url(port, "/"))));
    assert_eq!(
        outcome_code(&complete(&mut fixture, id)),
        Some(ErrorCode::Network)
    );
}

/// REQ: WIT:ilium:host/http.fetch
#[test]
fn http_fetch_257th_pending_request_busy() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut fixture = granted(port);
    fixture.hold_async_completions(true);
    for _ in 0..256 {
        expect_ok(fixture.http().fetch(request("GET", &url(port, "/"))));
    }
    assert_eq!(
        expect_code(fixture.http().fetch(request("GET", &url(port, "/")))),
        ErrorCode::Busy
    );
    drop(listener);
}

proptest! {
    // REQ: WIT:ilium:host/http.fetch
    #[test]
    fn http_fetch_only_the_seven_upper_case_methods_pass_validation(method in "[A-Za-z]{1,8}") {
        let (port, _heads) = loopback_server(http_response("200 OK", &[], b""), 1);
        let mut fixture = granted(port);
        let valid = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"].contains(&method.as_str());
        let result = fixture.http().fetch(request(&method, &url(port, "/")));
        prop_assert_eq!(result.is_ok(), valid);
    }
}
