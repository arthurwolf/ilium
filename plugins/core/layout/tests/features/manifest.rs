//! The manifest is exact (page "Complete manifest", COR-050).

use serde_json::{json, Value};

use crate::common::*;

fn page_manifest() -> Value {
    let blocks = json_blocks(&page_text());
    // blocks[0] is the fragment, blocks[1] the complete manifest, later blocks are samples.
    serde_json::from_str(&blocks[1]).expect("complete manifest on the page is JSON")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}

/// REQ: COR-050, F-003
#[test]
fn doc_cor_050_manifest_file_equals_complete_manifest_on_page() {
    assert_eq!(manifest_value(), page_manifest());
}

/// REQ: COR-050, F-003
#[test]
fn doc_cor_050_fragment_fields_match_complete_manifest() {
    let blocks = json_blocks(&page_text());
    let fragment: Value = serde_json::from_str(&blocks[0]).expect("fragment is JSON");
    let complete = page_manifest();
    for key in ["api", "id", "name", "version", "publisher", "capabilities"] {
        assert_eq!(fragment[key], complete[key], "fragment key {key}");
    }
}

/// REQ: COR-050, F-003
#[test]
fn doc_cor_050_identity_fields_are_exact() {
    let manifest = manifest_value();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.layout");
    assert_eq!(manifest["name"], "Layout");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050, F-003
#[test]
fn doc_cor_050_both_sides_have_entry_and_world() {
    let manifest = manifest_value();
    assert_eq!(
        manifest["server"],
        json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0"})
    );
    assert_eq!(
        manifest["client"],
        json!({"entry": "client.wasm", "world": "ilium:client/base@0.1.0"})
    );
}

/// REQ: COR-050, F-003
#[test]
fn doc_cor_050_server_capabilities_are_panes_read_panes_lifecycle_panes_create_tree_restructure() {
    let manifest = manifest_value();
    assert_eq!(
        strings(&manifest["capabilities"]["server"]),
        [
            "panes.read",
            "panes.lifecycle",
            "panes.create",
            "tree.restructure"
        ]
    );
}

/// REQ: COR-050, F-003
#[test]
fn doc_cor_050_client_capabilities_are_panes_read_ui_panels() {
    let manifest = manifest_value();
    assert_eq!(
        strings(&manifest["capabilities"]["client"]),
        ["panes.read", "ui.panels"]
    );
}

/// REQ: COR-050, F-003
#[test]
fn doc_cor_050_activation_triggers_are_command_and_view() {
    let manifest = manifest_value();
    assert_eq!(
        strings(&manifest["activation"]),
        ["command:layout.preset", "view:layout.presets"]
    );
}

/// REQ: COR-050, F-003
#[test]
fn doc_cor_050_manifest_declares_no_settings_because_layout_keys_stay_in_the_kernel() {
    // F-003 config keys are the kernel [ui] left_panel_* keys, not plugin settings.
    let manifest = manifest_value();
    assert!(manifest.get("settings").is_none());
    assert!(manifest["capabilities"].get("scene").is_none());
}

/// REQ: F-003
#[test]
fn f_003_server_host_holds_declared_server_grants_only() {
    let mut host = server_host();
    assert!(host.grant_now("tree.restructure").is_ok());
    assert!(host.grant_now("panes.lifecycle").is_ok());
    let refused = host
        .grant_now("ui.panels")
        .expect_err("ui.panels is a client grant");
    assert_eq!(refused.code, "harness.grant-not-declared");
}

/// REQ: F-003
#[test]
fn f_003_client_host_holds_declared_client_grants_only() {
    let mut host = client_host();
    assert!(host.grant_now("ui.panels").is_ok());
    assert!(host.grant_now("panes.read").is_ok());
    let refused = host
        .grant_now("tree.restructure")
        .expect_err("tree.restructure is a server grant");
    assert_eq!(refused.code, "harness.grant-not-declared");
}

/// REQ: F-003
#[test]
fn f_003_server_activation_succeeds_with_the_manifest_grants() {
    let mut host = server_host();
    let result = host.try_activate::<ilium_plugin_core_layout::LayoutServer>();
    assert!(result.is_ok(), "{:?}", result.err());
    for grant in ["panes.read", "panes.lifecycle", "tree.restructure"] {
        assert!(host.grant_now(grant).is_ok(), "{grant} is declared");
    }
    let refused = host
        .grant_now("ui.panels")
        .expect_err("ui.panels is a client grant");
    assert_eq!(refused.code, "harness.grant-not-declared");
}

/// REQ: F-003
#[test]
fn f_003_client_activation_succeeds_with_the_manifest_grants() {
    let mut host = client_host();
    let result = host.try_activate::<ilium_plugin_core_layout::LayoutClient>();
    assert!(result.is_ok(), "{:?}", result.err());
    for grant in ["panes.read", "ui.panels"] {
        assert!(host.grant_now(grant).is_ok(), "{grant} is declared");
    }
    let refused = host
        .grant_now("tree.restructure")
        .expect_err("tree.restructure is a server grant");
    assert_eq!(refused.code, "harness.grant-not-declared");
}
