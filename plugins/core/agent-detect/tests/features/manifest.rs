//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and every contribution
//! (signature, decoration slots, display mode) matches the Manifest fragment.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::{manifest_path, page_text};

fn manifest() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

/// The first json code block after the heading line `heading`.
fn json_block_after(page: &str, heading: &str) -> Value {
    let start = page
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
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
    let page = page_text();
    assert_eq!(manifest(), json_block_after(&page, "### Complete manifest"));
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_contributions_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["contributes"], complete["contributes"]);
    assert_eq!(fragment["server"], complete["server"]);
    assert_eq!(fragment["client"], complete["client"]);
    assert_eq!(fragment["id"], complete["id"]);
    assert_eq!(
        fragment["capabilities"]["server"],
        complete["capabilities"]["server"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.agent-detect");
    assert_eq!(manifest["name"], "Agent detect");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: COR-050
#[test]
fn manifest_050_declares_both_sides_and_the_detector_export() {
    let manifest = manifest();
    assert_eq!(manifest["server"]["entry"], "server.wasm");
    assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
    assert_eq!(manifest["server"]["exports"], json!(["detector"]));
    assert_eq!(manifest["client"]["entry"], "client.wasm");
    assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    assert!(
        manifest["client"].get("exports").is_none(),
        "the client side exports nothing"
    );
}

/// REQ: COR-050, F-026
#[test]
fn manifest_050_server_grants_are_the_four_detection_grants_in_order() {
    let manifest = manifest();
    assert_eq!(
        strings(&manifest["capabilities"]["server"]),
        [
            "agent.read",
            "agent.signatures",
            "tree.decorate",
            "panes.read"
        ]
    );
}

/// REQ: COR-050, COR-030
#[test]
fn manifest_050_client_grants_include_the_kernel_tab_write() {
    let manifest = manifest();
    assert_eq!(
        strings(&manifest["capabilities"]["client"]),
        [
            "agent.read",
            "ui.panels",
            "settings.write-kernel:agent-monitoring"
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_activates_at_startup_and_on_the_panel_view() {
    let manifest = manifest();
    assert_eq!(
        manifest["activation"],
        json!(["startup", "view:agent-detect.panel"])
    );
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050, F-030
#[test]
fn manifest_050_contributes_exactly_signatures_slots_and_display_modes() {
    let manifest = manifest();
    let mut keys: Vec<&String> = manifest["contributes"]
        .as_object()
        .expect("contributes")
        .keys()
        .collect();
    keys.sort();
    assert_eq!(keys, ["decoration_slots", "display_modes", "signatures"]);
}

/// REQ: F-026, F-030
#[test]
fn manifest_050_contributes_the_example_agent_signature() {
    let manifest = manifest();
    assert_eq!(
        manifest["contributes"]["signatures"],
        json!([{ "id": "example-agent", "process_names": ["example-agent"], "aliases": ["example"], "icon": "robot" }])
    );
}

/// REQ: F-027, F-029, F-034
#[test]
fn manifest_050_declares_the_activity_icon_slot_and_the_goal_badge_slot() {
    let manifest = manifest();
    assert_eq!(
        manifest["contributes"]["decoration_slots"],
        json!([
            { "id": "activity", "placement": "icon", "priority": 90 },
            { "id": "goal", "placement": "badge", "priority": 80 }
        ])
    );
}

/// REQ: F-030
#[test]
fn manifest_050_declares_the_attention_display_mode_that_hides_unmarked_rows() {
    let manifest = manifest();
    assert_eq!(
        manifest["contributes"]["display_modes"],
        json!([{ "id": "attention", "title": "Attention", "icon": "bell", "slot": "activity", "hide_unmarked": true }])
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_decoration_slot_priority_orders_activity_above_goal() {
    let manifest = manifest();
    let slots = manifest["contributes"]["decoration_slots"]
        .as_array()
        .expect("slots");
    let activity = slots[0]["priority"].as_i64().expect("priority");
    let goal = slots[1]["priority"].as_i64().expect("priority");
    assert!(activity > goal);
}

/// REQ: COR-050
#[test]
fn manifest_050_host_takes_grants_from_the_manifest_for_each_side() {
    // for_manifest reads id, side, grants and tier (DEV-411); building must not fail for either side.
    let _server = FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("server")
        .build();
    let _client = FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("client")
        .build();
}
