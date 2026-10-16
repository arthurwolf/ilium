//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and what the
//! page and the kernel boot table say about activation, grants and the context menu.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::{manifest_path, manual_page, page_text};

fn manifest() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

/// The first json code block after the heading line `heading`.
fn json_block_after(page: &str, heading: &str) -> Value {
    let start = page.find(heading).unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
}

fn grants(side: &str) -> Vec<String> {
    manifest()["capabilities"][side]
        .as_array()
        .unwrap_or_else(|| panic!("{side} grants"))
        .iter()
        .map(|grant| grant.as_str().expect("grant string").to_string())
        .collect()
}

/// REQ: COR-050
#[test]
fn manifest_050_equals_the_complete_manifest_of_the_page() {
    assert_eq!(manifest(), json_block_after(&page_text(), "### Complete manifest"));
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_matches_the_complete_manifest_where_it_overlaps() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["capabilities"], complete["capabilities"]);
    assert_eq!(fragment["contributes"], complete["contributes"]);
    assert_eq!(fragment["id"], complete["id"]);
    assert_eq!(fragment["api"], complete["api"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.smart-copy");
    assert_eq!(manifest["name"], "Smart copy");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: COR-050
#[test]
fn manifest_050_declares_both_sides_with_their_worlds() {
    let manifest = manifest();
    assert_eq!(manifest["server"], json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0"}));
    assert_eq!(manifest["client"], json!({"entry": "client.wasm", "world": "ilium:client/base@0.1.0"}));
}

/// REQ: COR-050, F-065
#[test]
fn manifest_050_server_reads_cells_and_the_screen_and_may_infer() {
    assert_eq!(grants("server"), ["terminal.read-cells", "terminal.read-screen", "inference"]);
}

/// REQ: COR-050, F-016, F-066
#[test]
fn manifest_050_client_reads_the_screen_claims_the_pointer_writes_the_clipboard_and_may_infer() {
    assert_eq!(
        grants("client"),
        ["terminal.read-screen", "terminal.pointer", "clipboard.write", "inference"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_server_has_no_pointer_or_clipboard_grant() {
    let server = grants("server");
    assert!(!server.iter().any(|grant| grant == "terminal.pointer"));
    assert!(!server.iter().any(|grant| grant == "clipboard.write"));
}

/// REQ: COR-050
#[test]
fn manifest_050_client_has_no_cell_grant() {
    assert!(!grants("client").iter().any(|grant| grant == "terminal.read-cells"));
}

/// REQ: COR-050, F-016
#[test]
fn manifest_050_context_menu_contributes_one_terminal_entry_for_the_smart_copy_command() {
    let manifest = manifest();
    assert_eq!(
        manifest["contributes"]["context_menu"],
        json!([{"id": "smart-copy", "title": "Smart Copy", "command": "smart-copy", "target": "terminal"}])
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_is_the_service_and_the_command_trigger_of_the_boot_table() {
    let manifest = manifest();
    assert_eq!(manifest["activation"], json!(["service:copy", "command:smart-copy"]));
    let boot = manual_page("reference-kernel-and-boot.md");
    let row = boot.lines().find(|line| line.starts_with("| Smart copy |")).expect("boot table row");
    assert!(row.contains("`service:copy`"));
    assert!(row.contains("`command:smart-copy`"));
}

/// REQ: COR-050
#[test]
fn manifest_050_limits_tier_is_first_party() {
    assert_eq!(manifest()["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_description_is_a_prefix_of_the_first_sentence_of_the_page() {
    let manifest = manifest();
    let description = manifest["description"].as_str().expect("description");
    assert!(description.starts_with("The Smart Copy plugin (ilium.core.smart-copy, side both) owns selection and copying"));
    assert!(page_text().contains(&description[..80]));
}

/// REQ: COR-011
#[test]
fn parity_011_settings_stay_kernel_settings_so_the_manifest_declares_none() {
    // Traceability F-016, F-065, F-066: the [ui] and [terminal] keys keep their config.toml names.
    let manifest = manifest();
    assert!(manifest.get("settings").is_none());
    assert!(manifest["contributes"].get("settings").is_none());
}

/// REQ: COR-050
#[test]
fn manifest_050_host_takes_grants_from_the_manifest_for_each_side() {
    let server = FakeHost::for_manifest(manifest_path()).expect("loads").side("server").build();
    let client = FakeHost::for_manifest(manifest_path()).expect("loads").side("client").build();
    drop((server, client));
}
