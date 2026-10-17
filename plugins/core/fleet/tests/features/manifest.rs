//! The manifest is exact (COR-050): it equals the Complete manifest block of the page; every
//! contributed command, route and CLI subcommand is declared (COR-040, F-107, F-109, F-110).

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::{manifest_path, page, page_text};

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

fn cli_entry(name: &str) -> Value {
    manifest()["contributes"]["cli"]
        .as_array()
        .expect("cli list")
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("cli entry {name} missing"))
        .clone()
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
fn manifest_050_fragment_capabilities_server_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(
        fragment["capabilities"]["server"],
        complete["capabilities"]["server"]
    );
    assert_eq!(fragment["id"], complete["id"]);
    assert_eq!(fragment["api"], complete["api"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_cli_entries_equal_the_complete_manifest() {
    // COR-050: every cli key in the Manifest fragment appears with the same value in the Complete manifest.
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(
        fragment["contributes"]["cli"],
        complete["contributes"]["cli"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_http_routes_and_commands_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(
        fragment["contributes"]["http_routes"],
        complete["contributes"]["http_routes"]
    );
    assert_eq!(
        fragment["contributes"]["commands"],
        complete["contributes"]["commands"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_client_grants_are_a_prefix_of_the_complete_client_grants() {
    // The fragment lists ui.panels only; the complete manifest adds the kernel tab grant.
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    let short = strings(&fragment["capabilities"]["client"]);
    let full = strings(&complete["capabilities"]["client"]);
    assert_eq!(short, ["ui.panels"]);
    assert_eq!(full[..short.len()], short[..]);
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.fleet");
    assert_eq!(manifest["name"], "Fleet");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_declares_both_sides_with_their_worlds() {
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
fn manifest_050_server_grants_are_the_nine_fleet_grants_in_order() {
    assert_eq!(
        strings(&manifest()["capabilities"]["server"]),
        [
            "cli.contribute",
            "sessions.cross",
            "http-api.route:/create_agent",
            "terminal.write",
            "panes.create",
            "panes.read",
            "agent.read",
            "messages.peer:ilium.core.notifications",
            "services.use:ilium.core.cost/usage",
        ]
    );
}

/// REQ: COR-050, COR-030
#[test]
fn manifest_030_client_holds_ui_panels_and_the_api_tab_grant() {
    assert_eq!(
        strings(&manifest()["capabilities"]["client"]),
        ["ui.panels", "settings.write-kernel:api"]
    );
}

/// REQ: COR-030, COR-033
#[test]
fn manifest_030_the_page_names_api_as_the_only_owned_kernel_tab() {
    let page = page_text();
    assert!(
        page.contains("| `api` | API | `[api]` | none |"),
        "kernel tab table row missing"
    );
    let tabs: Vec<&str> = page
        .lines()
        .filter(|line| line.starts_with("| `") && line.contains("| `["))
        .collect();
    assert_eq!(tabs.len(), 1, "exactly one kernel tab row: {tabs:?}");
}

/// REQ: COR-030
#[test]
fn manifest_030_no_other_core_page_lists_the_api_tab_as_owned() {
    // One holder per tab: the migration table names Fleet as the only owner of `api`.
    let table = page("reference-config-and-data-migration");
    let owners: Vec<&str> = table
        .lines()
        .filter(|line| line.starts_with("| `api` |"))
        .collect();
    assert_eq!(owners.len(), 1, "{owners:?}");
    assert!(owners[0].contains("Fleet"));
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_triggers_are_the_five_lazy_ones() {
    assert_eq!(
        strings(&manifest()["activation"]),
        [
            "cli:panes",
            "cli:broadcast",
            "http:/create_agent",
            "service:broadcast",
            "command:ask-for-update"
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_matches_the_kernel_and_boot_table() {
    let boot = page("reference-kernel-and-boot");
    let row = boot
        .lines()
        .find(|line| line.starts_with("| Fleet |"))
        .expect("Fleet row");
    for trigger in strings(&manifest()["activation"]) {
        assert!(
            row.contains(&format!("`{trigger}`")),
            "{trigger} missing in {row}"
        );
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_has_no_startup_activation() {
    assert!(!strings(&manifest()["activation"])
        .iter()
        .any(|trigger| trigger == "startup"));
}

/// REQ: COR-040, F-109
#[test]
fn manifest_040_panes_is_a_jsonl_command_that_needs_no_server() {
    assert_eq!(
        cli_entry("panes"),
        json!({"name": "panes", "summary": "List panes in every session", "output": "jsonl", "requires_server": false})
    );
}

/// REQ: COR-040, F-110
#[test]
fn manifest_040_broadcast_uses_the_pane_selector_argument_group() {
    assert_eq!(
        cli_entry("broadcast"),
        json!({
            "name": "broadcast",
            "summary": "Send a line to agents",
            "output": "jsonl",
            "requires_server": false,
            "argument_groups": ["pane-selector"],
        })
    );
}

/// REQ: COR-040, F-107
#[test]
fn manifest_040_new_pane_needs_a_server_and_has_mixed_output() {
    assert_eq!(
        cli_entry("new-pane"),
        json!({"name": "new-pane", "summary": "Create a pane", "output": "mixed", "requires_server": true})
    );
}

/// REQ: COR-040
#[test]
fn manifest_040_the_cli_compatibility_table_names_fleet_as_owner_of_all_three() {
    let table = page("reference-cli-compatibility");
    for command in ["ilium new-pane", "ilium panes", "ilium broadcast"] {
        let row = table
            .lines()
            .find(|line| line.starts_with(&format!("| `{command}` |")))
            .expect(command);
        assert!(row.contains("| fleet |"), "{row}");
    }
}

/// REQ: COR-040
#[test]
fn manifest_040_exactly_three_subcommands_are_declared() {
    let names: Vec<String> = manifest()["contributes"]["cli"]
        .as_array()
        .expect("cli")
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["panes", "broadcast", "new-pane"]);
}

/// REQ: COR-010
#[test]
fn manifest_010_the_plugin_registers_only_the_create_agent_route() {
    assert_eq!(
        manifest()["contributes"]["http_routes"],
        json!([{"method": "POST", "path": "/create_agent"}])
    );
    assert!(strings(&manifest()["capabilities"]["server"])
        .contains(&"http-api.route:/create_agent".to_string()));
}

/// REQ: F-113, F-036
#[test]
fn manifest_113_ask_for_update_is_a_declared_command() {
    assert_eq!(
        manifest()["contributes"]["commands"],
        json!([{"id": "ask-for-update", "title": "Ask for update"}])
    );
}

/// REQ: F-036, F-113, F-023
#[test]
fn manifest_036_the_terminal_and_pane_grants_for_messaging_are_held() {
    let server = strings(&manifest()["capabilities"]["server"]);
    for grant in ["terminal.write", "panes.read", "panes.create", "agent.read"] {
        assert!(server.contains(&grant.to_string()), "{grant} missing");
    }
}

/// REQ: F-109
#[test]
fn manifest_109_the_cost_column_needs_the_cost_usage_service_grant() {
    assert!(strings(&manifest()["capabilities"]["server"])
        .contains(&"services.use:ilium.core.cost/usage".to_string()));
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
    drop((server, client));
}
