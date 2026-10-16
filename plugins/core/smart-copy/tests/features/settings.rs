//! Kernel settings the plugin reads (COR-011): `[ui] terminal_text_selection_enabled`,
//! `[terminal] smart_copy_light` and `[terminal] smart_copy_light_key`. Defaults and validation
//! follow the user documentation (smart-copy.md "Settings", settings.md) and the existing config loader.

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::json;

use crate::common::client_host;

/// REQ: COR-011, F-016
#[test]
fn parity_011_terminal_text_selection_is_on_by_default() {
    let mut host = client_host();
    assert_eq!(host.settings().get("ui.terminal_text_selection_enabled"), Some(json!(true)));
}

/// REQ: COR-011, F-016
#[test]
fn parity_011_terminal_text_selection_accepts_a_boolean() {
    let mut host = client_host();
    host.settings().set("ui.terminal_text_selection_enabled", json!(false)).expect("boolean accepted");
    assert_eq!(host.settings().get("ui.terminal_text_selection_enabled"), Some(json!(false)));
}

/// REQ: COR-011, F-016
#[test]
fn parity_011_terminal_text_selection_refuses_a_non_boolean_with_invalid_argument() {
    let mut host = client_host();
    let error = host
        .settings()
        .set("ui.terminal_text_selection_enabled", json!("yes"))
        .expect_err("not a boolean");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.settings().get("ui.terminal_text_selection_enabled"), Some(json!(true)));
}

/// REQ: COR-011, F-065
#[test]
fn parity_011_smart_copy_light_is_on_by_default() {
    let mut host = client_host();
    assert_eq!(host.settings().get("terminal.smart_copy_light"), Some(json!(true)));
}

/// REQ: COR-011, F-065
#[test]
fn parity_011_smart_copy_light_accepts_a_boolean() {
    let mut host = client_host();
    host.settings().set("terminal.smart_copy_light", json!(false)).expect("boolean accepted");
    assert_eq!(host.settings().get("terminal.smart_copy_light"), Some(json!(false)));
}

/// REQ: COR-011, F-065
#[test]
fn parity_011_smart_copy_light_refuses_a_number_with_invalid_argument() {
    let mut host = client_host();
    let error = host.settings().set("terminal.smart_copy_light", json!(1)).expect_err("not a boolean");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: COR-011, F-065
#[test]
fn parity_011_smart_copy_light_key_defaults_to_ctrl() {
    let mut host = client_host();
    assert_eq!(host.settings().get("terminal.smart_copy_light_key"), Some(json!("ctrl")));
}

/// REQ: COR-011, F-065
#[test]
fn parity_011_smart_copy_light_key_accepts_ctrl_alt_and_shift() {
    let mut host = client_host();
    for value in ["ctrl", "alt", "shift"] {
        host.settings().set("terminal.smart_copy_light_key", json!(value)).expect("allowed value");
        assert_eq!(host.settings().get("terminal.smart_copy_light_key"), Some(json!(value)));
    }
}

/// REQ: COR-011, F-065
#[test]
fn parity_011_smart_copy_light_key_accepts_the_aliases_control_and_option_in_any_case() {
    let mut host = client_host();
    for value in ["control", "Option", " SHIFT "] {
        host.settings()
            .set("terminal.smart_copy_light_key", json!(value))
            .unwrap_or_else(|error| panic!("{value:?} must be accepted: {error:?}"));
    }
}

/// REQ: COR-011, F-065
#[test]
fn parity_011_smart_copy_light_key_refuses_meta_with_invalid_argument_and_the_loader_message() {
    let mut host = client_host();
    let error = host.settings().set("terminal.smart_copy_light_key", json!("meta")).expect_err("not allowed");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(
        error.message.contains("terminal.smart_copy_light_key = \"meta\" must be \"ctrl\", \"alt\" or \"shift\""),
        "message was {:?}",
        error.message
    );
    assert_eq!(host.settings().get("terminal.smart_copy_light_key"), Some(json!("ctrl")));
}

/// REQ: COR-011, F-065
#[test]
fn parity_011_smart_copy_light_key_refuses_an_empty_string_and_a_number() {
    let mut host = client_host();
    assert_eq!(
        host.settings().set("terminal.smart_copy_light_key", json!("")).expect_err("empty").code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        host.settings().set("terminal.smart_copy_light_key", json!(1)).expect_err("number").code,
        ErrorCode::InvalidArgument
    );
}
