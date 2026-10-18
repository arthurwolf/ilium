//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and every contribution
//! and grant matches the Manifest fragment.

use serde_json::{json, Value};

use crate::common::{manifest_path, page_text};

fn manifest() -> Value {
    serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("manifest.json readable"))
        .expect("JSON")
}

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
    assert_eq!(
        manifest(),
        json_block_after(&page_text(), "### Complete manifest")
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_grants_are_a_subset_of_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["id"], complete["id"]);
    for side in ["server", "client"] {
        let all = strings(&complete["capabilities"][side]);
        for grant in strings(&fragment["capabilities"][side]) {
            assert!(all.contains(&grant), "{side}: {grant}");
        }
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.chatroom");
    assert_eq!(manifest["name"], "Chatroom");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_both_sides_declare_their_entry_and_world() {
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
fn manifest_050_server_grants_are_exactly_the_eight_of_the_page() {
    assert_eq!(
        strings(&manifest()["capabilities"]["server"]),
        [
            "tree.decorate",
            "tree.write",
            "fs.read:project",
            "fs.write:project",
            "fs.watch:project",
            "topics.subscribe:chatroom",
            "services.use:ilium.core.agent-setup",
            "cli.contribute",
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_client_grants_are_topics_subscribe_and_ui_panels() {
    assert_eq!(
        strings(&manifest()["capabilities"]["client"]),
        ["topics.subscribe:chatroom", "ui.panels"]
    );
}

/// REQ: COR-050, F-064
#[test]
fn manifest_050_the_plugin_holds_no_agent_config_grant() {
    let manifest = manifest();
    for side in ["server", "client"] {
        assert!(strings(&manifest["capabilities"][side])
            .iter()
            .all(|grant| !grant.starts_with("agent-config")));
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_triggers_are_startup_and_topic_chatroom() {
    assert_eq!(
        strings(&manifest()["activation"]),
        ["startup", "topic:chatroom"]
    );
}

/// REQ: COR-050, F-062
#[test]
fn manifest_050_the_cli_contribution_is_chat_with_text_output_no_stdin_and_no_server() {
    assert_eq!(
        manifest()["contributes"]["cli"],
        json!([{
            "name": "chat",
            "summary": "Project chatroom: init, send, context, tail",
            "output": "text",
            "stdin": "none",
            "requires_server": false
        }])
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_the_room_scope_is_project_for_read_write_and_watch() {
    let server = strings(&manifest()["capabilities"]["server"]);
    for grant in ["fs.read:project", "fs.write:project", "fs.watch:project"] {
        assert!(server.iter().any(|candidate| candidate == grant), "{grant}");
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_the_description_is_the_first_sentence_of_the_page() {
    let description = manifest()["description"]
        .as_str()
        .expect("description")
        .to_string();
    assert!(page_text().contains(&description));
}

/// REQ: COR-050
#[test]
fn manifest_050_the_cargo_package_is_the_core_chatroom_crate() {
    let cargo = std::fs::read_to_string(manifest_path().parent().expect("dir").join("Cargo.toml"))
        .expect("Cargo.toml");
    assert!(cargo.contains("name = \"ilium-plugin-core-chatroom\""));
}
