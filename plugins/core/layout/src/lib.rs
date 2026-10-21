//! Core plugin `ilium.core.layout` (manual page core-plugin-layout.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.layout` (core-plugin-layout.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct LayoutServer;

impl Lifecycle for LayoutServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.layout: server lifecycle.activate")
    }
}

impl Handler for LayoutServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.layout: server handler.on_events")
    }
}

/// The client side of `ilium.core.layout` (core-plugin-layout.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct LayoutClient;

impl Lifecycle for LayoutClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.layout: client lifecycle.activate")
    }
}

impl Handler for LayoutClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.layout: client handler.on_events")
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::{json, Value};

    /// The shipped manifest; core-plugin-layout.md "Complete manifest" is the contract it must meet.
    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    /// Required top-level keys of the `ilium:plugin@0.1.0` manifest, per core-plugin-layout.md.
    const REQUIRED_TOP_LEVEL_KEYS: [&str; 13] = [
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
        "contributes",
        "activation",
        "limits_tier",
    ];

    fn parsed_manifest() -> Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json parses as JSON")
    }

    fn missing_required_keys(manifest: &Value) -> Vec<&'static str> {
        REQUIRED_TOP_LEVEL_KEYS
            .iter()
            .copied()
            .filter(|key| manifest.get(*key).is_none())
            .collect()
    }

    fn string_array(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("expected a JSON array")
            .iter()
            .map(|item| item.as_str().expect("expected a string").to_owned())
            .collect()
    }

    fn extension_point<'a>(manifest: &'a Value, point_id: &str) -> &'a Value {
        manifest["contributes"]["extension_points"]
            .as_array()
            .expect("contributes.extension_points is an array")
            .iter()
            .find(|point| point["id"] == point_id)
            .unwrap_or_else(|| panic!("extension point {point_id} is declared"))
    }

    /// REQ: COR-050
    #[test]
    fn manifest_has_every_required_top_level_key() {
        let manifest = parsed_manifest();
        let missing = missing_required_keys(&manifest);
        assert!(missing.is_empty(), "manifest.json lacks keys: {missing:?}");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_layout_as_first_party_plugin() {
        let manifest = parsed_manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.layout");
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["publisher"], "Ilium");
        assert_eq!(manifest["limits_tier"], "first-party");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_server_and_client_sides_with_worlds() {
        let manifest = parsed_manifest();
        assert_eq!(manifest["server"]["entry"], "server.wasm");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["entry"], "client.wasm");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_capabilities_per_side_match_the_page() {
        let manifest = parsed_manifest();
        assert_eq!(
            string_array(&manifest["capabilities"]["server"]),
            [
                "panes.read",
                "panes.lifecycle",
                "panes.create",
                "tree.restructure",
                "extensions.provide:ilium.core.layout/layout-presets",
                "extensions.provide:ilium.core.layout/before-split",
            ]
        );
        assert_eq!(
            string_array(&manifest["capabilities"]["client"]),
            ["panes.read", "ui.panels"]
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_contributes_both_extension_points_with_page_limits() {
        let manifest = parsed_manifest();

        let presets = extension_point(&manifest, "layout-presets");
        assert_eq!(presets["kind"], "collect");
        assert_eq!(presets["max_entries"], 64);
        assert_eq!(presets["dynamic"], Value::Bool(false));
        assert_eq!(presets["risk"], "standard");

        let before_split = extension_point(&manifest, "before-split");
        assert_eq!(before_split["kind"], "hook");
        assert_eq!(before_split["mode"], "veto");
        assert_eq!(before_split["deadline_ms"], 20);
        assert_eq!(before_split["max_handlers"], 8);
        assert_eq!(before_split["risk"], "standard");
        assert_eq!(
            string_array(&before_split["payload_schema"]["required"]),
            ["node", "direction", "pane_count"]
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_activation_names_the_command_and_view_triggers() {
        let manifest = parsed_manifest();
        assert_eq!(
            string_array(&manifest["activation"]),
            ["command:layout.preset", "view:layout.presets"]
        );
    }

    /// REQ: COR-050
    #[test]
    fn empty_manifest_reports_every_required_key_missing() {
        let missing = missing_required_keys(&json!({}));
        assert_eq!(missing, REQUIRED_TOP_LEVEL_KEYS);
    }

    /// REQ: COR-050
    #[test]
    fn truncated_manifest_text_is_refused() {
        assert!(serde_json::from_str::<Value>("{\"api\":").is_err());
    }
}
