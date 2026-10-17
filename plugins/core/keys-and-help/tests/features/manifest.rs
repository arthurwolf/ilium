//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and every
//! grant, contribution and trigger it lists is the one the page describes.

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
fn manifest_050_fragment_contributions_equal_the_complete_manifest_contributions() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    assert_eq!(fragment["contributes"], complete["contributes"]);
    assert_eq!(fragment["id"], complete["id"]);
    assert_eq!(fragment["client"], complete["client"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_grants_are_a_prefix_of_the_complete_grants() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = json_block_after(&page, "### Complete manifest");
    let short = fragment["capabilities"]["client"]
        .as_array()
        .expect("fragment grants");
    let full = complete["capabilities"]["client"]
        .as_array()
        .expect("complete grants");
    assert_eq!(short.len(), 5);
    assert_eq!(&full[..short.len()], short.as_slice());
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Keys and help");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_the_description_is_the_first_sentence_of_the_page() {
    let manifest = manifest();
    let description = manifest["description"].as_str().expect("description");
    assert_eq!(
        description,
        "The Keys and help plugin (id ilium.core.keys-and-help, side: client) is the plugin form of Ilium's keyboard and help surfaces."
    );
    assert!(page_text().contains(description));
}

/// REQ: COR-050
#[test]
fn manifest_050_the_client_side_uses_the_base_client_world_and_exports_views() {
    assert_eq!(
        manifest()["client"],
        json!({"entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"]})
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_the_plugin_has_no_server_side() {
    assert!(manifest().get("server").is_none());
    assert!(manifest()["capabilities"].get("server").is_none());
}

/// REQ: COR-050, COR-030
#[test]
fn manifest_050_client_grants_are_the_six_keys_and_help_grants_in_order() {
    assert_eq!(
        client_grants(),
        vec![
            "keymap.read",
            "settings.read-kernel:keyboard",
            "help.read",
            "plugins.manage",
            "ui.panels",
            "settings.write-kernel:keyboard",
        ]
    );
}

/// REQ: COR-030
#[test]
fn manifest_030_the_plugin_holds_the_write_grant_for_exactly_one_kernel_tab() {
    let writes: Vec<String> = client_grants()
        .into_iter()
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .collect();
    assert_eq!(writes, vec!["settings.write-kernel:keyboard".to_string()]);
}

/// REQ: COR-030
#[test]
fn manifest_030_the_read_grants_name_one_tab_and_no_wildcard() {
    for grant in client_grants() {
        if let Some(argument) = grant.strip_prefix("settings.read-kernel:") {
            assert_eq!(argument, "keyboard");
            assert!(!argument.contains('*'));
        }
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_triggers_are_the_command_and_the_keyboard_settings_tab() {
    assert_eq!(
        manifest()["activation"],
        json!(["command:keys-and-help.open", "settings:keyboard"])
    );
}

/// REQ: F-010
#[test]
fn manifest_010_two_commands_in_the_help_category_with_the_documented_titles() {
    let commands = manifest()["contributes"]["commands"].clone();
    assert_eq!(
        commands,
        json!([
            {"id": "show-keys", "title": "Show the key reference", "category": "Help", "keywords": ["shortcut", "binding"]},
            {"id": "show-plugins", "title": "Manage plugins", "category": "Help"}
        ])
    );
}

/// REQ: F-007, F-010
#[test]
fn manifest_007_one_prefix_mode_keybinding_on_the_question_mark() {
    assert_eq!(
        manifest()["contributes"]["keybindings"],
        json!([{"command": "ilium.core.keys-and-help/show-keys", "chord": "?", "mode": "prefix"}])
    );
}

/// REQ: F-010
#[test]
fn manifest_010_the_keybinding_command_is_a_declared_command_of_this_plugin() {
    let manifest = manifest();
    let binding = manifest["contributes"]["keybindings"][0]["command"]
        .as_str()
        .expect("command");
    let local = binding
        .strip_prefix(&format!("{PLUGIN_ID}/"))
        .expect("global id of this plugin");
    let ids: Vec<&str> = manifest["contributes"]["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .map(|command| command["id"].as_str().expect("id"))
        .collect();
    assert!(ids.contains(&local));
}

/// REQ: F-010
#[test]
fn manifest_010_two_non_modal_panels_keys_and_plugins() {
    assert_eq!(
        manifest()["contributes"]["panels"],
        json!([
            {"id": "keys", "title": "Keys", "modal": false},
            {"id": "plugins", "title": "Plugins", "modal": false}
        ])
    );
}

/// REQ: F-010
#[test]
fn manifest_010_one_help_topic_keys_overview_from_the_help_folder() {
    assert_eq!(
        manifest()["contributes"]["help"],
        json!([{"id": "keys-overview", "file": "help/keys-overview.md", "title": "Keys"}])
    );
}

/// REQ: F-010
#[test]
fn manifest_010_the_help_topic_file_is_shipped_with_the_plugin() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("help/keys-overview.md");
    assert!(
        path.exists(),
        "{path:?} is the file the manifest names for the topic"
    );
}

/// REQ: F-007
#[test]
fn manifest_007_the_keyboard_tab_gets_one_client_section_at_the_end() {
    let settings = manifest()["contributes"]["settings"].clone();
    assert_eq!(settings.as_array().expect("sections").len(), 1);
    assert_eq!(settings[0]["kind"], "section");
    assert_eq!(settings[0]["id"], "chords");
    assert_eq!(settings[0]["tab"], "keyboard");
    assert_eq!(settings[0]["placement"], "end");
    assert_eq!(settings[0]["title"], "Plugin keys");
    assert_eq!(settings[0]["scope"], "client");
}

/// REQ: F-007
#[test]
fn manifest_007_the_section_holds_one_keybinding_row_for_show_keys_defaulting_to_the_question_mark()
{
    assert_eq!(
        manifest()["contributes"]["settings"][0]["rows"],
        json!([{"type": "keybinding", "key": "show_keys", "title": "Key reference", "command": "show-keys", "default": "?"}])
    );
}

/// REQ: F-007
#[test]
fn manifest_007_the_row_command_is_a_declared_local_command() {
    let manifest = manifest();
    let command = manifest["contributes"]["settings"][0]["rows"][0]["command"]
        .as_str()
        .expect("command");
    assert!(manifest["contributes"]["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .any(|entry| entry["id"] == command));
}

/// REQ: F-010
#[test]
fn manifest_010_the_manifest_declares_no_hook_handlers_and_no_extra_exports() {
    let manifest = manifest();
    assert_eq!(
        manifest["client"]["exports"]
            .as_array()
            .expect("exports")
            .len(),
        1
    );
    assert!(manifest["contributes"].get("hooks").is_none());
}

/// REQ: COR-050
#[test]
fn manifest_050_every_contribution_key_is_one_the_page_lists() {
    let contributes = manifest()["contributes"].clone();
    let mut keys: Vec<&str> = contributes
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["commands", "help", "keybindings", "panels", "settings"]
    );
}
