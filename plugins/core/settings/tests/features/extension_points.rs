//! The extension points, services and topic the page declares: `config-importers` (collect),
//! `onboarding-steps` (collect), services `ilium.core.settings/get` and `/set`, topic
//! `ilium.core.settings/changed`. Collect points are filled by the kernel, so the point tests
//! read the page tables; the services are called through the session harness.

use ilium_plugin_sdk::exports::ServiceReply;
use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::ErrorCode;
use serde_json::{Value, json};

use crate::common::*;

fn point_rows() -> Vec<Vec<String>> {
    table_rows(&page_text(), "| Point | Kind |")
}

fn reference_page() -> String {
    manual_page("reference-core-extension-points.md")
}

/// REQ: F-089, F-090
#[test]
fn points_090_the_page_lists_exactly_two_points() {
    let names: Vec<String> = point_rows().into_iter().map(|row| row[0].trim_matches('`').to_string()).collect();
    assert_eq!(names, ["config-importers", "onboarding-steps"]);
}

/// REQ: F-090
#[test]
fn points_090_config_importers_is_a_collect_point_with_entry_id_title_service() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`config-importers`").expect("row");
    assert_eq!(row[1], "collect");
    assert_eq!(row[2], "Offer to import settings from another tool.");
    assert_eq!(row[3], "entry: id, title, service (returns key and value pairs)");
    assert_eq!(row[4], "standard");
}

/// REQ: F-089
#[test]
fn points_089_onboarding_steps_is_a_collect_point_with_entry_id_title_order() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`onboarding-steps`").expect("row");
    assert_eq!(row[1], "collect");
    assert_eq!(row[2], "Add a step to the guided setup.");
    assert_eq!(row[3], "entry: id, title, order (the plugin fills it with a view)");
    assert_eq!(row[4], "standard");
}

/// REQ: F-090
#[test]
fn points_090_every_point_is_standard_risk() {
    assert!(point_rows().iter().all(|row| row[4] == "standard"));
}

/// REQ: F-090
#[test]
fn points_090_the_reference_page_agrees_with_the_plugin_page() {
    let reference = reference_page();
    assert!(reference.contains("| `ilium.core.settings/config-importers` | collect | standard | Offer to import settings from another tool. |"));
    assert!(reference.contains("| `ilium.core.settings/onboarding-steps` | collect | standard | Add a step to the guided setup. |"));
    assert!(reference.contains("| [Settings](/pages/core-plugin-settings/) | `ilium.core.settings` | 2 | 2 | 1 | no |"));
}

/// REQ: F-090
#[test]
fn points_090_the_plugin_is_not_replaceable() {
    assert!(page_text().contains("**Replaceable:** no."));
    assert!(page_text().contains("so `replaceable` is `false`."));
}

/// REQ: F-090
#[test]
fn points_090_the_plugin_owns_its_points_through_the_provide_grant() {
    assert!(page_text().contains("(grant `extensions.provide:ilium.core.settings/<point>`)"));
    assert!(page_text().contains("Contributors need `extensions.contribute:ilium.core.settings/<point>`."));
}

/// REQ: F-090
#[test]
fn points_090_the_example_import_entry_names_the_service_the_package_provides() {
    let example = json_block_after(&page_text(), "#### Example: Adds an import for tmux key settings");
    let entry = &example["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.settings/config-importers");
    assert_eq!(entry["entry"]["service"], "example.tmux/import");
    assert_eq!(entry["priority"], 50);
    assert_eq!(example["capabilities"]["server"], json!(["services.provide:example.tmux/import"]));
}

fn started_session() -> FakeSession {
    let mut session = FakeSession::new();
    session.install_core("settings").start();
    session
}

fn reply_json(reply: ServiceReply) -> Value {
    match reply {
        ServiceReply::Done(text) => serde_json::from_str(&text).expect("reply is JSON"),
        ServiceReply::Deferred => panic!("the service answers at once"),
    }
}

/// REQ: F-090
#[test]
fn service_090_get_reads_a_setting_by_key() {
    let mut session = started_session();
    let reply = session
        .call_service(PLUGIN_ID, "ilium.core.settings/get", "^0.1", r#"{"key":"density"}"#)
        .expect("service answers");
    assert_eq!(reply_json(reply)["value"], json!("comfortable"));
}

/// REQ: F-090
#[test]
fn service_090_get_reads_a_kernel_tab_value() {
    let mut session = started_session();
    let reply = session
        .call_service(PLUGIN_ID, "ilium.core.settings/get", "^0.1", r#"{"key":"ui.motion_level"}"#)
        .expect("service answers");
    assert_eq!(reply_json(reply)["value"], json!("reduced"));
}

/// REQ: F-090
#[test]
fn service_090_set_writes_a_setting_by_key() {
    let mut session = started_session();
    session
        .call_service(PLUGIN_ID, "ilium.core.settings/set", "^0.1", r#"{"key":"density","value":"compact"}"#)
        .expect("service answers");
    session.pump().expect("pump");
    assert_eq!(session.plugin(PLUGIN_ID).settings().get("density"), Some(json!("compact")));
}

/// REQ: F-090
#[test]
fn service_090_set_then_get_round_trips() {
    let mut session = started_session();
    session
        .call_service(PLUGIN_ID, "ilium.core.settings/set", "^0.1", r##"{"key":"accent","value":"#102030"}"##)
        .expect("set answers");
    let reply = session
        .call_service(PLUGIN_ID, "ilium.core.settings/get", "^0.1", r#"{"key":"accent"}"#)
        .expect("get answers");
    assert_eq!(reply_json(reply)["value"], json!("#102030"));
}

/// REQ: F-090
#[test]
fn service_090_set_with_an_invalid_value_is_refused_with_invalid_argument() {
    let mut session = started_session();
    let error = session
        .call_service(PLUGIN_ID, "ilium.core.settings/set", "^0.1", r#"{"key":"density","value":"spacious"}"#)
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(session.plugin(PLUGIN_ID).settings().get("density"), Some(json!("comfortable")));
}

/// REQ: F-090
#[test]
fn service_090_a_request_that_is_not_json_is_refused_with_invalid_argument() {
    let mut session = started_session();
    for name in ["ilium.core.settings/get", "ilium.core.settings/set"] {
        let error = session.call_service(PLUGIN_ID, name, "^0.1", "{not json").expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{name}");
    }
}

/// REQ: F-090
#[test]
fn service_090_a_request_without_a_key_is_refused_with_invalid_argument() {
    let mut session = started_session();
    let error = session
        .call_service(PLUGIN_ID, "ilium.core.settings/get", "^0.1", "{}")
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-090
#[test]
fn service_090_an_unknown_service_name_is_not_found() {
    let mut session = started_session();
    let error = session
        .call_service(PLUGIN_ID, "ilium.core.settings/delete", "^0.1", "{}")
        .expect_err("refused");
    assert_eq!(error.code, ErrorCode::NotFound);
}

/// REQ: F-090
#[test]
fn service_090_the_page_lists_exactly_the_get_and_set_services() {
    let page = page_text();
    assert!(page.contains("- `ilium.core.settings/get`: get: read a setting by key"));
    assert!(page.contains("- `ilium.core.settings/set`: set: write a setting by key"));
    assert!(page.contains("**Services** (grant `services.use:<name>`):"));
}

/// REQ: F-090
#[test]
fn topic_090_the_page_lists_the_changed_topic_with_a_key_payload() {
    let page = page_text();
    assert!(page.contains("- `ilium.core.settings/changed`: { key }"));
    assert!(reference_page().contains("| `ilium.core.settings/changed` | { key } |"));
}
