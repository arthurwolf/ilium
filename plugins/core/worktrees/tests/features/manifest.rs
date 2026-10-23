//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and what the
//! page says about grants, the command, the keybinding and activation holds for it.

use serde_json::{json, Value};

use crate::common::{json_fences, manifest_path, manifest_value, page, page_text};

fn complete_manifest() -> Value {
    let text = page_text();
    let start = text.find("### Complete manifest").expect("heading");
    let fences = json_fences(&text[start..]);
    serde_json::from_str(&fences[0]).expect("page block is JSON")
}

fn fragment_manifest() -> Value {
    let text = page_text();
    let start = text.find("### Manifest").expect("heading");
    let fences = json_fences(&text[start..]);
    serde_json::from_str(&fences[0]).expect("fragment is JSON")
}

fn grants(side: &str) -> Vec<String> {
    manifest_value()["capabilities"][side]
        .as_array()
        .unwrap_or_else(|| panic!("{side} grants"))
        .iter()
        .map(|grant| grant.as_str().expect("grant string").to_string())
        .collect()
}

/// REQ: COR-050
#[test]
fn manifest_050_equals_the_complete_manifest_of_the_page() {
    assert_eq!(manifest_value(), complete_manifest());
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_matches_the_complete_manifest_where_it_overlaps() {
    let fragment = fragment_manifest();
    let complete = complete_manifest();
    assert_eq!(fragment["id"], complete["id"]);
    for (kind, fragment_entries) in fragment["contributes"].as_object().expect("contributes") {
        let complete_entries = complete["contributes"][kind]
            .as_array()
            .unwrap_or_else(|| panic!("complete manifest contributes {kind}"));
        for entry in fragment_entries.as_array().expect("fragment entries") {
            assert!(complete_entries.contains(entry), "{kind}: {entry}");
        }
    }
    let short = fragment["capabilities"]["server"]
        .as_array()
        .expect("fragment server grants")
        .clone();
    let long = complete["capabilities"]["server"]
        .as_array()
        .expect("complete server grants")
        .clone();
    assert_eq!(&long[..short.len()], &short[..]);
}

/// REQ: COR-050
#[test]
fn manifest_050_the_fragment_client_grants_are_a_prefix_of_the_complete_client_grants() {
    let fragment = fragment_manifest();
    let complete = complete_manifest();
    let short = fragment["capabilities"]["client"]
        .as_array()
        .expect("fragment client grants")
        .clone();
    let long = complete["capabilities"]["client"]
        .as_array()
        .expect("complete client grants")
        .clone();
    assert_eq!(&long[..short.len()], &short[..]);
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest_value();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.worktrees");
    assert_eq!(manifest["name"], "Worktrees");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_server_component_and_world() {
    assert_eq!(
        manifest_value()["server"],
        json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0"})
    );
}

/// REQ: COR-050, PKG-025
#[test]
fn manifest_050_every_side_with_grants_has_a_component_pkg_025_grant_side() {
    let manifest = manifest_value();
    for side in ["server", "client"] {
        assert!(
            manifest["capabilities"].get(side).is_some(),
            "capabilities.{side} missing"
        );
        assert!(
            manifest.get(side).is_some(),
            "capabilities.{side} is listed but the manifest has no {side} component"
        );
    }
    assert!(manifest["capabilities"].get("scene").is_none());
}

/// REQ: COR-050, F-067
#[test]
fn manifest_050_server_grants_are_the_seven_of_the_page_in_order() {
    assert_eq!(
        grants("server"),
        [
            "workspace.manage",
            "process.run:git",
            "process.run:sh",
            "panes.create",
            "panes.read",
            "fs.read:project",
            "fs.write:worktrees"
        ]
    );
}

/// REQ: COR-050, F-072
#[test]
fn manifest_050_client_grants_are_panels_and_the_git_kernel_tab() {
    assert_eq!(grants("client"), ["ui.panels", "settings.write-kernel:git"]);
}

/// REQ: COR-050, F-071
#[test]
fn manifest_050_the_only_programs_the_server_may_run_are_git_and_sh() {
    let programs: Vec<String> = grants("server")
        .into_iter()
        .filter_map(|grant| grant.strip_prefix("process.run:").map(str::to_string))
        .collect();
    assert_eq!(programs, ["git", "sh"]);
}

/// REQ: COR-050, F-071
#[test]
fn manifest_050_the_server_writes_only_the_worktrees_scope_and_reads_only_the_project_scope() {
    let server = grants("server");
    assert_eq!(
        server
            .iter()
            .filter(|grant| grant.starts_with("fs.write"))
            .collect::<Vec<_>>(),
        ["fs.write:worktrees"]
    );
    assert_eq!(
        server
            .iter()
            .filter(|grant| grant.starts_with("fs.read"))
            .collect::<Vec<_>>(),
        ["fs.read:project"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_the_client_has_no_server_only_grant() {
    let client = grants("client");
    for forbidden in ["workspace.manage", "panes.create", "panes.read"] {
        assert!(
            !client.iter().any(|grant| grant == forbidden),
            "{forbidden} on the client side"
        );
    }
}

/// REQ: COR-050, F-067
#[test]
fn manifest_050_one_command_new_agent_in_the_git_category() {
    assert_eq!(
        manifest_value()["contributes"]["commands"],
        json!([{"id": "new-agent", "title": "New agent in worktree", "category": "Git"}])
    );
}

/// REQ: COR-050, F-067
#[test]
fn manifest_050_the_keybinding_is_w_in_prefix_mode_for_the_command() {
    assert_eq!(
        manifest_value()["contributes"]["keybindings"],
        json!([{"command": "new-agent", "chord": "w", "mode": "prefix"}])
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_the_page_title_of_the_sample_names_ctrl_b_w() {
    assert!(page_text().contains("#### New agent in a worktree (Ctrl+B W)"));
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_is_the_command_and_the_create_service() {
    assert_eq!(
        manifest_value()["activation"],
        json!(["command:new-agent", "service:create"])
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_matches_the_kernel_and_boot_table_and_is_not_startup() {
    let boot = page("reference-kernel-and-boot");
    let row = boot
        .lines()
        .find(|line| line.starts_with("| Worktrees | `command:new-agent`"))
        .expect("boot row");
    let cells: Vec<&str> = row.trim_matches('|').split('|').map(str::trim).collect();
    assert_eq!(cells[1], "`command:new-agent`, `service:create`");
    assert_eq!(cells[2], "none");
    assert!(!manifest_value()["activation"]
        .as_array()
        .expect("activation")
        .iter()
        .any(|item| item == "startup"));
}

/// REQ: COR-050
#[test]
fn manifest_050_contributes_no_settings_the_git_tab_is_a_kernel_tab() {
    assert!(manifest_value()["contributes"].get("settings").is_none());
}

/// REQ: COR-050
#[test]
fn manifest_050_contributes_nothing_but_commands_and_keybindings() {
    let manifest = manifest_value();
    let keys: Vec<&String> = manifest["contributes"]
        .as_object()
        .expect("contributes")
        .keys()
        .collect();
    assert_eq!(keys, ["commands", "keybindings"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_the_manifest_file_is_where_the_testkit_reads_it() {
    assert!(manifest_path().ends_with("plugins/core/worktrees/manifest.json"));
    assert!(ilium_plugin_sdk::testing::FakeHost::for_manifest(manifest_path()).is_ok());
}

/// REQ: COR-050
#[test]
fn manifest_050_the_ability_grant_table_lists_every_server_grant_on_the_workspace_page() {
    let workspace = page("api-workspace");
    for grant in ["workspace.manage", "process.run:git", "fs.write:worktrees"] {
        assert!(
            workspace.contains(&format!("`{grant}`")),
            "{grant} missing on api-workspace"
        );
    }
    // manual does not pin which page owns panes.*, process.run:sh and fs.read:project, so those
    // are only required to be declared in the manifest.
    let declared = grants("server");
    for grant in [
        "workspace.manage",
        "process.run:git",
        "process.run:sh",
        "panes.create",
        "panes.read",
        "fs.read:project",
        "fs.write:worktrees",
    ] {
        assert!(declared.iter().any(|g| g == grant), "{grant} not declared");
    }
}
