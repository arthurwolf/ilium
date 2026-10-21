//! Core plugin `ilium.core.ambient` (manual page core-plugin-ambient.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, Views};

/// The client side of `ilium.core.ambient` (core-plugin-ambient.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct AmbientClient;

impl Lifecycle for AmbientClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.ambient: client lifecycle.activate")
    }
}

impl Handler for AmbientClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.ambient: client handler.on_events")
    }
}

impl Views for AmbientClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.ambient: client views.render")
    }
}

#[cfg(test)]
mod spec_tests {

    /// The manifest this plugin ships, parsed from the file next to `src/`.
    fn ambient_manifest() -> serde_json::Value {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("plugins/core/ambient/manifest.json is valid JSON")
    }

    /// Finds a settings row by its `key` across every tab and section of the manifest.
    fn find_row<'a>(manifest: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
        manifest["contributes"]["settings"]
            .as_array()?
            .iter()
            .flat_map(|tab| tab["sections"].as_array().into_iter().flatten())
            .flat_map(|section| section["rows"].as_array().into_iter().flatten())
            .find(|row| row["key"].as_str() == Some(key))
    }

    /// A saved place key must be 1 to 24 characters from `a-z` and `-` (manifest `key_pattern`).
    fn is_valid_saved_place_key(key: &str) -> bool {
        (1..=24).contains(&key.len())
            && key
                .chars()
                .all(|character| character.is_ascii_lowercase() || character == '-')
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_the_ambient_look_plugin() {
        let manifest = ambient_manifest();
        assert_eq!(manifest["id"], "ilium.core.ambient");
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn client_side_exports_only_views() {
        let manifest = ambient_manifest();
        let exports = manifest["client"]["exports"]
            .as_array()
            .expect("client exports is a list");
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0], "views");
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_startup_only() {
        let manifest = ambient_manifest();
        assert_eq!(manifest["activation"], serde_json::json!(["startup"]));
    }

    /// REQ: COR-050
    #[test]
    fn client_grants_include_the_manual_table_grants() {
        let manifest = ambient_manifest();
        let grants = manifest["capabilities"]["client"]
            .as_array()
            .expect("client capabilities is a list");
        for required in ["location.geocode", "media.video", "ui.panels"] {
            assert!(
                grants.iter().any(|grant| grant == required),
                "missing client grant {required}"
            );
        }
    }

    /// REQ: COR-030
    #[test]
    fn animations_tab_is_contributed_exactly_once() {
        let manifest = ambient_manifest();
        let tabs = manifest["contributes"]["settings"]
            .as_array()
            .expect("settings is a list");
        let animation_tabs: Vec<_> = tabs.iter().filter(|tab| tab["id"] == "animations").collect();
        assert_eq!(animation_tabs.len(), 1);
        assert_eq!(animation_tabs[0]["kind"], "tab");
        assert_eq!(animation_tabs[0]["title"], "Animations");
    }

    /// REQ: COR-050
    #[test]
    fn scene_choice_defaults_to_stars_among_listed_options() {
        let manifest = ambient_manifest();
        let scene = find_row(&manifest, "scene").expect("scene row exists");
        assert_eq!(scene["type"], "choice");
        assert_eq!(scene["default"], "stars");
        let values: Vec<&str> = scene["options"]
            .as_array()
            .expect("scene options is a list")
            .iter()
            .filter_map(|option| option["value"].as_str())
            .collect();
        assert_eq!(values, vec!["stars", "clouds"]);
        assert!(values.contains(&"stars"));
        assert!(find_row(&manifest, "waves").is_none());
    }

    /// REQ: COR-050
    #[test]
    fn observer_location_defaults_to_lisbon() {
        let manifest = ambient_manifest();
        let observer = find_row(&manifest, "observer").expect("observer row exists");
        assert_eq!(observer["type"], "location");
        assert_eq!(observer["precision"].as_u64(), Some(4));
        assert_eq!(observer["default"]["label"], "Lisbon");
        assert_eq!(observer["default"]["lat"].as_f64(), Some(38.7223));
        assert_eq!(observer["default"]["lon"].as_f64(), Some(-9.1393));
    }

    /// REQ: COR-050
    #[test]
    fn saved_place_keys_follow_the_manifest_pattern() {
        let manifest = ambient_manifest();
        let saved_places = find_row(&manifest, "saved_places").expect("saved_places row exists");
        assert_eq!(saved_places["type"], "map");
        assert_eq!(saved_places["key_pattern"], "^[a-z-]{1,24}$");
        assert_eq!(saved_places["max_entries"].as_u64(), Some(8));
        assert!(is_valid_saved_place_key("home"));
        assert!(is_valid_saved_place_key("work-place"));
        assert!(!is_valid_saved_place_key(""));
        assert!(!is_valid_saved_place_key("Harbour"));
        assert!(!is_valid_saved_place_key(&"a".repeat(25)));
    }

    /// REQ: COR-050
    #[test]
    fn scenes_panel_is_contributed_non_modal() {
        let manifest = ambient_manifest();
        let panels = manifest["contributes"]["panels"]
            .as_array()
            .expect("panels is a list");
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0]["id"], "gallery");
        assert_eq!(panels[0]["title"], "Scenes");
        assert_eq!(panels[0]["modal"], false);
    }

    /// Failure case: the manual says the `animations` kernel-settings write grant is server side only
    /// ("the client part never holds it"), so the client capabilities must not list it. The current
    /// manifest.json lists `settings.write-kernel:animations` under `capabilities.client`, so this
    /// test is expected to fail until that grant moves to the server side.
    /// REQ: COR-030
    #[test]
    fn client_side_does_not_hold_the_kernel_settings_write_grant() {
        let manifest = ambient_manifest();
        let client_grants = manifest["capabilities"]["client"]
            .as_array()
            .expect("client capabilities is a list");
        assert!(!client_grants
            .iter()
            .any(|grant| grant == "settings.write-kernel:animations"));
    }
}
