//! The manifest is exact (COR-050): it equals the Complete manifest block of the page; sides, worlds, grants,
//! activation and tier match the page and the kernel boot table. The kernel tab has one holder (COR-030).

use serde_json::{json, Value};

use crate::manual::{json_block_after, manifest, page_text, PLUGIN_ID};

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}

/// REQ: COR-050
#[test]
fn manifest_050_equals_the_complete_manifest_of_the_page() {
    assert_eq!(
        manifest(),
        json_block_after(&page_text(), "### Complete manifest")
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_id_and_server_grants_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["id"], complete["id"]);
    let fragment_server = strings(&fragment["capabilities"]["server"]);
    let complete_server = strings(&complete["capabilities"]["server"]);
    assert_eq!(
        &complete_server[..fragment_server.len()],
        fragment_server.as_slice()
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_client_grant_is_a_prefix_of_the_complete_client_grants() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    let fragment_client = strings(&fragment["capabilities"]["client"]);
    let complete_client = strings(&complete["capabilities"]["client"]);
    assert_eq!(fragment_client, ["ui.panels"]);
    assert_eq!(
        &complete_client[..fragment_client.len()],
        fragment_client.as_slice()
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Text triggers");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: COR-050
#[test]
fn manifest_050_declares_both_sides_with_the_base_worlds() {
    let manifest = manifest();
    assert_eq!(
        manifest["server"],
        json!({ "entry": "server.wasm", "world": "ilium:server/base@0.1.0" })
    );
    assert_eq!(
        manifest["client"],
        json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0" })
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_server_grants_are_output_filter_then_terminal_write() {
    assert_eq!(
        strings(&manifest()["capabilities"]["server"]),
        ["terminal.output-filter", "terminal.write"]
    );
}

/// REQ: COR-050, COR-030
#[test]
fn manifest_050_client_grants_are_panels_then_the_kernel_tab_write() {
    assert_eq!(
        strings(&manifest()["capabilities"]["client"]),
        ["ui.panels", "settings.write-kernel:text-triggers"]
    );
}

/// REQ: COR-030
#[test]
fn manifest_030_holds_exactly_one_kernel_tab_grant_and_only_on_the_client() {
    let manifest = manifest();
    let on_server = strings(&manifest["capabilities"]["server"])
        .into_iter()
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .count();
    let on_client: Vec<String> = strings(&manifest["capabilities"]["client"])
        .into_iter()
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .collect();
    assert_eq!(on_server, 0);
    assert_eq!(on_client, ["settings.write-kernel:text-triggers"]);
}

/// REQ: COR-050, COR-022
#[test]
fn manifest_050_activation_is_startup_and_the_rules_view() {
    assert_eq!(
        strings(&manifest()["activation"]),
        ["startup", "view:text-triggers.rules"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_limits_tier_is_first_party() {
    assert_eq!(manifest()["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_has_no_grant_outside_the_two_sides() {
    let manifest = manifest();
    let capabilities = manifest["capabilities"]
        .as_object()
        .expect("capabilities object");
    let mut sides: Vec<&String> = capabilities.keys().collect();
    sides.sort();
    assert_eq!(sides, ["client", "server"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_the_output_filter_grant_has_no_pane_selection_suffix() {
    // Page "terminal.output-filter": `terminal.output-filter:agents` is invalid, the pane selection is a field.
    for grant in strings(&manifest()["capabilities"]["server"]) {
        if grant.starts_with("terminal.output-filter") {
            assert_eq!(grant, "terminal.output-filter");
        }
    }
}
