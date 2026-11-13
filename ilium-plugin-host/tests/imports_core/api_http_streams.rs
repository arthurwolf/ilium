//! api-http-streams.md: `open-stream`, `close-stream`, `open-websocket`, `send-websocket`.

use ilium_plugin_api::generated::host::{HttpRequest, StreamFraming, StreamRequest};
use ilium_plugin_api::generated::types::{ErrorCode, StreamId};
use ilium_plugin_api::Tier;
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;

use crate::support::*;

fn stream_request(port: u16, framing: StreamFraming, max_chunk_bytes: u32) -> StreamRequest {
    StreamRequest {
        request: HttpRequest {
            method: "GET".to_owned(),
            url: format!("http://127.0.0.1:{port}/stream"),
            headers: Vec::new(),
            body: None,
            timeout_millis: 5000,
        },
        framing,
        max_chunk_bytes,
    }
}

fn granted(tier: Tier, port: u16) -> ImportsFixture {
    let grant = format!("http:127.0.0.1:{port}");
    fixture(tier, &[grant.as_str()])
}

fn listener() -> (std::net::TcpListener, u16) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

fn websocket_on(grants: &[&str]) -> ImportsFixture {
    builder_in(std::path::Path::new("."), Tier::T2, grants)
        .websocket_feature(true)
        .build()
}

/// REQ: WIT:ilium:host/http.open-stream
#[test]
fn http_open_stream_returns_a_stream_id_and_delivers_chunks_to_events() {
    let (port, _heads) = loopback_server(http_response("200 OK", &[], b"line one\nline two\n"), 1);
    let mut fixture = granted(Tier::T2, port);
    let stream = expect_ok(fixture.http().open_stream(stream_request(
        port,
        StreamFraming::Lines,
        1024,
    )));
    let chunks = fixture.wait_stream_chunks(stream, 2, WAIT);
    assert_eq!(chunks, vec![b"line one".to_vec(), b"line two".to_vec()]);
}

/// REQ: WIT:ilium:host/http.open-stream
#[test]
fn http_open_stream_max_chunk_bytes_of_exactly_64_kib_accepted_and_one_more_invalid_argument() {
    let (_listener, port) = listener();
    let mut fixture = granted(Tier::T2, port);
    assert!(fixture
        .http()
        .open_stream(stream_request(port, StreamFraming::Raw, 64 * 1024))
        .is_ok());
    assert_eq!(
        expect_code(fixture.http().open_stream(stream_request(
            port,
            StreamFraming::Raw,
            64 * 1024 + 1
        ))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/http.open-stream
#[test]
fn http_open_stream_zero_max_chunk_bytes_invalid_argument() {
    let (_listener, port) = listener();
    let mut fixture = granted(Tier::T2, port);
    assert_eq!(
        expect_code(
            fixture
                .http()
                .open_stream(stream_request(port, StreamFraming::Raw, 0))
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/http.open-stream
#[test]
fn http_open_stream_without_a_grant_not_permitted() {
    let (_listener, port) = listener();
    let mut fixture = fixture(Tier::T2, &[]);
    assert_eq!(
        expect_code(
            fixture
                .http()
                .open_stream(stream_request(port, StreamFraming::Raw, 1024))
        ),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/http.open-stream
#[test]
fn http_open_stream_5th_stream_of_a_t2_plugin_quota_exceeded() {
    let (_listener, port) = listener();
    let mut fixture = granted(Tier::T2, port);
    for _ in 0..4 {
        expect_ok(
            fixture
                .http()
                .open_stream(stream_request(port, StreamFraming::Raw, 1024)),
        );
    }
    assert_eq!(
        expect_code(
            fixture
                .http()
                .open_stream(stream_request(port, StreamFraming::Raw, 1024))
        ),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/http.open-stream
#[test]
fn http_open_stream_t1_plugin_may_hold_16_streams_and_not_17() {
    let (_listener, port) = listener();
    let mut fixture = granted(Tier::T1, port);
    for _ in 0..16 {
        expect_ok(
            fixture
                .http()
                .open_stream(stream_request(port, StreamFraming::Raw, 1024)),
        );
    }
    assert_eq!(
        expect_code(
            fixture
                .http()
                .open_stream(stream_request(port, StreamFraming::Raw, 1024))
        ),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/http.close-stream
#[test]
fn http_close_stream_frees_a_slot_under_the_stream_limit() {
    let (_listener, port) = listener();
    let mut fixture = granted(Tier::T2, port);
    let mut streams = Vec::new();
    for _ in 0..4 {
        streams.push(expect_ok(fixture.http().open_stream(stream_request(
            port,
            StreamFraming::Raw,
            1024,
        ))));
    }
    fixture.http().close_stream(streams[0]);
    assert!(fixture
        .http()
        .open_stream(stream_request(port, StreamFraming::Raw, 1024))
        .is_ok());
}

/// REQ: WIT:ilium:host/http.close-stream
#[test]
fn http_close_stream_of_an_unknown_stream_is_harmless() {
    let mut fixture = fixture(Tier::T2, &[]);
    fixture.http().close_stream(StreamId(909_090));
    assert!(fixture.logs().iter().all(|line| line.level != "error"));
}

/// REQ: WIT:ilium:host/http.open-websocket
#[test]
fn http_open_websocket_is_unsupported_when_the_websocket_feature_is_off() {
    let mut fixture = builder_in(std::path::Path::new("."), Tier::T2, &["http:127.0.0.1"])
        .websocket_feature(false)
        .build();
    let result = fixture
        .http()
        .open_websocket("ws://127.0.0.1:9/".to_owned(), Vec::new());
    assert_eq!(expect_code(result), ErrorCode::Unsupported);
}

/// REQ: WIT:ilium:host/http.open-websocket
#[test]
fn http_open_websocket_without_a_grant_not_permitted() {
    let mut fixture = websocket_on(&[]);
    let result = fixture
        .http()
        .open_websocket("ws://127.0.0.1:9/".to_owned(), Vec::new());
    assert_eq!(expect_code(result), ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/http.open-websocket
#[test]
fn http_open_websocket_malformed_url_invalid_argument() {
    let mut fixture = websocket_on(&["http:127.0.0.1"]);
    let code = expect_code(
        fixture
            .http()
            .open_websocket("not a url".to_owned(), Vec::new()),
    );
    // manual does not pin the exact code for a malformed websocket url
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/http.send-websocket
#[test]
fn http_send_websocket_on_an_unknown_stream_not_found() {
    let mut fixture = websocket_on(&[]);
    let code = expect_code(
        fixture
            .http()
            .send_websocket(StreamId(31_415), "hi".to_owned()),
    );
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/http.send-websocket
#[test]
fn http_send_websocket_on_a_plain_http_stream_invalid_argument() {
    let (_listener, port) = listener();
    let grant = format!("http:127.0.0.1:{port}");
    let mut fixture = websocket_on(&[grant.as_str()]);
    let stream = expect_ok(fixture.http().open_stream(stream_request(
        port,
        StreamFraming::Raw,
        1024,
    )));
    let code = expect_code(fixture.http().send_websocket(stream, "hi".to_owned()));
    assert_eq!(code, ErrorCode::InvalidArgument);
}
