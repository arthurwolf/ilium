//! A change of the plugin's rows by the user reaches the Animations kernel tab through the
//! service `ilium.core.ambient/set-setting` (COR-030, COR-033). The client part makes the call;
//! the server part validates it and writes the tab (`set_setting.rs`).

use ilium_plugin_sdk::types::{Event, PluginSide, SettingsChange, SettingsOrigin};
use serde_json::{json, Value};

use crate::common::*;

const SERVICE: &str = "ilium.core.ambient/set-setting";

fn payloads(host: &ilium_plugin_sdk::testing::FakeHost) -> Vec<Value> {
    host.service_calls()
        .into_iter()
        .filter(|(service, method, _)| service == SERVICE && method == "set")
        .map(|(_, _, payload)| serde_json::from_str(&payload).expect("the payload is JSON"))
        .collect()
}

fn change(keys: &[&str], origin: SettingsOrigin) -> Event {
    Event::SettingsChanged(SettingsChange {
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        origin,
        side: PluginSide::Client,
    })
}

/// REQ: COR-030
#[test]
fn cor_030_a_user_change_of_the_scene_row_calls_the_set_setting_service() {
    let mut host = activated_client();
    host.settings().set("scene", json!("clouds")).expect("set");
    let submissions = host.deliver(vec![settings_changed(&["scene"])]);
    host.settle();
    assert!(submissions.is_empty());
    assert_eq!(
        payloads(&host),
        vec![json!({ "tab": "animations", "key": "scene", "value": "clouds" })]
    );
}

/// REQ: COR-030
#[test]
fn cor_030_a_user_change_of_the_semantic_row_sends_true_or_false_text() {
    let mut host = activated_client();
    host.settings().set("semantic", json!(true)).expect("set");
    host.deliver(vec![settings_changed(&["semantic"])]);
    host.settle();
    assert_eq!(payloads(&host)[0]["value"], "true");
}

/// REQ: COR-030
#[test]
fn cor_030_one_call_is_made_per_changed_row_that_reaches_the_kernel_tab() {
    let mut host = activated_client();
    host.settings().set("scene", json!("stars")).expect("set");
    host.settings().set("semantic", json!(false)).expect("set");
    host.deliver(vec![settings_changed(&["scene", "semantic", "unrelated"])]);
    host.settle();
    let keys: Vec<String> = payloads(&host)
        .iter()
        .map(|payload| payload["key"].as_str().expect("key").to_owned())
        .collect();
    assert_eq!(keys, vec!["scene".to_owned(), "semantic".to_owned()]);
}

/// A change this plugin or a file made is not sent again, so the service never echoes.
///
/// REQ: COR-033
#[test]
fn cor_033_a_change_that_is_not_the_users_makes_no_call() {
    let mut host = activated_client();
    host.settings().set("scene", json!("clouds")).expect("set");
    for origin in [
        SettingsOrigin::Plugin,
        SettingsOrigin::OtherPlugin,
        SettingsOrigin::Reset,
        SettingsOrigin::File,
    ] {
        host.deliver(vec![change(&["scene"], origin)]);
    }
    host.settle();
    assert!(payloads(&host).is_empty());
}

/// REQ: COR-030
#[test]
fn cor_030_a_change_that_names_no_row_of_ours_makes_no_call() {
    let mut host = activated_client();
    host.deliver(vec![settings_changed(&[]), settings_changed(&["other"])]);
    host.settle();
    assert!(payloads(&host).is_empty());
}

/// A refusal of the service is logged and never fails the plugin.
///
/// REQ: COR-030
#[test]
fn cor_030_a_refused_call_never_fails_the_plugin() {
    let mut host = activated_client();
    host.stub_service(
        SERVICE,
        "set",
        r#"{"error":{"code":"not-permitted","message":"the grant is missing"}}"#,
    );
    host.settings().set("scene", json!("clouds")).expect("set");
    host.deliver(vec![settings_changed(&["scene"])]);
    host.settle();
    assert_eq!(payloads(&host).len(), 1);
    assert!(host.failures().is_empty());
    assert!(!host.is_disabled());
}
