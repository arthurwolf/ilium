//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and every contribution
//! (cli command, grants, activation triggers) matches the page.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::manifest_path;
use crate::manual::{json_block_after, page_text};

fn manifest() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

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
fn manifest_050_fragment_agrees_with_the_complete_manifest_on_id_and_server_grants() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["id"], complete["id"]);
    assert_eq!(fragment["api"], complete["api"]);
    assert_eq!(
        fragment["capabilities"]["server"],
        complete["capabilities"]["server"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_client_grants_are_a_prefix_of_the_complete_client_grants() {
    let page = page_text();
    let fragment = strings(&json_block_after(&page, "### Manifest")["capabilities"]["client"]);
    let complete =
        strings(&json_block_after(&page, "### Complete manifest")["capabilities"]["client"]);
    assert_eq!(fragment, complete[..fragment.len()]);
    assert_eq!(
        complete.last().map(String::as_str),
        Some("settings.write-kernel:voice-control")
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.voice");
    assert_eq!(manifest["name"], "Voice");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_declares_both_sides_with_the_base_worlds_and_no_exports() {
    let manifest = manifest();
    assert_eq!(manifest["server"]["entry"], "server.wasm");
    assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
    assert_eq!(manifest["client"]["entry"], "client.wasm");
    assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    assert!(
        manifest["server"].get("exports").is_none(),
        "the server side exports nothing"
    );
    assert!(
        manifest["client"].get("exports").is_none(),
        "the client side exports nothing"
    );
}

/// REQ: COR-050, F-052
#[test]
fn manifest_050_server_grant_is_only_cli_contribute() {
    assert_eq!(
        strings(&manifest()["capabilities"]["server"]),
        ["cli.contribute"]
    );
}

/// REQ: COR-050, F-050
#[test]
fn manifest_050_client_grants_are_the_six_voice_grants_in_order() {
    assert_eq!(
        strings(&manifest()["capabilities"]["client"]),
        [
            "audio.microphone",
            "keys.intercept",
            "commands.invoke:*",
            "inference",
            "jobs.run",
            "settings.write-kernel:voice-control"
        ]
    );
}

/// REQ: COR-050, COR-030
#[test]
fn manifest_050_holds_the_kernel_write_grant_of_exactly_one_tab() {
    let grants = strings(&manifest()["capabilities"]["client"]);
    let kernel: Vec<&String> = grants
        .iter()
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .collect();
    assert_eq!(kernel, ["settings.write-kernel:voice-control"]);
}

/// REQ: COR-050, F-050
#[test]
fn manifest_050_activation_is_the_toggle_command_and_the_cli_trigger_in_order() {
    assert_eq!(
        manifest()["activation"],
        json!(["command:voice.toggle", "cli:voice"])
    );
}

/// REQ: COR-050, F-052
#[test]
fn manifest_050_contributes_one_jsonl_cli_command_that_needs_a_server() {
    let manifest = manifest();
    assert_eq!(
        manifest["contributes"],
        json!({ "cli": { "name": "voice", "summary": "Voice control: say", "output": "jsonl", "requires_server": true } })
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_contributes_no_settings_page_because_the_tab_is_a_kernel_tab() {
    let manifest = manifest();
    assert!(manifest["contributes"].get("settings").is_none());
    assert!(
        manifest["contributes"].get("keybindings").is_none(),
        "F8 is a kernel keybinding of voice.toggle"
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_has_no_key_outside_the_documented_set() {
    let manifest = manifest();
    let mut keys: Vec<&String> = manifest.as_object().expect("object").keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "activation",
            "api",
            "capabilities",
            "client",
            "contributes",
            "description",
            "id",
            "license",
            "limits_tier",
            "name",
            "publisher",
            "server",
            "version"
        ]
    );
}

/// REQ: COR-050, F-050
#[test]
fn manifest_050_loads_into_a_fake_host_for_both_sides() {
    assert!(FakeHost::for_manifest(manifest_path()).is_ok());
    let _server = FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("server")
        .build();
    let _client = FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("client")
        .build();
}
