//! The manifest is exact (page "Complete manifest", COR-050) and every contribution is declared.

use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

fn page_manifest() -> Value {
    let blocks = json_blocks(&page_text());
    // blocks[0] is the fragment, blocks[1] the complete manifest, blocks[2] the extension example.
    serde_json::from_str(&blocks[1]).expect("complete manifest on the page is JSON")
}

fn fragment() -> Value {
    serde_json::from_str(&json_blocks(&page_text())[0]).expect("fragment is JSON")
}

/// REQ: COR-050, F-009
#[test]
fn doc_cor_050_manifest_file_equals_complete_manifest_on_page() {
    assert_eq!(manifest_value(), page_manifest());
}

/// REQ: COR-050, F-009
#[test]
fn doc_cor_050_page_has_fragment_complete_manifest_and_example_blocks() {
    assert_eq!(
        json_blocks(&page_text()).len(),
        3,
        "fragment, complete manifest and the issues example"
    );
}

/// REQ: COR-050, F-009
#[test]
fn doc_cor_050_fragment_contributions_equal_the_complete_manifest() {
    let fragment = fragment();
    let complete = page_manifest();
    for key in [
        "api",
        "id",
        "version",
        "publisher",
        "server",
        "client",
        "capabilities",
        "contributes",
    ] {
        assert_eq!(fragment[key], complete[key], "fragment key {key}");
    }
}

/// REQ: COR-050, F-009
#[test]
fn doc_cor_050_complete_manifest_carries_the_required_keys() {
    let manifest = manifest_value();
    for key in [
        "api",
        "id",
        "name",
        "version",
        "publisher",
        "license",
        "description",
        "limits_tier",
    ] {
        assert!(manifest.get(key).is_some(), "missing key {key}");
    }
}

/// REQ: F-009
#[test]
fn search_009_identity_is_ilium_core_search_at_api_0_1_0() {
    let manifest = manifest_value();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Search");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: F-009
#[test]
fn search_009_limits_tier_is_first_party() {
    assert_eq!(manifest_value()["limits_tier"], "first-party");
}

/// REQ: F-009
#[test]
fn search_009_has_a_server_side_with_the_job_runner_export() {
    let manifest = manifest_value();
    assert_eq!(manifest["server"]["entry"], "server.wasm");
    assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
    assert_eq!(manifest["server"]["exports"], json!(["job-runner"]));
}

/// REQ: F-009
#[test]
fn search_009_has_a_client_side_with_the_views_export() {
    let manifest = manifest_value();
    assert_eq!(manifest["client"]["entry"], "client.wasm");
    assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    assert_eq!(manifest["client"]["exports"], json!(["views"]));
}

/// REQ: F-009
#[test]
fn search_009_server_grants_are_exactly_the_five_of_the_page_in_order() {
    assert_eq!(
        strings(&manifest_value()["capabilities"]["server"]),
        [
            "inference.embed",
            "tree.read",
            "terminal.read-screen",
            "fs.read:project",
            "jobs.run"
        ]
    );
}

/// REQ: F-009
#[test]
fn search_009_client_grants_are_ui_panels_only() {
    assert_eq!(
        strings(&manifest_value()["capabilities"]["client"]),
        ["ui.panels"]
    );
}

/// REQ: F-009
#[test]
fn search_009_reads_terminal_screens_through_the_read_screen_grant() {
    assert!(strings(&manifest_value()["capabilities"]["server"])
        .contains(&"terminal.read-screen".to_string()));
    assert!(page_text().contains("`terminal.read-screen` grant"));
}

/// REQ: F-009
#[test]
fn search_009_activation_triggers_are_command_job_and_view() {
    assert_eq!(
        strings(&manifest_value()["activation"]),
        ["command:search", "job:index", "view:search.panel"]
    );
}

/// REQ: F-009
#[test]
fn search_009_contributes_one_open_command() {
    let commands = manifest_value()["contributes"]["commands"].clone();
    assert_eq!(
        commands,
        json!([{ "id": "open", "title": "Search the workspace", "category": "Search" }])
    );
}

/// REQ: F-009
#[test]
fn search_009_binds_the_slash_chord_in_prefix_mode_to_the_open_command() {
    let keybindings = manifest_value()["contributes"]["keybindings"].clone();
    assert_eq!(
        keybindings,
        json!([{ "command": "ilium.core.search/open", "chord": "/", "mode": "prefix" }])
    );
}

/// REQ: F-009
#[test]
fn search_009_contributes_one_modal_search_panel() {
    let panels = manifest_value()["contributes"]["panels"].clone();
    assert_eq!(
        panels,
        json!([{ "id": "search", "title": "Search", "modal": true }])
    );
}

/// REQ: F-009
#[test]
fn search_009_contributes_the_scan_files_job_with_its_limits() {
    let jobs = manifest_value()["contributes"]["jobs"].clone();
    assert_eq!(
        jobs,
        json!([{ "kind": "scan-files", "deadline_ms": 120000, "memory_mib": 128 }])
    );
}

/// REQ: F-009
#[test]
fn search_009_contributes_only_commands_keybindings_panels_and_jobs() {
    let manifest = manifest_value();
    let mut keys: Vec<&String> = manifest["contributes"]
        .as_object()
        .expect("contributes")
        .keys()
        .collect();
    keys.sort();
    assert_eq!(keys, ["commands", "jobs", "keybindings", "panels"]);
}

/// REQ: F-009
#[test]
fn search_009_the_keybinding_command_is_a_contributed_command_of_the_plugin() {
    let manifest = manifest_value();
    let command = manifest["contributes"]["keybindings"][0]["command"]
        .as_str()
        .expect("command")
        .to_string();
    let id = manifest["contributes"]["commands"][0]["id"]
        .as_str()
        .expect("id");
    assert_eq!(command, format!("{PLUGIN_ID}/{id}"));
}

/// REQ: F-009
#[test]
fn search_009_the_job_kind_has_a_server_job_runner_export() {
    let manifest = manifest_value();
    assert!(strings(&manifest["server"]["exports"]).contains(&"job-runner".to_string()));
    assert!(strings(&manifest["capabilities"]["server"]).contains(&"jobs.run".to_string()));
}

/// REQ: F-009
#[test]
fn search_009_the_fake_host_loads_the_manifest_without_error() {
    assert!(ilium_plugin_sdk::testing::FakeHost::for_manifest(manifest_path()).is_ok());
}

/// REQ: F-009
#[test]
fn search_009_the_page_names_both_sides_in_its_first_paragraph() {
    assert!(page_text().contains("(id `ilium.core.search`, sides: server and client)"));
}

/// REQ: F-009
#[test]
fn search_009_the_search_example_uses_the_documented_manifest_for_the_issues_source() {
    let blocks = json_blocks(&page_text());
    let example: Value = serde_json::from_str(&blocks[2]).expect("example JSON");
    assert_eq!(
        example["capabilities"]["server"],
        json!(["extensions.contribute:ilium.core.search/sources"])
    );
    assert_eq!(
        example["contributes"]["extensions"][0]["point"],
        "ilium.core.search/sources"
    );
}
