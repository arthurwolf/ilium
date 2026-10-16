//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and every
//! grant, contribution and trigger it lists is the one the page describes.

use serde_json::json;

use crate::common::*;

/// REQ: COR-050
#[test]
fn manifest_050_equals_the_complete_manifest_of_the_page() {
    assert_eq!(manifest(), json_block_after(&page_text(), "### Complete manifest"));
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
    let short = fragment["capabilities"]["client"].as_array().expect("fragment grants");
    let full = complete["capabilities"]["client"].as_array().expect("complete grants");
    assert_eq!(short.len(), 4);
    assert_eq!(&full[..short.len()], short.as_slice());
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_and_api_version() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Settings");
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
        "The Settings plugin (id ilium.core.settings, side: client) is the plugin form of the Settings screen."
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

/// REQ: COR-050
#[test]
fn manifest_050_client_grants_are_the_eight_settings_grants_in_order() {
    assert_eq!(
        client_grants(),
        [
            "settings.contribute:user-interface",
            "settings.read-all",
            "settings.write-all",
            "ui.panels",
            "settings.write-kernel:user-interface",
            "settings.write-kernel:icons",
            "settings.write-kernel:debug",
            "settings.write-kernel:about",
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_holds_no_grant_the_page_does_not_name() {
    let page = page_text();
    for grant in client_grants() {
        assert!(page.contains(&format!("\"{grant}\"")), "grant {grant} is not on the page");
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_activates_at_startup_with_first_party_limits() {
    let manifest = manifest();
    assert_eq!(manifest["activation"], json!(["startup"]));
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050, F-090
#[test]
fn manifest_090_appearance_section_is_placed_at_the_end_of_the_user_interface_tab() {
    let section = &manifest()["contributes"]["settings"][0];
    assert_eq!(manifest()["contributes"]["settings"].as_array().expect("settings").len(), 1);
    assert_eq!(section["kind"], "section");
    assert_eq!(section["id"], "appearance");
    assert_eq!(section["tab"], "user-interface");
    assert_eq!(section["placement"], "end");
    assert_eq!(section["title"], "Appearance");
    assert_eq!(section["scope"], "client");
}

/// REQ: COR-050, F-090
#[test]
fn manifest_090_density_row_is_a_choice_defaulting_to_comfortable() {
    let row = &manifest()["contributes"]["settings"][0]["rows"][0];
    assert_eq!(row["type"], "choice");
    assert_eq!(row["key"], "density");
    assert_eq!(row["title"], "Density");
    assert_eq!(row["default"], "comfortable");
    assert_eq!(
        row["options"],
        json!([
            {"value": "compact", "title": "Compact"},
            {"value": "comfortable", "title": "Comfortable"}
        ])
    );
}

/// REQ: COR-050, F-092
#[test]
fn manifest_092_accent_row_is_a_theme_swatch_colour_defaulting_to_4a9eff() {
    let row = &manifest()["contributes"]["settings"][0]["rows"][1];
    assert_eq!(row["type"], "color");
    assert_eq!(row["key"], "accent");
    assert_eq!(row["title"], "Accent colour");
    assert_eq!(row["swatches"], "theme");
    assert_eq!(row["default"], "#4a9eff");
    assert_eq!(manifest()["contributes"]["settings"][0]["rows"].as_array().expect("rows").len(), 2);
}

/// REQ: COR-050, F-089
#[test]
fn manifest_089_declares_one_onboarding_step_first_run_with_order_10() {
    assert_eq!(
        manifest()["contributes"]["onboarding_steps"],
        json!([{"id": "first-run", "title": "Welcome", "order": 10}])
    );
}

/// REQ: COR-050, F-090
#[test]
fn manifest_090_declares_the_non_modal_overview_panel_and_holds_ui_panels() {
    assert_eq!(
        manifest()["contributes"]["panels"],
        json!([{"id": "overview", "title": "All settings", "modal": false}])
    );
    assert!(client_grants().contains(&"ui.panels".to_string()));
}

/// REQ: COR-050
#[test]
fn manifest_050_contributes_nothing_beyond_settings_onboarding_steps_and_panels() {
    let manifest = manifest();
    let mut keys: Vec<&String> = manifest["contributes"].as_object().expect("contributes").keys().collect();
    keys.sort();
    assert_eq!(keys, ["onboarding_steps", "panels", "settings"]);
}

/// REQ: COR-050, F-090
#[test]
fn manifest_090_the_host_accepts_the_manifest_and_activates_the_client_side() {
    let mut host = client_host();
    host.settle();
    assert!(!host.is_disabled());
    assert!(host.failures().is_empty());
}
