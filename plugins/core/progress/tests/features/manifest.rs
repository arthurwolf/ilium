//! The manifest is exact (COR-050): it equals the Complete manifest block of the page; the CLI
//! contribution declares `progress` and its alias `wait` (COR-040, F-060, F-108).

use serde_json::json;

use crate::common::*;
use crate::feature_support::*;

/// REQ: COR-050, F-059
#[test]
fn manifest_050_equals_the_complete_manifest_of_the_page() {
    assert_eq!(
        manifest_value(),
        json_block_after(&page_text(), "### Complete manifest")
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_capabilities_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["id"], complete["id"]);
    for side in ["server", "client"] {
        let fragment_grants = fragment["capabilities"][side].as_array().expect("array");
        let complete_grants = complete["capabilities"][side].as_array().expect("array");
        assert!(complete_grants.starts_with(fragment_grants));
    }
}

/// REQ: COR-050, COR-040
#[test]
fn manifest_050_fragment_cli_entry_equals_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(
        fragment["contributes"]["cli"],
        complete["contributes"]["cli"]
    );
}

/// REQ: COR-040, F-060
#[test]
fn manifest_040_declares_the_progress_subcommand() {
    let cli = &manifest_value()["contributes"]["cli"][0];
    assert_eq!(cli["name"], "progress");
    assert_eq!(cli["summary"], "Progress monitors");
    assert_eq!(cli["output"], "jsonl");
    assert_eq!(cli["requires_server"], true);
}

/// REQ: COR-040, F-108
#[test]
fn manifest_040_declares_wait_as_the_only_alias() {
    let cli = &manifest_value()["contributes"]["cli"][0];
    assert_eq!(cli["aliases"], json!(["wait"]));
}

/// REQ: COR-050
#[test]
fn manifest_050_contributes_only_a_cli_entry() {
    // The plugin owns no tab, no command, no keybinding and no route (page: "The plugin owns no tab").
    let manifest = manifest_value();
    let keys: Vec<&String> = manifest["contributes"]
        .as_object()
        .expect("contributes")
        .keys()
        .collect();
    assert_eq!(keys, ["cli"]);
}

/// REQ: COR-050, F-059
#[test]
fn manifest_050_server_grants_are_exactly_the_five_of_the_page() {
    assert_eq!(
        strings(&manifest_value()["capabilities"]["server"]),
        [
            "cli.contribute",
            "process.run:sh",
            "process.long-lived",
            "http-api.route:/progress.json",
            "settings.read-kernel:agent-monitoring"
        ]
    );
}

/// REQ: COR-050, F-061
#[test]
fn manifest_050_client_grants_are_exactly_the_two_of_the_page() {
    assert_eq!(
        strings(&manifest_value()["capabilities"]["client"]),
        ["ui.chrome", "settings.read-kernel:agent-monitoring"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_has_no_write_grant_for_the_kernel_tab() {
    // "this plugin never writes them" (page, Settings it reads).
    let manifest = manifest_value();
    for side in ["server", "client"] {
        for grant in strings(&manifest["capabilities"][side]) {
            assert!(
                !grant.starts_with("settings.write-kernel"),
                "{side}: {grant}"
            );
        }
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_worlds_activation_and_tier() {
    let manifest = manifest_value();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Progress");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(
        manifest["server"],
        json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0"})
    );
    assert_eq!(
        manifest["client"],
        json!({"entry": "client.wasm", "world": "ilium:client/base@0.1.0"})
    );
    assert_eq!(manifest["activation"], json!(["startup"]));
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_description_is_the_first_sentence_of_the_page() {
    let manifest = manifest_value();
    assert_eq!(
        manifest["description"],
        "Progress monitors let a long task show how far it is."
    );
    assert!(page_text().contains("Progress monitors let a long task show how far it is."));
}

/// REQ: F-059, F-061
#[test]
fn manifest_059_both_sides_load_and_activate_with_the_manifest_grants() {
    let mut server = server_host();
    server.activate::<ilium_plugin_core_progress::ProgressServer>();
    let mut client = client_host();
    client.activate::<ilium_plugin_core_progress::ProgressClient>();
    assert!(!server.is_disabled());
    assert!(!client.is_disabled());
}

/// REQ: COR-011
#[test]
fn manifest_011_the_plugin_runs_on_both_sides() {
    let manifest = manifest_value();
    assert!(manifest.get("server").is_some());
    assert!(manifest.get("client").is_some());
}
