//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and what the
//! page, the kernel boot table and the capability pages say about activation, grants and tabs.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::{
    json_block_after, manifest, manifest_path, manual_page, page_text, section, table_rows,
};

fn grants(side: &str) -> Vec<String> {
    manifest()["capabilities"][side]
        .as_array()
        .unwrap_or_else(|| panic!("{side} grants"))
        .iter()
        .map(|grant| grant.as_str().expect("grant string").to_string())
        .collect()
}

fn tab_ids_of_page() -> Vec<String> {
    let rows = table_rows(&section(&page_text(), "### Kernel tabs this plugin owns"));
    rows.into_iter().map(|row| row[0].clone()).collect()
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
fn manifest_050_fragment_server_grants_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(
        fragment["capabilities"]["server"],
        complete["capabilities"]["server"]
    );
    assert_eq!(fragment["id"], complete["id"]);
    assert_eq!(fragment["api"], complete["api"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_client_grants_are_a_prefix_of_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    let short = fragment["capabilities"]["client"]
        .as_array()
        .expect("fragment client");
    let full = complete["capabilities"]["client"]
        .as_array()
        .expect("complete client");
    assert_eq!(short.as_slice(), &full[..short.len()]);
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.inference");
    assert_eq!(manifest["name"], "Inference");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: COR-050
#[test]
fn manifest_050_server_side_world_and_entry() {
    assert_eq!(
        manifest()["server"],
        json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0"})
    );
}

/// REQ: COR-050, F-084
#[test]
fn manifest_050_server_grants_provide_inference_and_reach_two_hosts() {
    assert_eq!(
        grants("server"),
        ["inference.provide", "http:api.openai.com", "http:localhost"]
    );
}

/// REQ: COR-050, F-084, F-085
#[test]
fn manifest_050_client_grants_draw_panels_and_write_the_two_kernel_tabs() {
    assert_eq!(
        grants("client"),
        [
            "ui.panels",
            "settings.write-kernel:inference",
            "settings.write-kernel:llm-instructions"
        ]
    );
}

/// REQ: COR-050, F-084
#[test]
fn manifest_050_the_server_holds_no_bare_inference_grant() {
    // The plugin answers requests (inference.provide); it never asks the gate for a model itself.
    assert!(!grants("server").iter().any(|grant| grant == "inference"));
}

/// REQ: COR-050, F-084
#[test]
fn manifest_050_the_http_grants_are_exactly_the_openai_host_and_localhost() {
    let hosts: Vec<String> = grants("server")
        .into_iter()
        .filter_map(|grant| grant.strip_prefix("http:").map(str::to_string))
        .collect();
    assert_eq!(hosts, ["api.openai.com", "localhost"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_the_client_has_no_http_grant() {
    assert!(!grants("client")
        .iter()
        .any(|grant| grant.starts_with("http:")));
}

/// REQ: COR-030, F-085
#[test]
fn tabs_030_each_kernel_tab_has_exactly_one_write_grant_in_this_manifest() {
    let all: Vec<String> = grants("server")
        .into_iter()
        .chain(grants("client"))
        .collect();
    for tab in ["inference", "llm-instructions"] {
        let wanted = format!("settings.write-kernel:{tab}");
        assert_eq!(
            all.iter().filter(|grant| **grant == wanted).count(),
            1,
            "{tab}"
        );
    }
}

/// REQ: COR-030, F-085
#[test]
fn tabs_030_the_write_grants_equal_the_tab_ids_of_the_page_table() {
    let mut from_grants: Vec<String> = grants("client")
        .iter()
        .filter_map(|grant| {
            grant
                .strip_prefix("settings.write-kernel:")
                .map(str::to_string)
        })
        .collect();
    let mut from_page = tab_ids_of_page();
    from_grants.sort();
    from_page.sort();
    assert_eq!(from_grants, from_page);
}

/// REQ: COR-030, F-085
#[test]
fn tabs_030_the_titles_tab_is_not_held_here_although_it_shares_the_inference_table() {
    let all: Vec<String> = grants("server")
        .into_iter()
        .chain(grants("client"))
        .collect();
    assert!(!all
        .iter()
        .any(|grant| grant == "settings.write-kernel:titles"));
}

/// REQ: COR-030
#[test]
fn tabs_030_the_server_side_writes_no_kernel_tab() {
    assert!(!grants("server")
        .iter()
        .any(|grant| grant.starts_with("settings.write-kernel:")));
}

/// REQ: COR-050, COR-022
#[test]
fn manifest_050_activation_is_the_provider_service_of_the_boot_table() {
    assert_eq!(
        manifest()["activation"],
        json!(["service:ilium:inference/provider"])
    );
    let boot = manual_page("reference-kernel-and-boot.md");
    let row = boot
        .lines()
        .find(|line| line.starts_with("| Inference |"))
        .expect("boot table row");
    assert!(row.contains("`service:ilium:inference/provider`"));
    assert!(row.contains("Starts when a consumer calls it"));
}

/// REQ: COR-050
#[test]
fn manifest_050_limits_tier_is_first_party() {
    assert_eq!(manifest()["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_description_is_the_first_sentence_of_the_page() {
    let manifest = manifest();
    let description = manifest["description"].as_str().expect("description");
    assert_eq!(
        description,
        "Inference providers is the plugin form of Ilium's AI provider layer."
    );
    assert!(page_text().contains(description));
}

/// REQ: COR-011, COR-033
#[test]
fn manifest_033_settings_stay_kernel_settings_so_the_manifest_declares_none() {
    // Traceability F-084, F-085: the [inference] and [voice] keys keep their config.toml names.
    let manifest = manifest();
    assert!(manifest.get("settings").is_none());
    assert!(manifest
        .get("contributes")
        .map_or(true, |contributes| contributes.get("settings").is_none()));
}

/// REQ: COR-050, F-084
#[test]
fn manifest_provide_lists_the_inference_provider_export_on_the_server_side() {
    // capability-inference-provide.md: "Its manifest lists `inference-provider` in `exports`".
    let manifest = manifest();
    let exports = manifest["server"]["exports"]
        .as_array()
        .expect("server.exports");
    assert!(exports.iter().any(|export| export == "inference-provider"));
}

/// REQ: F-084
#[test]
fn manifest_provide_lists_the_catalog_provider_export_and_the_models_catalogue() {
    // Sample 3 of the page: contributes.catalogs [{ id: "models" }] and exports ["catalog-provider"].
    let manifest = manifest();
    let exports = manifest["server"]["exports"]
        .as_array()
        .expect("server.exports");
    assert!(exports.iter().any(|export| export == "catalog-provider"));
    let catalogs = manifest["contributes"]["catalogs"]
        .as_array()
        .expect("contributes.catalogs");
    assert!(catalogs.iter().any(|catalog| catalog["id"] == "models"));
}

/// REQ: COR-050
#[test]
fn manifest_050_host_takes_grants_from_the_manifest_for_each_side() {
    let server = FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("server")
        .build();
    let client = FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("client")
        .build();
    drop((server, client));
}

/// REQ: COR-050
#[test]
fn manifest_050_the_core_plugins_index_lists_this_plugin_on_both_sides() {
    let index = manual_page("core-plugins.md");
    let row = index
        .lines()
        .find(|line| line.contains("`ilium.core.inference`"))
        .expect("index row");
    let cells: Vec<&str> = row.trim_matches('|').split('|').map(str::trim).collect();
    assert_eq!(cells[2], "both");
}
