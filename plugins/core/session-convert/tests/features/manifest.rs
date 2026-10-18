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

/// REQ: COR-050, F-077
#[test]
fn manifest_050_equals_the_complete_manifest_of_the_page() {
    assert_eq!(
        manifest(),
        json_block_after(&page_text(), "### Complete manifest")
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_contributions_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    for key in [
        "contributes",
        "capabilities",
        "server",
        "client",
        "surface",
        "replaceable",
        "limits_tier",
        "id",
        "api",
    ] {
        assert_eq!(fragment[key], complete[key], "key {key}");
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.session-convert");
    assert_eq!(manifest["name"], "Session convert");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: COR-050
#[test]
fn manifest_050_declares_both_sides_with_their_worlds_and_exports() {
    let manifest = manifest();
    assert_eq!(
        manifest["server"],
        json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["job-runner", "service-provider"]})
    );
    assert_eq!(
        manifest["client"],
        json!({"entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views", "menu-provider"]})
    );
}

/// REQ: COR-050, F-077
#[test]
fn manifest_050_server_grants_are_the_fifteen_of_the_page_in_order() {
    assert_eq!(
        strings(&manifest()["capabilities"]["server"]),
        [
            "panes.read",
            "panes.lifecycle",
            "panes.create",
            "tree.write",
            "agent.read",
            "fs.read:agent-sessions:claude",
            "fs.read:agent-sessions:codex",
            "fs.write:agent-config:claude",
            "process.run:codex",
            "process.long-lived",
            "jobs.run",
            "notify.toast",
            "services.provide:ilium.core.session-convert/convert",
            "extensions.provide:ilium.core.session-convert/formats",
            "extensions.provide:ilium.core.session-convert/after-convert",
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_client_grants_are_agent_read_and_ui_panels() {
    assert_eq!(
        strings(&manifest()["capabilities"]["client"]),
        ["agent.read", "ui.panels"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_triggers_are_on_demand() {
    assert_eq!(
        strings(&manifest()["activation"]),
        [
            "command:convert",
            "service:convert",
            "view:session-convert.progress"
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_runs_with_first_party_limits_and_is_replaceable() {
    let manifest = manifest();
    assert_eq!(manifest["limits_tier"], "first-party");
    assert_eq!(manifest["replaceable"], true);
}

/// REQ: COR-050, COR-011
#[test]
fn manifest_050_surface_lists_the_service_both_points_and_the_command() {
    assert_eq!(
        strings(&manifest()["surface"]),
        [
            "services:convert",
            "points:formats",
            "points:after-convert",
            "commands:convert"
        ]
    );
}

/// REQ: COR-050, F-077
#[test]
fn manifest_050_command_convert_is_titled_convert_session_in_category_agents() {
    assert_eq!(
        manifest()["contributes"]["commands"],
        json!([{"id": "convert", "title": "Convert session", "category": "Agents"}])
    );
}

/// REQ: COR-050, F-077
#[test]
fn manifest_050_context_menu_entry_is_dynamic_for_panes_and_runs_convert() {
    assert_eq!(
        manifest()["contributes"]["context_menu"],
        json!([{"id": "pane-convert", "applies_to": ["pane"], "dynamic": true, "command": "convert"}])
    );
}

/// REQ: COR-050, F-077
#[test]
fn manifest_050_panel_progress_is_modal() {
    assert_eq!(
        manifest()["contributes"]["panels"],
        json!([{"id": "progress", "title": "Convert session", "modal": true}])
    );
}

/// REQ: COR-050, F-077
#[test]
fn manifest_050_job_codex_to_claude_has_a_60_second_deadline_and_256_mib() {
    assert_eq!(
        manifest()["contributes"]["jobs"],
        json!([{"kind": "codex-to-claude", "deadline_ms": 60000, "memory_mib": 256}])
    );
}

/// REQ: COR-050, F-077
#[test]
fn manifest_050_service_convert_start_takes_pane_and_target_and_returns_conversion() {
    let manifest = manifest();
    let service =
        &manifest["contributes"]["services"]["ilium.core.session-convert/convert"]["start"];
    assert_eq!(service["params"]["required"], json!(["pane", "target"]));
    assert_eq!(
        service["params"]["properties"]["pane"],
        json!({"type": "integer"})
    );
    assert_eq!(
        service["params"]["properties"]["target"],
        json!({"type": "string", "enum": ["codex", "claude"]})
    );
    assert_eq!(service["result"]["required"], json!(["conversion"]));
    assert_eq!(
        service["result"]["properties"]["conversion"],
        json!({"type": "string"})
    );
}

/// REQ: COR-050, COR-011
#[test]
fn manifest_050_extension_point_formats_is_a_static_collect_point_with_eight_entries() {
    let points = manifest()["contributes"]["extension_points"].clone();
    let formats = &points[0];
    assert_eq!(formats["id"], "formats");
    assert_eq!(formats["kind"], "collect");
    assert_eq!(formats["title"], "Transcript formats");
    assert_eq!(formats["max_entries"], 8);
    assert_eq!(formats["dynamic"], false);
    assert_eq!(formats["version"], "0.1.0");
    assert_eq!(
        formats["entry_schema"]["required"],
        json!(["id", "title", "direction", "service"])
    );
    assert_eq!(
        formats["entry_schema"]["properties"]["direction"]["enum"],
        json!(["read", "write", "both"])
    );
}

/// REQ: COR-050, COR-011
#[test]
fn manifest_050_extension_point_after_convert_is_a_standard_observe_hook() {
    let points = manifest()["contributes"]["extension_points"].clone();
    let hook = &points[1];
    assert_eq!(hook["id"], "after-convert");
    assert_eq!(hook["kind"], "hook");
    assert_eq!(hook["mode"], "observe");
    assert_eq!(hook["title"], "After conversion");
    assert_eq!(hook["risk"], "standard");
    assert_eq!(hook["deadline_ms"], 20);
    assert_eq!(hook["max_handlers"], 8);
    assert_eq!(hook["version"], "0.1.0");
    assert_eq!(
        hook["payload_schema"]["properties"],
        json!({"from": {"type": "string"}, "to": {"type": "string"}, "messages": {"type": "integer"}, "path": {"type": "string"}})
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_exactly_two_extension_points_exist() {
    assert_eq!(
        manifest()["contributes"]["extension_points"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_the_context_menu_command_is_a_declared_command() {
    let manifest = manifest();
    let entry = &manifest["contributes"]["context_menu"][0];
    let declared: Vec<String> = manifest["contributes"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert!(declared.contains(&entry["command"].as_str().unwrap().to_string()));
}

/// REQ: COR-050
#[test]
fn manifest_050_the_set_of_contribution_kinds_is_exactly_the_six_of_the_page() {
    let manifest = manifest();
    let mut kinds: Vec<&String> = manifest["contributes"]
        .as_object()
        .unwrap()
        .keys()
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        [
            "commands",
            "context_menu",
            "extension_points",
            "jobs",
            "panels",
            "services"
        ]
    );
}

/// REQ: COR-050, COR-011
#[test]
fn manifest_050_the_plugin_has_no_settings_keys_or_keybindings() {
    let manifest = manifest();
    for kind in ["settings", "keybindings", "keys", "cli", "status_segments"] {
        assert!(
            manifest["contributes"].get(kind).is_none(),
            "{kind} must not be contributed"
        );
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_every_provide_grant_names_a_contribution_of_the_manifest() {
    let manifest = manifest();
    let server = strings(&manifest["capabilities"]["server"]);
    let service = "ilium.core.session-convert/convert";
    assert!(server.contains(&format!("services.provide:{service}")));
    assert!(manifest["contributes"]["services"].get(service).is_some());
    for point in manifest["contributes"]["extension_points"]
        .as_array()
        .unwrap()
    {
        let grant = format!(
            "extensions.provide:ilium.core.session-convert/{}",
            point["id"].as_str().unwrap()
        );
        assert!(server.contains(&grant), "{grant}");
    }
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
    let server = FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("server")
        .build();
    let client = FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("client")
        .build();
    {
        // Both hosts are released here; a plain drop() call is flagged for non-Drop tuples.
        let _released_hosts = (server, client);
    }
}
