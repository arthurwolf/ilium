//! API settings tab (COR-022, COR-030): the tab is contributed through the manifest as one
//! number row bound to the kernel `[api]` table, so the port, its boundaries and its default
//! live in the manifest and the host draws and validates the page.

use serde_json::Value;

use crate::common::manifest_path;

fn manifest() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

fn tab() -> Value {
    let manifest = manifest();
    let tabs = manifest["contributes"]["settings"]
        .as_array()
        .expect("settings array");
    assert_eq!(tabs.len(), 1, "fleet contributes exactly one settings tab");
    tabs[0].clone()
}

fn port_row() -> Value {
    let tab = tab();
    let rows = tab["sections"][0]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1, "the API tab has exactly one row");
    rows[0].clone()
}

/// REQ: COR-030
#[test]
fn settings_030_the_api_tab_is_a_kernel_tab_contributed_by_the_manifest() {
    let tab = tab();
    assert_eq!(tab["kind"], "tab");
    assert_eq!(tab["id"], "api");
    assert_eq!(tab["title"], "API");
}

/// REQ: COR-030
#[test]
fn settings_030_the_default_port_is_8872() {
    assert_eq!(port_row()["default"], 8872);
}

/// REQ: COR-030
#[test]
fn settings_030_port_one_and_65535_are_the_accepted_boundaries() {
    let row = port_row();
    assert_eq!(row["type"], "number");
    assert_eq!(row["minimum"], 1);
    assert_eq!(row["maximum"], 65535);
}

/// REQ: COR-030
#[test]
fn settings_030_the_api_tab_shows_the_port_row() {
    assert_eq!(port_row()["title"], "HTTP API port");
}

/// REQ: COR-022
#[test]
fn settings_022_the_row_edits_the_kernel_api_table_so_a_disabled_fleet_keeps_the_values() {
    let row = port_row();
    assert_eq!(row["kernel_tab"], "api");
    assert_eq!(row["kernel_key"], "port");
    let manifest = manifest();
    let grants: Vec<&str> = manifest["capabilities"]["server"]
        .as_array()
        .expect("server grants")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(grants.contains(&"settings.write-kernel:api"));
}
