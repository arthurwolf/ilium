//! API settings tab (COR-022, COR-030): `api.port`, its boundaries, the tab render and the
//! disabled notice. Key name and tab id come from the page ownership table.

use serde_json::json;
use ilium_plugin_sdk::types::{ErrorCode, ViewTarget};

use crate::common::*;

fn tab() -> ViewTarget {
    ViewTarget::SettingsPage("api".to_string())
}

/// REQ: COR-030
#[test]
fn settings_030_the_default_port_is_8872() {
    let mut host = client_host();
    assert_eq!(host.settings().get("api.port"), Some(json!(8872)));
}

/// REQ: COR-030
#[test]
fn settings_030_port_one_and_65535_are_accepted() {
    for port in [1u32, 65535] {
        let mut host = client_host();
        assert!(host.settings().set("api.port", json!(port)).is_ok(), "{port}");
        assert_eq!(host.settings().get("api.port"), Some(json!(port)));
    }
}

/// REQ: COR-030
#[test]
fn settings_030_port_zero_is_rejected_and_the_old_value_stays() {
    let mut host = client_host();
    let error = host.settings().set("api.port", json!(0)).expect_err("0 rejected");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.settings().get("api.port"), Some(json!(8872)));
}

/// REQ: COR-030
#[test]
fn settings_030_port_65536_is_rejected() {
    let mut host = client_host();
    let error = host.settings().set("api.port", json!(65536)).expect_err("65536 rejected");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: COR-030
#[test]
fn settings_030_a_negative_or_text_port_is_rejected() {
    for value in [json!(-1), json!("8872"), json!(1.5), json!(null)] {
        let mut host = client_host();
        assert!(host.settings().set("api.port", value.clone()).is_err(), "{value}");
    }
}

/// REQ: COR-030
#[test]
fn settings_030_the_api_tab_shows_the_port_row() {
    let mut host = client_host();
    let text = host.render(tab(), 80, 24).text();
    assert!(text.contains("HTTP API port"), "{text}");
    assert!(text.contains("8872"), "{text}");
}

/// REQ: COR-030
#[test]
fn settings_030_the_tab_fits_the_documented_widths() {
    for columns in [40u16, 80, 120] {
        let mut host = client_host();
        let text = host.render(tab(), columns, 24).text();
        assert!(text.lines().all(|l| l.chars().count() <= usize::from(columns)), "{columns}");
        assert!(text.contains("HTTP API port"), "{columns}");
    }
}

/// REQ: COR-022
#[test]
fn settings_022_a_disabled_fleet_shows_the_notice_and_keeps_the_values() {
    let mut host = client_host();
    host.settings().set("api.port", json!(9000)).expect("set");
    host.disable_plugin();
    let text = host.render(tab(), 80, 24).text();
    assert!(text.contains("Fleet is disabled. Values are kept."), "{text}");
    assert_eq!(host.settings().get("api.port"), Some(json!(9000)));
}
