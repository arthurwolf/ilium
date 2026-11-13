//! The manifest against the page (COR-050): identity, side, world, grants, contributions, rows.

use serde_json::{json, Value};

use crate::common::*;

/// REQ: COR-050
#[test]
fn manifest_050_whole_manifest_equals_the_complete_manifest_block_of_the_page() {
    assert_eq!(manifest_value(), complete_manifest_on_page());
}

/// REQ: COR-050
#[test]
fn manifest_050_page_has_the_fragment_and_the_complete_block_in_that_order() {
    let blocks = json_blocks(&page_text());
    assert!(
        blocks.len() >= 3,
        "fragment, complete manifest, extension example"
    );
    let fragment: Value = serde_json::from_str(&blocks[0]).expect("fragment");
    let complete: Value = serde_json::from_str(&blocks[1]).expect("complete");
    assert!(fragment.get("description").is_none());
    assert!(complete.get("description").is_some());
}

/// REQ: COR-050
#[test]
fn manifest_050_every_fragment_key_has_the_same_value_in_the_complete_manifest() {
    let fragment = fragment_on_page();
    let complete = complete_manifest_on_page();
    for key in [
        "api",
        "id",
        "name",
        "version",
        "publisher",
        "client",
        "contributes",
    ] {
        assert_eq!(fragment[key], complete[key], "{key}");
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_grants_are_a_prefix_of_the_complete_grants() {
    let fragment = strings(&fragment_on_page()["capabilities"]["client"]);
    let complete = strings(&complete_manifest_on_page()["capabilities"]["client"]);
    assert_eq!(fragment, complete[..fragment.len()].to_vec());
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_fields_match_the_page() {
    let manifest = manifest_value();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Ambient look");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_description_states_id_side_and_purpose() {
    let description = manifest_value()["description"]
        .as_str()
        .expect("description")
        .to_string();
    assert_eq!(
        description,
        "The Ambient look plugin (id ilium.core.ambient, side: client) is the plugin form of the shared look of animated backgrounds."
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_the_plugin_has_a_client_side_and_no_other_side() {
    let manifest = manifest_value();
    assert!(manifest.get("client").is_some());
    assert!(manifest.get("server").is_none());
    assert!(manifest.get("scene").is_none());
    assert!(manifest["capabilities"].get("server").is_none());
}

/// REQ: COR-050
#[test]
fn manifest_050_client_side_uses_the_base_world_and_exports_only_views() {
    let client = &manifest_value()["client"];
    assert_eq!(client["entry"], "client.wasm");
    assert_eq!(client["world"], "ilium:client/base@0.1.0");
    assert_eq!(client["exports"], json!(["views"]));
}

/// REQ: COR-050, F-093, F-094
#[test]
fn manifest_050_client_grants_are_exactly_the_four_of_the_page_in_order() {
    let grants = strings(&manifest_value()["capabilities"]["client"]);
    assert_eq!(
        grants,
        [
            "location.geocode",
            "media.video",
            "ui.panels",
            "settings.write-kernel:animations"
        ]
    );
}

/// REQ: COR-050, COR-030
#[test]
fn manifest_050_holds_write_kernel_for_exactly_one_tab_the_animations_tab() {
    let grants = strings(&manifest_value()["capabilities"]["client"]);
    let kernel: Vec<&String> = grants
        .iter()
        .filter(|grant| grant.starts_with("settings.write-kernel:"))
        .collect();
    assert_eq!(kernel, ["settings.write-kernel:animations"]);
}

/// REQ: COR-050, F-093
#[test]
fn manifest_050_activation_is_the_startup_trigger_only() {
    assert_eq!(manifest_value()["activation"], json!(["startup"]));
}

/// REQ: COR-050, F-093
#[test]
fn manifest_050_contributes_exactly_panels_and_settings() {
    let manifest = manifest_value();
    let mut keys: Vec<&String> = manifest["contributes"]
        .as_object()
        .expect("object")
        .keys()
        .collect();
    keys.sort();
    assert_eq!(keys, ["panels", "settings"]);
}

/// REQ: COR-050, F-103
#[test]
fn manifest_050_one_non_modal_panel_gallery_titled_scenes() {
    let panels = manifest_value()["contributes"]["panels"].clone();
    assert_eq!(
        panels,
        json!([{ "id": "gallery", "title": "Scenes", "modal": false }])
    );
}

/// REQ: COR-050, F-093
#[test]
fn manifest_050_one_settings_tab_animations_with_one_client_section_look() {
    let settings = manifest_value()["contributes"]["settings"].clone();
    assert_eq!(settings.as_array().expect("array").len(), 1);
    let tab = &settings[0];
    assert_eq!(tab["kind"], "tab");
    assert_eq!(tab["id"], TAB);
    assert_eq!(tab["title"], "Animations");
    let sections = tab["sections"].as_array().expect("sections");
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0]["id"], "look");
    assert_eq!(sections[0]["title"], "Look");
    assert_eq!(sections[0]["scope"], "client");
}

/// REQ: COR-050, F-093, F-103
#[test]
fn manifest_050_rows_are_choice_preview_toggle_location_map_in_that_order() {
    let types: Vec<String> = manifest_rows()
        .iter()
        .map(|row| row["type"].as_str().expect("type").to_string())
        .collect();
    assert_eq!(types, ["choice", "preview", "toggle", "location", "map"]);
}

/// REQ: COR-050, COR-011, F-093
#[test]
fn manifest_050_value_row_keys_are_scene_semantic_observer_and_saved_places() {
    let keys: Vec<String> = manifest_rows()
        .iter()
        .filter_map(|row| row.get("key").and_then(Value::as_str).map(str::to_string))
        .collect();
    assert_eq!(keys, ["scene", "semantic", "observer", "saved_places"]);
}

/// REQ: COR-011, F-093
#[test]
fn manifest_011_scene_row_is_a_choice_defaulting_to_stars_with_stars_and_clouds() {
    let scene = row("scene");
    assert_eq!(scene["type"], "choice");
    assert_eq!(scene["title"], "Scene");
    assert_eq!(scene["default"], "stars");
    assert_eq!(
        scene["options"],
        json!([{ "value": "stars", "title": "Stars" }, { "value": "clouds", "title": "Clouds" }])
    );
}

/// A choice row has 2 to 64 options (reference-settings-rows.md, row 2).
///
/// REQ: COR-011
#[test]
fn manifest_011_scene_options_are_within_the_two_to_sixty_four_limit() {
    let count = row("scene")["options"].as_array().expect("options").len();
    assert!((2..=64).contains(&count));
}

/// REQ: COR-011
#[test]
fn manifest_011_scene_default_is_one_of_its_options() {
    let scene = row("scene");
    let values: Vec<&str> = scene["options"]
        .as_array()
        .expect("options")
        .iter()
        .map(|option| option["value"].as_str().expect("v"))
        .collect();
    assert_eq!(scene["default"], "stars");
    assert!(values.contains(&"stars"));
}

/// REQ: F-093, F-094
#[test]
fn manifest_093_preview_row_is_live_four_rows_high_and_names_the_scene_preview_view() {
    let preview = manifest_rows()
        .into_iter()
        .find(|row| row["type"] == "preview")
        .expect("preview");
    assert_eq!(
        preview,
        json!({ "type": "preview", "view": "scene-preview", "height": 4, "live": true })
    );
}

/// A preview row height is 4 to 24 rows (reference-settings-rows.md, row 14).
///
/// REQ: F-093
#[test]
fn manifest_093_preview_height_is_inside_four_to_twenty_four() {
    let preview = manifest_rows()
        .into_iter()
        .find(|row| row["type"] == "preview")
        .expect("preview");
    let height = preview["height"].as_u64().expect("height");
    assert!((4..=24).contains(&height));
}

/// The preview `view` is an own `panels` id (reference-settings-rows.md, row 14: "the view must exist").
///
/// REQ: F-093, F-094
#[test]
fn manifest_093_preview_view_names_a_panel_the_plugin_contributes() {
    let preview = manifest_rows()
        .into_iter()
        .find(|row| row["type"] == "preview")
        .expect("preview");
    let panels: Vec<String> = manifest_value()["contributes"]["panels"]
        .as_array()
        .expect("panels")
        .iter()
        .map(|panel| panel["id"].as_str().expect("id").to_string())
        .collect();
    assert!(
        panels.contains(&preview["view"].as_str().expect("view").to_string()),
        "{panels:?}"
    );
}

/// REQ: F-103
#[test]
fn manifest_103_semantic_row_is_a_toggle_off_by_default() {
    let semantic = row("semantic");
    assert_eq!(
        semantic,
        json!({ "type": "toggle", "key": "semantic", "title": "Semantic animation", "default": false })
    );
}

/// REQ: F-093, F-094
#[test]
fn manifest_093_observer_row_is_a_searchable_location_with_current_position_and_lisbon_default() {
    let observer = row("observer");
    assert_eq!(observer["type"], "location");
    assert_eq!(observer["title"], "Observer location");
    assert_eq!(observer["search"], true);
    assert_eq!(observer["allow_current"], true);
    assert_eq!(observer["precision"], 4);
    assert_eq!(
        observer["default"],
        json!({ "lat": 38.7223, "lon": -9.1393, "label": "Lisbon" })
    );
}

/// REQ: F-093
#[test]
fn manifest_093_observer_precision_and_default_are_inside_the_location_row_limits() {
    let observer = row("observer");
    assert!(observer["precision"].as_u64().expect("precision") <= 6);
    let lat = observer["default"]["lat"].as_f64().expect("lat");
    let lon = observer["default"]["lon"].as_f64().expect("lon");
    assert!((-90.0..=90.0).contains(&lat));
    assert!((-180.0..=180.0).contains(&lon));
    assert!(
        observer["default"]["label"]
            .as_str()
            .expect("label")
            .chars()
            .count()
            <= 120
    );
}

/// `allow_current` needs `location.observer` on the client side (reference-settings-rows.md, row 13).
///
/// REQ: F-093
#[test]
fn manifest_093_allow_current_needs_the_location_observer_grant() {
    let grants = strings(&manifest_value()["capabilities"]["client"]);
    assert_eq!(row("observer")["allow_current"], true);
    assert!(
        grants.iter().any(|grant| grant == "location.observer"),
        "{grants:?}"
    );
}

/// REQ: F-093, F-094
#[test]
fn manifest_093_saved_places_row_is_a_string_map_of_at_most_eight_named_entries() {
    let places = row("saved_places");
    assert_eq!(places["type"], "map");
    assert_eq!(places["title"], "Saved places");
    assert_eq!(places["value_type"], "string");
    assert_eq!(places["key_pattern"], "^[a-z-]{1,24}$");
    assert_eq!(places["max_entries"], 8);
    assert_eq!(places["key_title"], "Name");
    assert_eq!(places["value_title"], "Label");
    assert_eq!(places["default"], json!({}));
}

/// Map titles are at most 24 characters and `max_entries` is 1 to 64 (reference-settings-rows.md, row 16).
///
/// REQ: F-093
#[test]
fn manifest_093_saved_places_titles_and_entry_cap_are_inside_the_map_row_limits() {
    let places = row("saved_places");
    assert!(places["key_title"].as_str().expect("k").chars().count() <= 24);
    assert!(places["value_title"].as_str().expect("v").chars().count() <= 24);
    assert!((1..=64).contains(&places["max_entries"].as_u64().expect("max")));
}

/// REQ: COR-011
#[test]
fn manifest_011_row_keys_are_unique_and_snake_case() {
    let keys: Vec<String> = manifest_rows()
        .iter()
        .filter_map(|row| row.get("key").and_then(Value::as_str).map(str::to_string))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), keys.len());
    for key in keys {
        assert!(
            key.chars()
                .all(|symbol| symbol.is_ascii_lowercase() || symbol == '_'),
            "{key}"
        );
    }
}

/// The page promises the plugin owns the points, a service and a topic. The manifest declares them
/// as grants and contributions (api-extension-points.md); the skeleton manifest does not (API_GAPS).
///
/// REQ: F-093, F-094
#[test]
fn manifest_093_provides_both_extension_points_by_grant() {
    let grants = strings(&manifest_value()["capabilities"]["client"]);
    for point in ["palettes", "style-presets"] {
        let grant = format!("extensions.provide:ilium.core.ambient/{point}");
        assert!(grants.contains(&grant), "missing {grant}");
    }
}

/// REQ: F-093
#[test]
fn manifest_093_provides_the_look_service_by_grant() {
    let grants = strings(&manifest_value()["capabilities"]["client"]);
    assert!(
        grants.contains(&"services.provide:ilium.core.ambient/look".to_string()),
        "{grants:?}"
    );
}

/// REQ: F-093
#[test]
fn manifest_093_declares_the_look_service_with_a_get_method() {
    let services = &manifest_value()["contributes"]["services"];
    assert!(
        services["ilium.core.ambient/look"]["get"].is_object(),
        "{services}"
    );
}

/// REQ: F-093
#[test]
fn manifest_093_declares_the_look_changed_topic_it_publishes() {
    let manifest = manifest_value();
    let published = serde_json::to_string(&manifest["contributes"]["topics"]).expect("json");
    assert!(
        published.contains("ilium.core.ambient/look-changed"),
        "{published}"
    );
}

/// REQ: F-093
#[test]
fn manifest_093_client_exports_include_the_service_provider_for_the_look_service() {
    let exports = strings(&manifest_value()["client"]["exports"]);
    assert!(
        exports.contains(&"service-provider".to_string()),
        "{exports:?}"
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_core_plugins_index_lists_ambient_look_as_a_client_plugin() {
    let row = table_row(
        &page("core-plugins"),
        "[Ambient look](/pages/core-plugin-ambient/)",
    );
    assert_eq!(
        row,
        [
            "[Ambient look](/pages/core-plugin-ambient/)",
            "`ilium.core.ambient`",
            "client",
            "Shared colour, palette, dither and display for backgrounds."
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_package_and_crate_names_follow_the_core_plugin_convention() {
    let cargo = std::fs::read_to_string(crate_dir().join("Cargo.toml")).expect("Cargo.toml");
    assert!(cargo.contains("name = \"ilium-plugin-core-ambient\""));
}
