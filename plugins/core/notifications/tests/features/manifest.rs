//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and every
//! grant and trigger it lists is the one the page describes.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::json;

use crate::common::*;

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
fn manifest_050_fragment_server_grants_equal_the_complete_manifest_server_grants() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["id"], complete["id"]);
    assert_eq!(fragment["api"], complete["api"]);
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
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Notifications");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: COR-050
#[test]
fn manifest_050_the_description_is_the_first_sentence_of_the_page() {
    let manifest = manifest();
    let description = manifest["description"].as_str().expect("description");
    assert_eq!(
        description,
        "Notifications is the plugin form of Ilium's alerts."
    );
    assert!(page_text().contains(description));
}

/// REQ: COR-050
#[test]
fn manifest_050_the_server_side_uses_the_base_server_world() {
    assert_eq!(
        manifest()["server"],
        json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0"})
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_server_grants_are_the_five_alert_grants_in_order() {
    assert_eq!(
        server_grants(),
        [
            "sound.play",
            "notify.desktop",
            "agent.read",
            "tree.read",
            "tree.attention"
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_client_grants_are_the_toast_grant_and_the_kernel_tab_grant() {
    assert_eq!(
        manifest()["capabilities"]["client"],
        json!(["notify.toast", "settings.write-kernel:sound"])
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_holds_no_grant_the_page_does_not_name() {
    let page = page_text();
    let manifest = manifest();
    for side in ["server", "client"] {
        for grant in manifest["capabilities"][side].as_array().expect("grants") {
            let grant = grant.as_str().expect("grant string");
            assert!(
                page.contains(&format!("\"{grant}\"")),
                "grant {grant} is not on the page"
            );
        }
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_activates_at_startup_with_first_party_limits() {
    let manifest = manifest();
    assert_eq!(manifest["activation"], json!(["startup"]));
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_the_boot_table_activates_notifications_at_startup() {
    let boot = manual_page("reference-kernel-and-boot.md");
    assert!(
        boot.lines().any(|line| line
            == "| Notifications | `startup` | none | Must catch the first state change |"),
        "boot table row missing"
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_has_no_unknown_top_level_key() {
    let manifest = manifest();
    let allowed = [
        "api",
        "id",
        "name",
        "version",
        "publisher",
        "description",
        "license",
        "server",
        "client",
        "capabilities",
        "activation",
        "contributes",
        "limits_tier",
    ];
    for key in manifest.as_object().expect("object").keys() {
        assert!(allowed.contains(&key.as_str()), "unexpected key {key}");
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_a_host_takes_the_server_grants_from_the_manifest() {
    let host = FakeHost::for_manifest(manifest_path())
        .expect("loads")
        .side("server")
        .build();
    drop(host);
}

/// REQ: COR-030, COR-033
#[test]
fn manifest_033_holds_the_write_grant_of_its_one_kernel_tab() {
    let manifest = manifest();
    let kernel: Vec<String> = manifest["capabilities"]["client"]
        .as_array()
        .expect("client grants")
        .iter()
        .map(|grant| grant.as_str().expect("string").to_string())
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .collect();
    assert_eq!(kernel, ["settings.write-kernel:sound"]);
}

/// REQ: COR-030
#[test]
fn manifest_030_the_kernel_tab_table_names_tab_label_and_config_tables() {
    assert!(page_text().contains("| `sound` | Sound | `[sound]`, `[notifications]` | none |"));
}

/// REQ: COR-030
#[test]
fn manifest_030_the_config_page_assigns_the_sound_tab_to_this_plugin() {
    let config = manual_page("reference-config-and-data-migration.md");
    assert!(config.contains("| `sound` | Sound | `[sound]`, `[notifications]` | Notifications | none | `[sound]`, `[notifications]` |"));
}

/// REQ: COR-022
#[test]
fn manifest_022_the_disabled_notice_is_the_documented_sentence() {
    let page = page_text();
    assert!(page.contains("\"Notifications is disabled. Values are kept.\""));
    assert!(page.contains("the tab stays in the rail with its stored values shown read-only"));
}

/// REQ: COR-022
#[test]
fn manifest_022_the_boot_table_says_what_is_off_when_disabled() {
    let boot = manual_page("reference-kernel-and-boot.md");
    assert!(
        boot.lines().any(|line| line == "| Notifications | No sound, toast or desktop notice for agents | Kernel messages still show |"),
        "disabled row missing"
    );
}
