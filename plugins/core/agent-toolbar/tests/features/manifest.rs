//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and every contribution is as the page lists it.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

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

fn strings(value: &Value) -> Vec<String> {
    value.as_array().expect("array").iter().map(|item| item.as_str().expect("string").to_string()).collect()
}

/// REQ: COR-050
#[test]
fn manifest_050_equals_the_complete_manifest_of_the_page() {
    assert_eq!(manifest(), json_block_after(&page_text(), "### Complete manifest"));
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_contributions_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["contributes"], complete["contributes"]);
    assert_eq!(fragment["capabilities"], complete["capabilities"]);
    assert_eq!(fragment["server"], complete["server"]);
    assert_eq!(fragment["client"], complete["client"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.agent-toolbar");
    assert_eq!(manifest["name"], "Agent toolbar");
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

/// REQ: COR-050
#[test]
fn manifest_050_server_grants_are_the_five_agent_grants_in_order() {
    let manifest = manifest();
    assert_eq!(
        strings(&manifest["capabilities"]["server"]),
        ["agent.read", "agent.read-prompts", "agent.transcript", "agent.control", "journal.read"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_client_grants_are_read_read_prompts_and_ui_chrome() {
    let manifest = manifest();
    assert_eq!(strings(&manifest["capabilities"]["client"]), ["agent.read", "agent.read-prompts", "ui.chrome"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_triggers_are_agent_state_event_then_startup() {
    assert_eq!(strings(&manifest()["activation"]), ["event:agent-state-changed", "startup"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_runs_with_first_party_limits() {
    assert_eq!(manifest()["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_commands_are_compact_and_history_with_their_titles() {
    assert_eq!(
        manifest()["contributes"]["commands"],
        json!([
            {"id": "compact", "title": "Compact agent context"},
            {"id": "history", "title": "Show agent debug history"}
        ])
    );
}

/// REQ: COR-050, F-035
#[test]
fn manifest_050_tree_toolbar_has_one_compact_button() {
    assert_eq!(
        manifest()["contributes"]["tree_toolbar"],
        json!([{"id": "compact", "icon": "arrows-minimize", "label": "Compact", "command": "compact"}])
    );
}

/// REQ: COR-050, F-035
#[test]
fn manifest_050_row_action_history_applies_to_agent_rows_at_order_200() {
    assert_eq!(
        manifest()["contributes"]["row_actions"],
        json!([{"id": "history", "icon": "history", "label": "Debug history", "command": "history", "applies_to": ["agent"], "order": 200}])
    );
}

/// REQ: COR-050, F-033
#[test]
fn manifest_050_frame_slots_are_prompt_banner_model_header_text_and_recover_footer_row() {
    assert_eq!(
        manifest()["contributes"]["pane_frame_slots"],
        json!([
            {"id": "prompt", "placement": "banner", "applies_to": "agents"},
            {"id": "model", "placement": "header-text", "applies_to": "agents"},
            {"id": "recover", "placement": "footer-row", "applies_to": "agents"}
        ])
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_every_toolbar_and_row_command_is_a_declared_command() {
    let manifest = manifest();
    let declared: Vec<String> =
        manifest["contributes"]["commands"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    for group in ["tree_toolbar", "row_actions"] {
        for entry in manifest["contributes"][group].as_array().unwrap() {
            assert!(declared.contains(&entry["command"].as_str().unwrap().to_string()), "{group}: {entry}");
        }
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_the_set_of_contribution_kinds_is_exactly_the_four_of_the_page() {
    let manifest = manifest();
    let mut kinds: Vec<&String> = manifest["contributes"].as_object().unwrap().keys().collect();
    kinds.sort();
    assert_eq!(kinds, ["commands", "pane_frame_slots", "row_actions", "tree_toolbar"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_description_is_the_first_sentence_of_the_page() {
    let manifest = manifest();
    let description = manifest["description"].as_str().expect("description");
    assert!(page_text().contains(description));
}

/// REQ: COR-050
#[test]
fn manifest_050_host_takes_grants_from_the_manifest_for_each_side() {
    let server = FakeHost::for_manifest(manifest_path()).expect("loads").side("server").build();
    let client = FakeHost::for_manifest(manifest_path()).expect("loads").side("client").build();
    drop((server, client));
    let page = json_block_after(&page_text(), "### Complete manifest");
    let manifest = manifest();
    assert_eq!(
        strings(&manifest["capabilities"]["server"]),
        ["agent.read", "agent.read-prompts", "agent.transcript", "agent.control", "journal.read"]
    );
    assert_eq!(strings(&manifest["capabilities"]["client"]), ["agent.read", "agent.read-prompts", "ui.chrome"]);
    assert_eq!(manifest["capabilities"], page["capabilities"]);
}

/// REQ: F-033, F-035
#[test]
fn doc_f_033_the_traceability_grants_of_the_owned_features_are_in_the_manifest() {
    let manifest = manifest();
    let mut granted = strings(&manifest["capabilities"]["server"]);
    granted.extend(strings(&manifest["capabilities"]["client"]));
    for grant in ["agent.transcript", "ui.chrome"] {
        assert!(granted.iter().any(|g| g == grant), "{grant} missing");
    }
}
