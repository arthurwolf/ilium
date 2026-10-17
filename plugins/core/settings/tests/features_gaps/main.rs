//! Feature tests of `ilium.core.settings` that need testkit items the skeleton lacks (see
//! ../API_GAPS.md): the topic publications of a host and the invocation of a collect point.

use ilium_plugin_sdk::testing::FakeSession;
use serde_json::{json, Value};

const PLUGIN_ID: &str = "ilium.core.settings";

fn started_session() -> FakeSession {
    let mut session = FakeSession::new();
    session.install_core("settings").start();
    session
}

/// REQ: F-090
#[test]
fn topic_090_set_publishes_changed_with_the_key() {
    let mut session = started_session();
    session
        .call_service(
            PLUGIN_ID,
            "ilium.core.settings/set",
            "^0.1",
            r#"{"key":"density","value":"compact"}"#,
        )
        .expect("set answers");
    session.pump().expect("pump");
    let published = session.plugin(PLUGIN_ID).published_topics();
    let payloads: Vec<Value> = published
        .iter()
        .filter(|(topic, _)| topic == "ilium.core.settings/changed")
        .map(|(_, payload)| serde_json::from_str(payload).expect("payload JSON"))
        .collect();
    assert_eq!(payloads, [json!({"key": "density"})]);
}

/// REQ: F-090
#[test]
fn topic_090_a_refused_set_publishes_nothing() {
    let mut session = started_session();
    let _ = session.call_service(
        PLUGIN_ID,
        "ilium.core.settings/set",
        "^0.1",
        r#"{"key":"density","value":"x"}"#,
    );
    session.pump().expect("pump");
    let published = session.plugin(PLUGIN_ID).published_topics();
    assert!(published
        .iter()
        .all(|(topic, _)| topic != "ilium.core.settings/changed"));
}

/// REQ: F-090
#[test]
fn points_090_config_importers_collects_the_contributions_of_installed_packages() {
    let mut session = started_session();
    let entries = session
        .call_extension("ilium.core.settings/config-importers", "{}")
        .expect("collect answers");
    assert!(entries.is_empty());
}

/// REQ: F-089
#[test]
fn points_089_onboarding_steps_collects_the_steps_ordered_by_order_then_id() {
    let mut session = started_session();
    let entries = session
        .call_extension("ilium.core.settings/onboarding-steps", "{}")
        .expect("collect answers");
    let first: Value = serde_json::from_str(&entries[0]).expect("entry JSON");
    assert_eq!(first["id"], "first-run");
    assert_eq!(first["order"], 10);
}
