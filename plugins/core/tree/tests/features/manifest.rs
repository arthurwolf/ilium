//! The manifest is exact (COR-050): it equals the Complete manifest block of the page.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::Value;

use crate::common::{manifest_path, page_text};

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

/// REQ: COR-050
#[test]
fn manifest_050_equals_the_complete_manifest_of_the_page() {
    let page = page_text();
    assert_eq!(manifest(), json_block_after(&page, "### Complete manifest"));
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_capabilities_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["capabilities"], complete["capabilities"]);
    assert_eq!(fragment["id"], complete["id"]);
    assert_eq!(fragment["version"], complete["version"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.tree");
    assert_eq!(manifest["name"], "Tree");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: COR-050
#[test]
fn manifest_050_declares_both_sides_with_their_worlds() {
    let manifest = manifest();
    assert_eq!(manifest["server"]["entry"], "server.wasm");
    assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
    assert_eq!(manifest["client"]["entry"], "client.wasm");
    assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
}

/// REQ: COR-050
#[test]
fn manifest_050_server_grants_are_the_five_tree_grants_in_order() {
    let manifest = manifest();
    let server: Vec<&str> = manifest["capabilities"]["server"]
        .as_array()
        .expect("server grants")
        .iter()
        .map(|grant| grant.as_str().expect("grant string"))
        .collect();
    assert_eq!(server, ["tree.read", "tree.write", "tree.metadata", "tree.decorate", "tree.restructure"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_client_holds_only_tree_read() {
    let manifest = manifest();
    assert_eq!(manifest["capabilities"]["client"], serde_json::json!(["tree.read"]));
}

/// REQ: COR-050
#[test]
fn manifest_050_activates_at_startup_with_first_party_limits() {
    let manifest = manifest();
    assert_eq!(manifest["activation"], serde_json::json!(["startup"]));
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_description_is_the_first_sentence_prefix_of_the_page() {
    let manifest = manifest();
    let description = manifest["description"].as_str().expect("description");
    assert!(description.starts_with("The Tree plugin (ilium.core.tree, side both) owns the project tree"));
    assert!(page_text().contains(&description[..80]));
}

/// REQ: COR-050
#[test]
fn manifest_050_host_takes_grants_from_the_manifest_for_each_side() {
    // for_manifest reads id, side, grants and tier (DEV-411); building must not fail for either side.
    let server = FakeHost::for_manifest(manifest_path()).expect("loads").side("server").build();
    let client = FakeHost::for_manifest(manifest_path()).expect("loads").side("client").build();
    drop((server, client));
}
