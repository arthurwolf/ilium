//! Service `ilium.core.session-convert/convert`, method `start`: another plugin starts a conversion (F-077).

use ilium_plugin_sdk::exports::ServiceReply;
use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::ErrorCode;
use serde_json::{json, Value};

use crate::common::*;

const SERVICE: &str = "ilium.core.session-convert/convert";

fn session_with_plugin() -> FakeSession {
    let mut session = FakeSession::new();
    session.install(env!("CARGO_MANIFEST_DIR")).start();
    session
}

fn call(
    session: &mut FakeSession,
    request: &str,
) -> Result<ServiceReply, ilium_plugin_sdk::types::PluginError> {
    session.call_service("test.caller", SERVICE, "^0.1", request)
}

fn session_with_agent_pane(signature: &str) -> (FakeSession, u64) {
    let mut session = session_with_plugin();
    let pane = add_agent_pane(session.plugin(PLUGIN_ID), "agent", signature);
    (session, pane)
}

/// REQ: F-077
#[test]
fn svc_077_the_page_describes_start_as_converting_a_transcript_between_two_formats() {
    assert!(page_text().contains(
        "`ilium.core.session-convert/convert`: start: convert a transcript between two formats"
    ));
}

/// REQ: F-077
#[test]
fn svc_077_start_answers_with_a_conversion_id() {
    let (mut session, pane) = session_with_agent_pane("codex");
    let reply = call(
        &mut session,
        &json!({"pane": pane, "target": "claude"}).to_string(),
    )
    .expect("answers");
    let ServiceReply::Done(text) = reply else {
        panic!("{reply:?}")
    };
    let value: Value = serde_json::from_str(&text).unwrap();
    assert!(
        value["conversion"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "{value}"
    );
}

/// REQ: F-077
#[test]
fn svc_077_two_starts_get_different_conversion_ids() {
    let (mut session, first) = session_with_agent_pane("codex");
    let second = add_agent_pane(session.plugin(PLUGIN_ID), "second", "codex");
    let one = call(
        &mut session,
        &json!({"pane": first, "target": "claude"}).to_string(),
    )
    .expect("first");
    let two = call(
        &mut session,
        &json!({"pane": second, "target": "claude"}).to_string(),
    )
    .expect("second");
    assert_ne!(one, two);
}

/// REQ: F-077
#[test]
fn svc_077_start_freezes_the_pane_like_the_menu_does() {
    let (mut session, pane) = session_with_agent_pane("codex");
    call(
        &mut session,
        &json!({"pane": pane, "target": "claude"}).to_string(),
    )
    .expect("answers");
    assert_eq!(frozen(&session.plugin(PLUGIN_ID).submitted()), [pane]);
}

/// REQ: F-077
#[test]
fn svc_077_a_target_outside_the_enum_is_invalid_argument() {
    let (mut session, pane) = session_with_agent_pane("codex");
    let error = call(
        &mut session,
        &json!({"pane": pane, "target": "gemini"}).to_string(),
    )
    .expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-077
#[test]
fn svc_077_a_missing_target_is_invalid_argument() {
    let (mut session, pane) = session_with_agent_pane("codex");
    let error = call(&mut session, &json!({"pane": pane}).to_string()).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-077
#[test]
fn svc_077_a_missing_pane_is_invalid_argument() {
    let mut session = session_with_plugin();
    let error = call(&mut session, r#"{"target":"claude"}"#).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-077
#[test]
fn svc_077_a_pane_that_is_not_an_integer_is_invalid_argument() {
    let mut session = session_with_plugin();
    let error = call(&mut session, r#"{"pane":"one","target":"claude"}"#).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-077
#[test]
fn svc_077_a_malformed_request_is_invalid_argument() {
    let mut session = session_with_plugin();
    assert_eq!(
        call(&mut session, "not json").expect_err("refused").code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-077
#[test]
fn svc_077_an_unknown_pane_is_not_found() {
    let mut session = session_with_plugin();
    let error = call(&mut session, r#"{"pane":99999,"target":"claude"}"#).expect_err("refused");
    assert_eq!(error.code, ErrorCode::NotFound);
}

/// REQ: F-077
#[test]
fn svc_077_converting_to_the_same_agent_is_invalid_argument() {
    let (mut session, pane) = session_with_agent_pane("claude");
    let error = call(
        &mut session,
        &json!({"pane": pane, "target": "claude"}).to_string(),
    )
    .expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-077
#[test]
fn svc_077_a_refused_start_freezes_nothing() {
    let (mut session, pane) = session_with_agent_pane("claude");
    let _ = call(
        &mut session,
        &json!({"pane": pane, "target": "claude"}).to_string(),
    );
    assert!(frozen(&session.plugin(PLUGIN_ID).submitted()).is_empty());
}

/// REQ: F-077
#[test]
fn svc_077_an_unknown_service_version_is_not_found() {
    let mut session = session_with_plugin();
    let error = session
        .call_service("test.caller", SERVICE, "^9.0", "{}")
        .expect_err("no provider");
    assert_eq!(error.code, ErrorCode::NotFound);
}
