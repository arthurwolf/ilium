//! The service `ilium.core.inference/complete`, reached by other plugins through the kernel.

use ilium_plugin_sdk::testing::FakeSession;
use serde_json::json;

use crate::common::{manifest_path, PLUGIN_ID};

fn session() -> FakeSession {
    let mut session = FakeSession::new();
    session
        .try_install(manifest_path().parent().expect("package dir"))
        .expect("install");
    session
}

/// REQ: F-084
#[test]
fn service_084_the_service_is_listed_after_install() {
    let session = session();
    assert!(session
        .services()
        .iter()
        .any(|name| name == "ilium.core.inference/complete"));
}

/// REQ: F-084
#[test]
fn service_084_a_caller_with_the_use_grant_gets_a_completion() {
    let mut session = session();
    session.grant_service("ilium.test.caller", "ilium.core.inference/complete");
    let reply = session.call_service(
        "ilium.test.caller",
        "ilium.core.inference/complete",
        "^0.1",
        &json!({"purpose": "title", "messages": [{"role": "user", "content": "hi"}]}).to_string(),
    );
    let body = match reply.expect("completion") {
        ilium_plugin_sdk::exports::ServiceReply::Done(body) => body,
        other => panic!("expected an immediate completion, got {other:?}"),
    };
    // manual does not pin the reply fields beyond the completion shape: text and model.
    let value: serde_json::Value = serde_json::from_str(&body).expect("JSON reply");
    assert!(value["text"].as_str().is_some(), "{value}");
    assert!(value["model"].as_str().is_some(), "{value}");
}

/// REQ: F-084, SEC-293
#[test]
fn service_293_a_caller_without_the_grant_is_refused_with_not_permitted() {
    let mut session = session();
    let reply = session.call_service(
        "ilium.test.caller",
        "ilium.core.inference/complete",
        "^0.1",
        "{}",
    );
    assert_eq!(
        reply.expect_err("refused").code,
        ilium_plugin_sdk::types::ErrorCode::NotPermitted
    );
}

/// REQ: F-084
#[test]
fn service_084_malformed_arguments_are_refused_with_invalid_argument() {
    let mut session = session();
    session.grant_service("ilium.test.caller", "ilium.core.inference/complete");
    let reply = session.call_service(
        "ilium.test.caller",
        "ilium.core.inference/complete",
        "^0.1",
        "not json",
    );
    assert_eq!(
        reply.expect_err("refused").code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: COR-022
#[test]
fn service_022_disabling_the_plugin_removes_the_service_and_keeps_values() {
    let mut session = session();
    session.disable(PLUGIN_ID);
    assert!(!session
        .services()
        .iter()
        .any(|name| name == "ilium.core.inference/complete"));
    assert!(session.kernel_settings_value("inference").is_some());
}

/// REQ: COR-022
#[test]
fn service_022_a_disabled_plugin_shows_the_page_notice() {
    let mut session = session();
    session.disable(PLUGIN_ID);
    assert!(session
        .disabled_notice("inference")
        .contains("Inference providers is disabled. Values are kept."));
}
