//! Service `ilium.core.keys-and-help/bindings` (list: current key bindings with labels) and the
//! topic `ilium.core.keys-and-help/changed` ({ reason } after a remap).

use ilium_plugin_sdk::exports::ServiceReply;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::testing::FakeSession;
use serde_json::{json, Value};

use crate::common::*;

const SERVICE: &str = "ilium.core.keys-and-help/bindings";
const TOPIC: &str = "ilium.core.keys-and-help/changed";

fn session() -> FakeSession {
    let mut session = FakeSession::new();
    session.install_core("keys-and-help").start();
    session.pump().expect("pump");
    session
        .plugin(PLUGIN_ID)
        .keymap()
        .set_bindings(sample_bindings());
    seed_sample_labels(session.plugin(PLUGIN_ID));
    session
}

fn reply_json(reply: ServiceReply) -> Value {
    match reply {
        ServiceReply::Done(text) => serde_json::from_str(&text).expect("JSON reply"),
        ServiceReply::Deferred => panic!("the bindings service answers at once"),
    }
}

/// REQ: F-010
#[test]
fn service_010_the_manifest_exports_the_service_provider_and_provides_the_bindings_service() {
    let manifest = manifest();
    let exports: Vec<&str> = manifest["client"]["exports"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(exports.contains(&"service-provider"));
    assert!(client_grants().contains(&format!("services.provide:{SERVICE}")));
}

/// REQ: F-010
#[test]
fn service_010_a_caller_with_the_use_grant_gets_the_current_bindings_with_labels() {
    let mut session = session();
    session.grant_service("ilium.test.caller", SERVICE);
    let reply = session
        .call_service("ilium.test.caller", SERVICE, "^0.1", "{}")
        .expect("answers");
    let value = reply_json(reply);
    let list = value.as_array().expect("a list");
    assert_eq!(list.len(), 4);
    assert_eq!(list[0]["command"], "Split vertically");
    assert_eq!(list[0]["label"], "Prefix  %");
}

/// REQ: F-010
#[test]
fn service_010_a_caller_without_the_use_grant_is_refused_with_not_permitted() {
    let mut session = session();
    let error = session
        .call_service("ilium.test.caller", SERVICE, "^0.1", "{}")
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::NotPermitted);
}

/// REQ: F-010
#[test]
fn service_010_a_version_range_the_provider_does_not_satisfy_is_refused_with_not_found() {
    let mut session = session();
    session.grant_service("ilium.test.caller", SERVICE);
    let error = session
        .call_service("ilium.test.caller", SERVICE, "^9.0", "{}")
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::NotFound);
}

/// REQ: F-010
#[test]
fn service_010_malformed_request_json_is_refused_with_invalid_argument() {
    let mut session = session();
    session.grant_service("ilium.test.caller", SERVICE);
    let error = session
        .call_service("ilium.test.caller", SERVICE, "^0.1", "not json")
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-010
#[test]
fn service_010_an_empty_keymap_gives_an_empty_list() {
    let mut session = session();
    session.plugin(PLUGIN_ID).keymap().set_bindings(Vec::new());
    session.grant_service("ilium.test.caller", SERVICE);
    let value = reply_json(
        session
            .call_service("ilium.test.caller", SERVICE, "^0.1", "{}")
            .expect("answers"),
    );
    assert_eq!(value, json!([]));
}

/// REQ: F-010
#[test]
fn service_010_the_service_reflects_a_remap_made_after_the_first_call() {
    let mut session = session();
    session.grant_service("ilium.test.caller", SERVICE);
    session
        .call_service("ilium.test.caller", SERVICE, "^0.1", "{}")
        .expect("answers");
    session
        .plugin(PLUGIN_ID)
        .keymap()
        .set_label("prefix+z", "Prefix  z");
    session
        .plugin(PLUGIN_ID)
        .keymap()
        .set_bindings(vec![binding(
            "Panes",
            "Split vertically",
            "prefix+z",
            false,
        )]);
    session.plugin(PLUGIN_ID).deliver_one(Event::KeymapChanged);
    session.pump().expect("pump");
    let value = reply_json(
        session
            .call_service("ilium.test.caller", SERVICE, "^0.1", "{}")
            .expect("answers"),
    );
    assert_eq!(value[0]["label"], "Prefix  z");
}

/// REQ: F-010
#[test]
fn topic_010_a_remap_publishes_changed_with_a_reason() {
    let mut session = session();
    session.plugin(PLUGIN_ID).deliver_one(Event::KeymapChanged);
    session.pump().expect("pump");
    let published = session.plugin(PLUGIN_ID).published_topics();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].0, TOPIC);
    assert!(
        serde_json::from_str::<serde_json::Value>(&published[0].1)
            .expect("json")
            .get("reason")
            .is_some(),
        "{:?}",
        published[0].1
    );
}

/// REQ: F-010
#[test]
fn topic_010_the_reason_of_a_keymap_change_is_remap() {
    let mut session = session();
    session.plugin(PLUGIN_ID).deliver_one(Event::KeymapChanged);
    session.pump().expect("pump");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &session.plugin(PLUGIN_ID).published_topics()[0].1
        )
        .expect("json")["reason"],
        "remap"
    );
}

/// REQ: F-010
#[test]
fn topic_010_no_topic_is_published_without_a_change() {
    let mut session = session();
    session.pump().expect("pump");
    assert!(session.plugin(PLUGIN_ID).published_topics().is_empty());
}

/// REQ: F-010
#[test]
fn topic_010_the_manifest_lists_the_changed_topic() {
    assert!(manifest()["contributes"]["topics"]["publishes"]
        .as_array()
        .is_some_and(|topics| topics
            .iter()
            .any(|topic| topic == "changed" || topic == TOPIC)));
}
