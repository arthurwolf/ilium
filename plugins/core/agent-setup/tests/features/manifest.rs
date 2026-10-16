//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and every contribution
//! (command, help topic, service, three extension points, grants, activation) matches the Manifest fragment.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{Value, json};

use crate::common::{manifest_path, page_text, workspace_root};

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

fn point<'a>(manifest: &'a Value, id: &str) -> &'a Value {
    manifest["contributes"]["extension_points"]
        .as_array()
        .expect("points")
        .iter()
        .find(|point| point["id"] == id)
        .unwrap_or_else(|| panic!("point {id} missing"))
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
    assert_eq!(fragment["surface"], complete["surface"]);
    assert_eq!(fragment["replaceable"], complete["replaceable"]);
    assert_eq!(fragment["limits_tier"], complete["limits_tier"]);
    assert_eq!(fragment["capabilities"]["server"], complete["capabilities"]["server"]);
}

/// REQ: COR-050, COR-030
#[test]
fn manifest_050_the_complete_manifest_adds_only_the_kernel_tab_grant_to_the_fragment_client_grants() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    let mut expected = strings(&fragment["capabilities"]["client"]);
    expected.push("settings.write-kernel:setup".to_string());
    assert_eq!(strings(&complete["capabilities"]["client"]), expected);
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.agent-setup");
    assert_eq!(manifest["name"], "Agent setup");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(
        manifest["description"],
        "Agents work better in Ilium when their instruction files describe what Ilium offers."
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_declares_both_sides_with_their_exports() {
    let manifest = manifest();
    assert_eq!(manifest["server"]["entry"], "server.wasm");
    assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
    assert_eq!(manifest["server"]["exports"], json!(["service-provider"]));
    assert_eq!(manifest["client"]["entry"], "client.wasm");
    assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    assert_eq!(manifest["client"]["exports"], json!(["views"]));
}

/// REQ: COR-050, F-037, F-063
#[test]
fn manifest_050_server_grants_are_the_twelve_file_service_and_extension_grants_in_order() {
    let manifest = manifest();
    assert_eq!(
        strings(&manifest["capabilities"]["server"]),
        [
            "fs.read:agent-config:claude",
            "fs.write:agent-config:claude",
            "fs.read:agent-config:codex",
            "fs.write:agent-config:codex",
            "fs.read:project",
            "fs.write:project",
            "projects.read",
            "settings.read-kernel:agent-setup",
            "services.provide:ilium.core.agent-setup/install",
            "extensions.provide:ilium.core.agent-setup/instruction-blocks",
            "extensions.provide:ilium.core.agent-setup/targets",
            "extensions.provide:ilium.core.agent-setup/before-install",
        ]
    );
}

/// REQ: COR-050, COR-030
#[test]
fn manifest_050_client_grants_include_the_kernel_tab_write_grant_of_the_setup_tab() {
    let manifest = manifest();
    assert_eq!(strings(&manifest["capabilities"]["client"]), ["ui.panels", "help.read", "settings.write-kernel:setup"]);
}

/// REQ: COR-050, COR-033
#[test]
fn manifest_050_only_this_plugin_holds_the_write_grant_of_the_setup_tab() {
    let grants = strings(&manifest()["capabilities"]["client"]);
    let holders: Vec<&String> = grants.iter().filter(|grant| grant.starts_with("settings.write-kernel:")).collect();
    assert_eq!(holders, ["settings.write-kernel:setup"]);
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_activation_triggers_are_the_tab_the_command_and_the_service() {
    assert_eq!(manifest()["activation"], json!(["settings:setup", "command:refresh", "service:install"]));
}

/// REQ: COR-050
#[test]
fn manifest_050_is_replaceable_with_the_first_party_tier() {
    let manifest = manifest();
    assert_eq!(manifest["replaceable"], true);
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_surface_lists_the_service_the_three_points_and_the_command() {
    assert_eq!(
        manifest()["surface"],
        json!([
            "services:install",
            "points:instruction-blocks",
            "points:targets",
            "points:before-install",
            "commands:refresh"
        ])
    );
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_contributes_the_refresh_command() {
    assert_eq!(
        manifest()["contributes"]["commands"],
        json!([{ "id": "refresh", "title": "Refresh agent instructions", "category": "Agents" }])
    );
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_contributes_one_help_topic() {
    assert_eq!(
        manifest()["contributes"]["help"],
        json!([{ "id": "agent-setup", "file": "help/agent-setup.md", "title": "Agent setup" }])
    );
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_the_help_file_named_by_the_manifest_exists_in_the_plugin_folder() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("help/agent-setup.md");
    assert!(file.is_file(), "{file:?} is missing");
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_the_install_service_has_install_remove_and_status() {
    let manifest = manifest();
    let service = &manifest["contributes"]["services"]["ilium.core.agent-setup/install"];
    let mut methods: Vec<&String> = service.as_object().expect("service object").keys().collect();
    methods.sort();
    assert_eq!(methods, ["install", "remove", "status"]);
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_install_and_remove_require_block_and_target_strings() {
    let manifest = manifest();
    let service = &manifest["contributes"]["services"]["ilium.core.agent-setup/install"];
    for method in ["install", "remove"] {
        let params = &service[method]["params"];
        assert_eq!(params["required"], json!(["block", "target"]), "{method}");
        assert_eq!(params["properties"]["block"]["type"], "string", "{method}");
        assert_eq!(params["properties"]["target"]["type"], "string", "{method}");
        assert_eq!(service[method]["result"]["properties"]["status"]["type"], "string", "{method}");
    }
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_status_takes_an_object_and_returns_a_targets_array() {
    let manifest = manifest();
    let status = &manifest["contributes"]["services"]["ilium.core.agent-setup/install"]["status"];
    assert_eq!(status["params"]["type"], "object");
    assert_eq!(status["result"]["properties"]["targets"]["type"], "array");
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_declares_exactly_three_extension_points_in_order() {
    let manifest = manifest();
    let ids: Vec<&str> = manifest["contributes"]["extension_points"]
        .as_array()
        .expect("points")
        .iter()
        .map(|point| point["id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids, ["instruction-blocks", "targets", "before-install"]);
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_instruction_blocks_is_a_static_collect_point_of_at_most_eight_entries() {
    let manifest = manifest();
    let block = point(&manifest, "instruction-blocks");
    assert_eq!(block["kind"], "collect");
    assert_eq!(block["max_entries"], 8);
    assert_eq!(block["dynamic"], false);
    assert_eq!(block["version"], "0.1.0");
    assert_eq!(block["title"], "Instruction blocks");
    assert_eq!(block["entry_schema"]["required"], json!(["id", "title", "marker", "file"]));
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_targets_is_a_static_collect_point_of_at_most_sixteen_entries() {
    let manifest = manifest();
    let targets = point(&manifest, "targets");
    assert_eq!(targets["kind"], "collect");
    assert_eq!(targets["max_entries"], 16);
    assert_eq!(targets["dynamic"], false);
    assert_eq!(targets["title"], "Target files");
    assert_eq!(targets["entry_schema"]["required"], json!(["id", "title", "path_template", "scope"]));
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_target_scope_is_limited_to_global_and_project() {
    let manifest = manifest();
    let targets = point(&manifest, "targets");
    assert_eq!(targets["entry_schema"]["properties"]["scope"]["enum"], json!(["global", "project"]));
}

/// REQ: COR-050, F-063
#[test]
fn manifest_050_before_install_is_a_high_risk_veto_hook_with_a_twenty_millisecond_deadline() {
    let manifest = manifest();
    let hook = point(&manifest, "before-install");
    assert_eq!(hook["kind"], "hook");
    assert_eq!(hook["mode"], "veto");
    assert_eq!(hook["risk"], "high");
    assert_eq!(hook["deadline_ms"], 20);
    assert_eq!(hook["max_handlers"], 4);
    assert_eq!(hook["version"], "0.1.0");
    let mut payload: Vec<&String> = hook["payload_schema"]["properties"].as_object().expect("payload").keys().collect();
    payload.sort();
    assert_eq!(payload, ["block", "path", "target"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_a_host_built_for_the_manifest_loads_with_every_declared_grant() {
    let host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build();
    drop(host);
}

/// REQ: COR-050
#[test]
fn manifest_050_the_manifest_has_no_undeclared_top_level_key() {
    let manifest = manifest();
    let mut keys: Vec<&String> = manifest.as_object().expect("object").keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "activation", "api", "capabilities", "client", "contributes", "description", "id", "license",
            "limits_tier", "name", "publisher", "replaceable", "server", "surface", "version"
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_the_plugin_crate_is_a_member_of_the_core_plugin_folder() {
    assert!(workspace_root().join("plugins/core/agent-setup/Cargo.toml").is_file());
}
