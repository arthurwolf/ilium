//! The manifest is exact (page "Complete manifest"): file, page and kernel-and-boot agree.

use serde_json::json;

use crate::common::*;

/// REQ: COR-050
#[test]
fn manifest_050_file_equals_the_complete_manifest_of_the_page() {
    assert_eq!(manifest_value(), page_manifest());
}

/// REQ: COR-050
#[test]
fn manifest_050_required_keys_are_present() {
    let manifest = manifest_value();
    for key in [
        "api",
        "id",
        "name",
        "version",
        "publisher",
        "description",
        "license",
    ] {
        assert!(manifest.get(key).is_some(), "missing key {key}");
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_values() {
    let manifest = manifest_value();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Sessions");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: COR-050
#[test]
fn manifest_050_description_is_the_first_sentence_of_the_page() {
    let manifest = manifest_value();
    assert_eq!(
        manifest["description"],
        "A session is one project's tree of groups and panes together with the processes that run in it."
    );
    assert!(page_text().contains(manifest["description"].as_str().expect("description")));
}

/// REQ: COR-050
#[test]
fn manifest_050_server_side_world_and_entry() {
    let manifest = manifest_value();
    assert_eq!(
        manifest["server"],
        json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0"})
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_client_side_world_and_entry() {
    let manifest = manifest_value();
    assert_eq!(
        manifest["client"],
        json!({"entry": "client.wasm", "world": "ilium:client/base@0.1.0"})
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_server_grants_in_page_order() {
    let manifest = manifest_value();
    assert_eq!(
        strings(&manifest["capabilities"]["server"]),
        [
            "panes.read",
            "panes.lifecycle",
            "tree.restructure",
            "agent.read",
            "settings.write-kernel:session",
            "extensions.provide:ilium.core.sessions/snapshot-sections",
            "extensions.provide:ilium.core.sessions/before-restore"
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_grants_equal_the_complete_manifest_grants() {
    let blocks = json_blocks(&page_text());
    let fragment: serde_json::Value = serde_json::from_str(&blocks[0]).expect("fragment is JSON");
    assert_eq!(fragment["id"], PLUGIN_ID);
    // The fragment lists the contributions; the complete manifest adds the kernel-tab and
    // extension-point grants after them (page "Manifest" and "Complete manifest").
    let fragment_grants = strings(&fragment["capabilities"]["server"]);
    let complete_grants = strings(&manifest_value()["capabilities"]["server"]);
    assert_eq!(complete_grants[..fragment_grants.len()], fragment_grants[..]);
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_is_startup_and_the_list_view() {
    let manifest = manifest_value();
    assert_eq!(
        strings(&manifest["activation"]),
        ["startup", "view:sessions.list"]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_matches_the_kernel_and_boot_table() {
    let row = table_row(&page("reference-kernel-and-boot"), "Sessions");
    assert_eq!(row[1], "`startup`");
    assert_eq!(row[2], "`view:sessions.list`");
    assert_eq!(row[3], "Offers the snapshot sections to the kernel writer");
}

/// REQ: COR-050
#[test]
fn manifest_050_limits_tier_is_first_party() {
    assert_eq!(manifest_value()["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_core_plugins_index_lists_sessions_as_server_side() {
    let row = table_row(
        &page("core-plugins"),
        "[Sessions](/pages/core-plugin-sessions/)",
    );
    assert_eq!(row[1], "`ilium.core.sessions`");
    assert_eq!(row[2], "server");
    assert_eq!(
        row[3],
        "Snapshots, restore, recovery and detach and reattach."
    );
}

/// REQ: COR-030
#[test]
fn manifest_030_client_holds_the_session_kernel_tab_grant() {
    let manifest = manifest_value();
    // Page: the server side holds the grant, the client side needs no grants of its own.
    assert!(manifest["capabilities"].get("client").is_none());
    assert!(strings(&manifest["capabilities"]["server"])
        .iter()
        .any(|grant| grant == "settings.write-kernel:session"));
}

/// REQ: COR-030
#[test]
fn manifest_030_only_the_session_tab_is_written_by_this_plugin() {
    let manifest = manifest_value();
    let all: Vec<String> = ["server", "client"]
        .iter()
        .filter(|side| manifest["capabilities"].get(**side).is_some())
        .flat_map(|side| strings(&manifest["capabilities"][side]))
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .collect();
    assert_eq!(all, ["settings.write-kernel:session"]);
    assert!(manifest["capabilities"].get("client").is_none());
}
