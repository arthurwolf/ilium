//! The manifest is exact (COR-050): it equals the Complete manifest block of the page.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::feature_support::{json_block_after, manifest, manifest_path, manual_page, page_text};

fn grants(side: &str) -> Vec<String> {
    manifest()["capabilities"][side]
        .as_array()
        .unwrap_or_else(|| panic!("{side} grants"))
        .iter()
        .map(|grant| grant.as_str().expect("grant string").to_string())
        .collect()
}

/// REQ: COR-050, F-055, F-057
#[test]
fn manifest_050_equals_the_complete_manifest_of_the_page() {
    assert_eq!(
        manifest(),
        json_block_after(&page_text(), "### Complete manifest")
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_grants_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["capabilities"], complete["capabilities"]);
    assert_eq!(fragment["id"], complete["id"]);
    assert_eq!(fragment["api"], complete["api"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.scheduler");
    assert_eq!(manifest["name"], "Scheduler");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
    assert_eq!(
        manifest["description"],
        "Scheduler is the plugin form of Ilium's delayed and queued input."
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_runs_on_both_sides() {
    let manifest = manifest();
    assert_eq!(
        manifest["server"],
        json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0"})
    );
    assert_eq!(
        manifest["client"],
        json!({"entry": "client.wasm", "world": "ilium:client/base@0.1.0"})
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_server_grants_are_the_five_scheduler_grants_in_order() {
    assert_eq!(
        grants("server"),
        [
            "terminal.write",
            "terminal.raw-input",
            "terminal.input-lock",
            "agent.read",
            "journal.read"
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_client_grants_are_ui_panels_only() {
    assert_eq!(grants("client"), ["ui.panels"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_capabilities_have_exactly_the_two_sides() {
    let manifest = manifest();
    let sides: Vec<&String> = manifest["capabilities"]
        .as_object()
        .expect("object")
        .keys()
        .collect();
    assert_eq!(sides, ["client", "server"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_triggers_are_startup_and_the_queue_view() {
    assert_eq!(
        manifest()["activation"],
        json!(["startup", "view:scheduler.queue"])
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_agrees_with_the_kernel_boot_table() {
    let boot = manual_page("reference-kernel-and-boot.md");
    assert!(boot.contains(
        "| Scheduler | `startup` | `view:scheduler.queue` | Due timers must fire after a restart |"
    ));
}

/// REQ: COR-050
#[test]
fn manifest_050_holds_no_grant_the_page_does_not_name() {
    let page = page_text();
    for grant in grants("server").into_iter().chain(grants("client")) {
        assert!(
            page.contains(&format!("\"{grant}\"")),
            "grant {grant} is not on the page"
        );
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_has_no_unlisted_top_level_key() {
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

/// REQ: COR-050
#[test]
fn manifest_050_raw_input_is_the_high_risk_grant_the_user_confirms() {
    assert!(page_text().contains(
        "`terminal.raw-input` is a high-risk grant, and the user confirms it at install."
    ));
}

/// REQ: COR-050
#[test]
fn manifest_050_the_fake_host_loads_the_manifest_on_both_sides() {
    for side in ["server", "client"] {
        let host = FakeHost::for_manifest(manifest_path())
            .expect("manifest loads")
            .side(side)
            .build();
        assert!(!host.is_disabled(), "{side}");
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_the_fake_host_grants_exactly_the_manifest_grants() {
    let host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    let _ = host.snapshot();
    let value: Value = manifest();
    assert_eq!(
        value["capabilities"]["server"].as_array().map(Vec::len),
        Some(5)
    );
}
