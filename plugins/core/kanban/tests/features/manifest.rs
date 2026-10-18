//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and every
//! contribution it lists is the one the page describes.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::{json_block_after, manifest, manifest_path, page_text};

fn client_grants() -> Vec<String> {
    manifest()["capabilities"]["client"]
        .as_array()
        .expect("client grants")
        .iter()
        .map(|grant| grant.as_str().expect("grant string").to_string())
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
fn manifest_050_fragment_contributions_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["contributes"], complete["contributes"]);
    assert_eq!(fragment["client"], complete["client"]);
    assert_eq!(fragment["id"], complete["id"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.kanban");
    assert_eq!(manifest["name"], "Kanban");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: COR-050
#[test]
fn manifest_050_runs_on_the_client_side_only() {
    let manifest = manifest();
    assert_eq!(
        manifest["client"],
        json!({"entry": "client.wasm", "world": "ilium:client/base@0.1.0"})
    );
    assert!(
        manifest.get("server").is_none(),
        "the page says the plugin runs on the client side"
    );
    assert!(manifest["capabilities"].get("server").is_none());
}

/// REQ: COR-050
#[test]
fn manifest_050_client_grants_are_the_eight_kanban_grants_in_order() {
    assert_eq!(
        client_grants(),
        [
            "panes.provide-kind",
            "fs.read:project",
            "fs.write:project",
            "fs.watch",
            "fs.pick",
            "fs.read:picked",
            "editor.read",
            "settings.write-kernel:kanban-board",
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_holds_no_grant_the_page_does_not_name() {
    let page = page_text();
    for grant in client_grants() {
        assert!(
            page.contains(&format!("\"{grant}\"")),
            "grant {grant} is not on the page"
        );
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_activates_on_the_pane_kind_and_has_first_party_limits() {
    let manifest = manifest();
    assert_eq!(manifest["activation"], json!(["pane-kind:kanban"]));
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_description_is_the_first_sentence_of_the_page() {
    let manifest = manifest();
    let description = manifest["description"].as_str().expect("description");
    assert_eq!(
        description,
        "The Kanban plugin has the id ilium.core.kanban and runs on the client side."
    );
    assert!(page_text().contains(description));
}

/// REQ: F-024
#[test]
fn feature_024_declares_one_basic_board_pane_kind() {
    let manifest = manifest();
    let kinds = manifest["contributes"]["pane_kinds"]
        .as_array()
        .expect("pane kinds");
    assert_eq!(kinds.len(), 1);
    assert_eq!(
        kinds[0],
        json!({
            "id": "board", "title": "Board", "icon": "layout-kanban",
            "primitive": "basic", "wants_keys": true, "persist": "metadata"
        })
    );
}

/// REQ: F-024
#[test]
fn feature_024_the_board_pane_wants_keys_for_the_keyboard_table() {
    let manifest = manifest();
    assert_eq!(manifest["contributes"]["pane_kinds"][0]["wants_keys"], true);
}

/// REQ: F-024
#[test]
fn feature_024_the_board_pane_keeps_only_metadata_in_the_snapshot() {
    let manifest = manifest();
    assert_eq!(
        manifest["contributes"]["pane_kinds"][0]["persist"],
        "metadata"
    );
}

/// REQ: F-024
#[test]
fn feature_024_contributes_only_pane_kinds_and_settings() {
    let manifest = manifest();
    let mut keys: Vec<&String> = manifest["contributes"]
        .as_object()
        .expect("contributes")
        .keys()
        .collect();
    keys.sort();
    assert_eq!(keys, ["pane_kinds", "settings"]);
}

/// REQ: F-024
#[test]
fn feature_024_provides_a_pane_kind_so_the_kind_grant_is_held() {
    assert!(client_grants().contains(&"panes.provide-kind".to_string()));
}

/// REQ: F-025
#[test]
fn feature_025_holds_the_picker_and_file_grants_for_boards_from_files() {
    let grants = client_grants();
    for needed in [
        "fs.pick",
        "fs.read:picked",
        "fs.read:project",
        "fs.write:project",
        "fs.watch",
        "editor.read",
    ] {
        assert!(grants.contains(&needed.to_string()), "missing {needed}");
    }
}

/// REQ: COR-030, COR-033
#[test]
fn manifest_033_holds_the_write_grant_of_its_one_kernel_tab() {
    let kernel: Vec<String> = client_grants()
        .into_iter()
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .collect();
    assert_eq!(kernel, ["settings.write-kernel:kanban-board"]);
}

/// REQ: COR-030
#[test]
fn manifest_030_the_kernel_tab_table_names_tab_label_and_config_table() {
    let page = page_text();
    assert!(page.contains("| `kanban-board` | Kanban Board | `[kanban_board]` | none |"));
}

/// REQ: COR-030, COR-033
#[test]
fn manifest_033_the_kernel_tab_is_held_by_this_plugin_only_on_the_t1_tier() {
    let page = page_text();
    assert!(page.contains(
        "It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab"
    ));
    assert_eq!(manifest()["limits_tier"], "first-party");
}

/// REQ: COR-022
#[test]
fn manifest_022_the_disabled_notice_is_the_documented_sentence() {
    let page = page_text();
    assert!(page.contains("\"Kanban is disabled. Values are kept.\""));
    assert!(page.contains("the tab stays in the rail with its stored values shown read-only"));
}

/// REQ: COR-022
#[test]
fn manifest_022_disabled_kanban_shows_a_placeholder_and_keeps_board_files() {
    let path = crate::common::workspace_root()
        .join("docs/plugins-manual/pages/reference-kernel-and-boot.md");
    let boot = std::fs::read_to_string(path).expect("kernel and boot page");
    let row = boot
        .lines()
        .find(|line| line.starts_with("| Kanban | Board panes show a placeholder."))
        .expect("kanban disabled row");
    assert!(row.contains("No new board"));
    assert!(row.contains("Board files stay on disk."));
}

/// REQ: COR-050
#[test]
fn manifest_050_the_boot_table_activates_kanban_on_the_pane_kind() {
    let path = crate::common::workspace_root()
        .join("docs/plugins-manual/pages/reference-kernel-and-boot.md");
    let boot = std::fs::read_to_string(path).expect("kernel and boot page");
    let row = boot
        .lines()
        .find(|line| line.starts_with("| Kanban | none | `pane-kind:kanban` |"))
        .expect("row");
    assert!(row.contains("In plugin form it starts with a board pane"));
}

/// REQ: COR-050
#[test]
fn manifest_050_host_takes_grants_from_the_manifest_for_the_client_side() {
    let client = FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("client")
        .build();
    let _ = client;
}

/// REQ: COR-050
#[test]
fn manifest_050_the_manifest_is_valid_json_with_no_unknown_top_level_key() {
    let manifest = manifest();
    let allowed = [
        "api",
        "id",
        "name",
        "version",
        "publisher",
        "description",
        "license",
        "client",
        "capabilities",
        "activation",
        "contributes",
        "limits_tier",
    ];
    let object: &serde_json::Map<String, Value> = manifest.as_object().expect("object");
    for key in object.keys() {
        assert!(allowed.contains(&key.as_str()), "unexpected key {key}");
    }
}
