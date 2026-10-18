//! `ilium:host/http-api` (api-http-api.md): answering an HTTP call.

use std::time::Duration;

use ilium_plugin_api::generated::host::http_api::HostHttpApi;
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &["http-api.token"];

fn open_call(rig: &Rig) -> u64 {
    rig.kernel.start_http_call("GET", "/x")
}

/// REQ: WIT:ilium:host/http-api.respond
#[test]
fn wit_http_api_respond_delivers_status_headers_and_body() {
    let rig = server_rig(GRANTS);
    let call = open_call(&rig);
    rig.host
        .http_api()
        .respond(
            call,
            200,
            vec![("content-type".to_owned(), "text/plain".to_owned())],
            b"hi".to_vec(),
        )
        .expect("answers");
    let answer = rig.kernel.http_response(call).expect("delivered");
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, b"hi");
}

/// REQ: WIT:ilium:host/http-api.respond
#[test]
fn wit_http_api_respond_status_boundaries_invalid_argument() {
    let rig = server_rig(GRANTS);
    for status in [99u16, 600] {
        let call = open_call(&rig);
        assert_eq!(
            code_of(
                rig.host
                    .http_api()
                    .respond(call, status, Vec::new(), Vec::new())
            ),
            ErrorCode::InvalidArgument,
            "{status}"
        );
    }
    for status in [100u16, 599] {
        let call = open_call(&rig);
        assert!(
            rig.host
                .http_api()
                .respond(call, status, Vec::new(), Vec::new())
                .is_ok(),
            "{status}"
        );
    }
}

/// REQ: WIT:ilium:host/http-api.respond
#[test]
fn wit_http_api_respond_body_size_boundaries_invalid_argument() {
    let rig = server_rig(GRANTS);
    let at_limit = open_call(&rig);
    assert!(rig
        .host
        .http_api()
        .respond(at_limit, 200, Vec::new(), vec![0; 8 * MIB])
        .is_ok());
    let over = open_call(&rig);
    assert_eq!(
        code_of(
            rig.host
                .http_api()
                .respond(over, 200, Vec::new(), vec![0; 8 * MIB + 1])
        ),
        ErrorCode::InvalidArgument
    );
    let under = open_call(&rig);
    assert!(rig
        .host
        .http_api()
        .respond(under, 200, Vec::new(), vec![0; 8 * MIB - 1])
        .is_ok());
}

/// REQ: WIT:ilium:host/http-api.respond
#[test]
fn wit_http_api_respond_each_hop_by_hop_header_invalid_argument() {
    let rig = server_rig(GRANTS);
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "Connection",
    ] {
        let call = open_call(&rig);
        let result = rig.host.http_api().respond(
            call,
            200,
            vec![(name.to_owned(), "x".to_owned())],
            Vec::new(),
        );
        assert_eq!(code_of(result), ErrorCode::InvalidArgument, "{name}");
    }
}

/// REQ: WIT:ilium:host/http-api.respond
#[test]
fn wit_http_api_respond_second_answer_not_found() {
    let rig = server_rig(GRANTS);
    let call = open_call(&rig);
    rig.host
        .http_api()
        .respond(call, 204, Vec::new(), Vec::new())
        .expect("first");
    assert_eq!(
        code_of(
            rig.host
                .http_api()
                .respond(call, 204, Vec::new(), Vec::new())
        ),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/http-api.respond
#[test]
fn wit_http_api_respond_unknown_call_not_found() {
    let rig = server_rig(GRANTS);
    assert_eq!(
        code_of(
            rig.host
                .http_api()
                .respond(424_242, 200, Vec::new(), Vec::new())
        ),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/http-api.respond
#[test]
fn wit_http_api_respond_after_thirty_seconds_not_found() {
    let rig = server_rig(GRANTS);
    let call = open_call(&rig);
    rig.host.advance_clock(Duration::from_secs(30));
    rig.host.pump(8);
    assert_eq!(
        code_of(
            rig.host
                .http_api()
                .respond(call, 200, Vec::new(), Vec::new())
        ),
        ErrorCode::NotFound
    );
}
