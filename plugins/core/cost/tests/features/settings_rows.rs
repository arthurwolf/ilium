//! The two Display rows (unit, display): defaults, valid sets, rejected values.

use serde_json::json;

use crate::common::*;

/// REQ: F-038, F-040
#[test]
fn settings_040_unit_defaults_to_dollars() {
    let mut host = activated_server();
    assert_eq!(host.settings().get("unit"), Some(json!("dollars")));
}

/// REQ: F-038, F-040
#[test]
fn settings_040_display_defaults_to_tree_and_status() {
    let mut host = activated_server();
    assert_eq!(
        host.settings().get("display"),
        Some(json!(["tree", "status"]))
    );
}

/// REQ: F-040
#[test]
fn settings_040_unit_accepts_quota() {
    let mut host = activated_server();
    assert!(host.settings().set("unit", json!("quota")).is_ok());
    assert_eq!(host.settings().get("unit"), Some(json!("quota")));
}

/// REQ: F-040
#[test]
fn settings_040_unit_rejects_a_value_outside_the_options_with_invalid_argument() {
    let mut host = activated_server();
    let error = host
        .settings()
        .set("unit", json!("euros"))
        .expect_err("not an option");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
    assert_eq!(host.settings().get("unit"), Some(json!("dollars")));
}

/// REQ: F-040
#[test]
fn settings_040_display_accepts_a_single_surface() {
    let mut host = activated_server();
    assert!(host.settings().set("display", json!(["status"])).is_ok());
    assert_eq!(host.settings().get("display"), Some(json!(["status"])));
}

/// REQ: F-040
#[test]
fn settings_040_display_rejects_duplicates_with_invalid_argument() {
    let mut host = activated_server();
    let error = host
        .settings()
        .set("display", json!(["tree", "tree"]))
        .expect_err("unique items");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: F-040
#[test]
fn settings_040_display_rejects_an_unknown_surface_with_invalid_argument() {
    let mut host = activated_server();
    let error = host
        .settings()
        .set("display", json!(["tree", "toast"]))
        .expect_err("not an option");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: F-040
#[test]
fn settings_040_unit_rejects_a_non_string_with_invalid_argument() {
    let mut host = activated_server();
    let error = host
        .settings()
        .set("unit", json!(3))
        .expect_err("wrong type");
    assert_eq!(
        error.code,
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: F-040
#[test]
fn settings_040_unknown_key_is_rejected_with_not_found() {
    let mut host = activated_server();
    let error = host
        .settings()
        .set("currency", json!("EUR"))
        .expect_err("no such row");
    assert_eq!(error.code, ilium_plugin_sdk::types::ErrorCode::NotFound);
}
