//! The manifest is exact (page "Complete manifest"): identity, sides, grants per side, activation,
//! contributions and the kernel tab the plugin owns.

use ilium_plugin_core_terminal::{TerminalClient, TerminalServer};
use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::{fenced_block_after, manifest_json, manifest_path, page_text};

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
fn cor_050_manifest_identity_fields_match_the_page() {
    let manifest = manifest_json();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.terminal");
    assert_eq!(manifest["name"], "Terminal");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: COR-050
#[test]
fn cor_050_manifest_limits_tier_is_first_party() {
    assert_eq!(manifest_json()["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn cor_050_manifest_declares_both_sides_with_their_worlds() {
    let manifest = manifest_json();
    assert_eq!(
        manifest["server"],
        json!({ "entry": "server.wasm", "world": "ilium:server/base@0.1.0" })
    );
    assert_eq!(
        manifest["client"],
        json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0" })
    );
}

/// REQ: COR-050
#[test]
fn cor_050_server_capabilities_are_exactly_the_seven_of_the_page() {
    assert_eq!(
        strings(&manifest_json()["capabilities"]["server"]),
        [
            "terminal.read-screen",
            "terminal.read-cells",
            "terminal.history",
            "terminal.write",
            "panes.read",
            "panes.signal",
            "tree.write"
        ]
    );
}

/// REQ: COR-050
#[test]
fn cor_050_client_capabilities_are_exactly_the_ones_of_the_page() {
    assert_eq!(
        strings(&manifest_json()["capabilities"]["client"]),
        [
            "terminal.read-screen",
            "tree.read",
            "editor.edit",
            "commands.invoke:ilium.core.editor/*",
            "ui.panels",
            "settings.write-kernel:terminal",
            "open-external",
            "notify.toast",
            "extensions.provide:ilium.core.terminal/open-link",
            "extensions.provide:ilium.core.terminal/link-actions"
        ]
    );
}

/// REQ: COR-050
#[test]
fn cor_050_activation_triggers_are_screen_changed_event_and_startup() {
    assert_eq!(
        strings(&manifest_json()["activation"]),
        ["event:screen-changed", "startup"]
    );
}

/// REQ: COR-050
#[test]
fn cor_050_complete_manifest_block_of_the_page_equals_the_file() {
    let block = fenced_block_after(&page_text(), "### Complete manifest", "json");
    let from_page: Value = serde_json::from_str(&block).expect("page block is JSON");
    assert_eq!(from_page, manifest_json());
}

/// REQ: COR-050
#[test]
fn cor_050_fragment_capabilities_are_a_subset_of_the_complete_manifest() {
    let fragment: Value =
        serde_json::from_str(&fenced_block_after(&page_text(), "### Manifest", "json"))
            .expect("fragment is JSON");
    let manifest = manifest_json();
    for side in ["server", "client"] {
        let complete = strings(&manifest["capabilities"][side]);
        for grant in strings(&fragment["capabilities"][side]) {
            assert!(
                complete.contains(&grant),
                "{side} grant {grant} missing from the complete manifest"
            );
        }
    }
    assert_eq!(fragment["contributes"], manifest["contributes"]);
}

/// REQ: COR-050
#[test]
fn cor_050_server_side_activates_with_the_manifest_grants() {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    let handle = host
        .try_activate::<TerminalServer>()
        .expect("activation succeeds");
    assert_eq!(handle.side, "server");
    assert_eq!(handle.instance, 1);
}

/// REQ: COR-050
#[test]
fn cor_050_client_side_activates_with_the_manifest_grants() {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    let handle = host
        .try_activate::<TerminalClient>()
        .expect("activation succeeds");
    assert_eq!(handle.side, "client");
    assert_eq!(handle.instance, 1);
}

/// REQ: COR-050
#[test]
fn cor_050_contributes_the_link_detectors_and_the_link_open_command() {
    let manifest = manifest_json();
    let contributes = manifest["contributes"]
        .as_object()
        .expect("contributes object");
    assert_eq!(
        contributes.keys().collect::<Vec<_>>(),
        [
            "commands",
            "context_menu",
            "extension_points",
            "link_detectors",
            "services",
            "topics"
        ]
    );
    // `url` comes first: detectors are tried in manifest order, and a `host:8080` inside a
    // URL also matches `path-line`.
    assert_eq!(
        manifest["contributes"]["link_detectors"],
        json!([
            { "id": "url", "regex": "https?://[^\\s]*[^\\s)\\]},.;\"'`]", "command": "link-open" },
            { "id": "path-line", "regex": "[./A-Za-z0-9_-]+\\.[a-z]+:[0-9]+", "command": "link-open" },
            { "id": "path", "regex": "[^\\s()\\[\\]{},;\"'`]*[^\\s()\\[\\]{},;\"'`.]", "command": "link-open" }
        ])
    );
    // EXT-050: a detector's command names a declared command.
    assert_eq!(
        manifest["contributes"]["commands"],
        json!([
            { "id": "link-open", "title": "Open link", "side": "client" },
            { "id": "paste-screen", "title": "Paste screen into another pane", "side": "client" }
        ])
    );
    // COR-004: the right-click entry of a terminal pane runs the declared `paste-screen` command.
    assert_eq!(
        manifest["contributes"]["context_menu"],
        json!([{
            "id": "paste-screen",
            "title": "Paste screen into another pane",
            "command": "paste-screen",
            "target": "terminal"
        }])
    );
}

/// REQ: COR-030, COR-033
#[test]
fn cor_030_only_the_client_holds_the_kernel_terminal_tab_grant() {
    let manifest = manifest_json();
    let client = strings(&manifest["capabilities"]["client"]);
    let server = strings(&manifest["capabilities"]["server"]);
    let kernel_grants: Vec<_> = client
        .iter()
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .collect();
    assert_eq!(kernel_grants, ["settings.write-kernel:terminal"]);
    assert!(server
        .iter()
        .all(|grant| !grant.starts_with("settings.write-kernel:")));
}

/// REQ: COR-033
#[test]
fn cor_033_kernel_tab_table_names_terminal_tab_table_and_readers() {
    let text = page_text();
    assert!(
        text.contains("| `terminal` | Terminal | `[terminal]` | Smart copy |"),
        "tab row missing"
    );
    assert!(text.contains("| Tab id | Label | Config table | Readers |"));
}

/// REQ: COR-022
#[test]
fn cor_022_disabled_plugin_shows_the_stored_values_notice() {
    let text = page_text();
    assert!(text.contains("\"Terminal is disabled. Values are kept.\""));
    assert!(text.contains("the tab stays in the rail with its stored values shown read-only"));
}

/// REQ: COR-030
#[test]
fn cor_030_manifest_has_no_plugin_settings_block_because_values_stay_in_the_terminal_table() {
    let manifest = manifest_json();
    assert!(manifest.get("settings").is_none());
    assert!(manifest.get("config").is_none());
    assert!(manifest["contributes"].get("settings").is_none());
}
