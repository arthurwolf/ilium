//! `ilium:host/client-requests` (api-client-requests.md): server asks a client, client answers.

use std::time::Duration;

use ilium_plugin_api::generated::host::client_requests::HostClientRequests;
use ilium_plugin_api::generated::types::ids::{ClientId, RequestId};
use ilium_plugin_api::{ErrorCode, PluginError};

use crate::common::*;

const GRANTS: &[&str] = &["clipboard.read"];
const CLIENT: ClientId = ClientId(7);

fn seeded() -> Rig {
    let rig = server_rig(GRANTS);
    rig.kernel.register_client(CLIENT, &["clipboard-text"]);
    rig
}

/// REQ: WIT:ilium:host/client-requests.request
#[test]
fn wit_client_requests_request_returns_a_request_id_and_reaches_the_client() {
    let mut rig = seeded();
    let id = rig
        .host
        .client_requests()
        .request(CLIENT, "clipboard-text".to_owned(), "{}".to_owned(), 5_000)
        .expect("sent");
    assert!(rig
        .kernel
        .client_inbox(CLIENT)
        .iter()
        .any(|(kind, _)| kind == "clipboard-text"));
    let _ = id;
}

/// REQ: WIT:ilium:host/client-requests.request
#[test]
fn wit_client_requests_request_timeout_boundaries_invalid_argument() {
    let mut rig = seeded();
    let ask = |rig: &mut Rig, millis: u32| {
        rig.host.client_requests().request(
            CLIENT,
            "clipboard-text".to_owned(),
            "{}".to_owned(),
            millis,
        )
    };
    assert_eq!(code_of(ask(&mut rig, 0)), ErrorCode::InvalidArgument);
    assert!(ask(&mut rig, 1).is_ok());
    assert!(ask(&mut rig, 30_000).is_ok());
    assert_eq!(code_of(ask(&mut rig, 30_001)), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/client-requests.request
#[test]
fn wit_client_requests_request_unknown_client_not_found() {
    let mut rig = seeded();
    let result = rig.host.client_requests().request(
        ClientId(404),
        "clipboard-text".to_owned(),
        "{}".to_owned(),
        1_000,
    );
    assert_eq!(code_of(result), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/client-requests.request
#[test]
fn wit_client_requests_request_undeclared_kind_not_found() {
    let mut rig = seeded();
    let result =
        rig.host
            .client_requests()
            .request(CLIENT, "nope".to_owned(), "{}".to_owned(), 1_000);
    assert_eq!(code_of(result), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/client-requests.request
#[test]
fn wit_client_requests_request_unanswered_completes_timed_out() {
    let mut rig = seeded();
    let id = rig
        .host
        .client_requests()
        .request(CLIENT, "clipboard-text".to_owned(), "{}".to_owned(), 100)
        .expect("sent");
    rig.host.advance_clock(Duration::from_millis(100));
    rig.host.pump(8);
    assert_eq!(rig.host.request_error(id), Some(ErrorCode::TimedOut));
}

/// REQ: WIT:ilium:host/client-requests.take-answer
#[test]
fn wit_client_requests_take_answer_returns_the_answer_once() {
    let mut rig = seeded();
    let id = rig
        .host
        .client_requests()
        .request(CLIENT, "clipboard-text".to_owned(), "{}".to_owned(), 5_000)
        .expect("sent");
    rig.kernel
        .client_answers(CLIENT, id, Ok("\"text\"".to_owned()));
    rig.host.pump(8);
    assert_eq!(
        rig.host.client_requests().take_answer(id),
        Some("\"text\"".to_owned())
    );
    assert_eq!(rig.host.client_requests().take_answer(id), None);
}

/// REQ: WIT:ilium:host/client-requests.take-answer
#[test]
fn wit_client_requests_take_answer_before_the_answer_is_none() {
    let mut rig = seeded();
    let id = rig
        .host
        .client_requests()
        .request(CLIENT, "clipboard-text".to_owned(), "{}".to_owned(), 5_000)
        .expect("sent");
    assert_eq!(rig.host.client_requests().take_answer(id), None);
}

/// REQ: WIT:ilium:host/client-requests.take-answer
#[test]
fn wit_client_requests_take_answer_unknown_request_is_none() {
    let mut rig = seeded();
    assert_eq!(
        rig.host.client_requests().take_answer(RequestId(1_234_567)),
        None
    );
}

/// REQ: WIT:ilium:host/client-requests.respond
#[test]
fn wit_client_requests_respond_delivers_the_answer_to_the_server() {
    let mut rig = rig(GRANTS);
    let call = rig.kernel.start_client_request_call("clipboard-text", "{}");
    rig.host
        .client_requests()
        .respond(call, Ok("\"yes\"".to_owned()))
        .expect("answered");
    assert_eq!(
        rig.kernel.client_request_answer(call),
        Some(Ok("\"yes\"".to_owned()))
    );
}

/// REQ: WIT:ilium:host/client-requests.respond
#[test]
fn wit_client_requests_respond_with_an_error_outcome_is_forwarded() {
    let mut rig = rig(GRANTS);
    let call = rig.kernel.start_client_request_call("clipboard-text", "{}");
    let failure = PluginError {
        code: ErrorCode::NotPermitted,
        message: "declined".to_owned(),
        retry_after_ms: None,
    };
    rig.host
        .client_requests()
        .respond(call, Err(failure.clone()))
        .expect("answered");
    assert_eq!(rig.kernel.client_request_answer(call), Some(Err(failure)));
}

/// REQ: WIT:ilium:host/client-requests.respond
#[test]
fn wit_client_requests_respond_second_answer_not_found() {
    let mut rig = rig(GRANTS);
    let call = rig.kernel.start_client_request_call("clipboard-text", "{}");
    rig.host
        .client_requests()
        .respond(call, Ok("1".to_owned()))
        .expect("first");
    assert_eq!(
        code_of(rig.host.client_requests().respond(call, Ok("2".to_owned()))),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/client-requests.respond
#[test]
fn wit_client_requests_respond_unknown_call_not_found() {
    let mut rig = rig(GRANTS);
    assert_eq!(
        code_of(
            rig.host
                .client_requests()
                .respond(9_999_999, Ok("1".to_owned()))
        ),
        ErrorCode::NotFound
    );
}
