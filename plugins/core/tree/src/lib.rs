//! Core plugin `ilium.core.tree` (manual page core-plugin-tree.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.tree` (core-plugin-tree.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct TreeServer;

impl Lifecycle for TreeServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.tree: server lifecycle.activate")
    }
}

impl Handler for TreeServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.tree: server handler.on_events")
    }
}

/// The client side of `ilium.core.tree` (core-plugin-tree.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct TreeClient;

impl Lifecycle for TreeClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.tree: client lifecycle.activate")
    }
}

impl Handler for TreeClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.tree: client handler.on_events")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use serde_json::Value;

    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    /// The four essential core plugins that cannot be disabled (LIF-183, LIF-276).
    const ESSENTIAL_PLUGIN_IDS: [&str; 4] = [
        "ilium.core.tree",
        "ilium.core.layout",
        "ilium.core.settings",
        "ilium.core.sessions",
    ];

    fn manifest() -> Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json is valid JSON")
    }

    fn string_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn extension_point<'a>(manifest: &'a Value, point_id: &str) -> Option<&'a Value> {
        manifest["contributes"]["extension_points"]
            .as_array()
            .and_then(|points| points.iter().find(|point| point["id"] == point_id))
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identity_matches_core_page() {
        let manifest = manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.tree");
        assert_eq!(manifest["name"], "Tree");
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["publisher"], "Ilium");
        assert_eq!(manifest["license"], "MIT");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_sides_and_worlds_match_core_page() {
        let manifest = manifest();
        assert_eq!(manifest["server"]["entry"], "server.wasm");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["entry"], "client.wasm");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn server_grants_match_core_page() {
        let mut grants = string_list(&manifest()["capabilities"]["server"]);
        grants.sort();
        let mut expected = vec![
            "tree.read",
            "tree.write",
            "tree.metadata",
            "tree.decorate",
            "tree.restructure",
            "extensions.provide:ilium.core.tree/before-move",
            "extensions.provide:ilium.core.tree/before-close",
            "extensions.provide:ilium.core.tree/node-sorts",
        ];
        expected.sort();
        assert_eq!(grants, expected);
    }

    /// REQ: COR-050
    #[test]
    fn client_grants_are_read_only() {
        let grants = string_list(&manifest()["capabilities"]["client"]);
        assert_eq!(grants, vec!["tree.read".to_owned()]);
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_startup() {
        let manifest = manifest();
        assert_eq!(
            string_list(&manifest["activation"]),
            vec!["startup".to_owned()]
        );
    }

    /// REQ: EXT-052
    #[test]
    fn veto_hooks_declare_mode_deadline_and_limit() {
        let manifest = manifest();
        for point_id in ["before-move", "before-close"] {
            let point = extension_point(&manifest, point_id)
                .unwrap_or_else(|| panic!("extension point {point_id} is declared"));
            assert_eq!(point["kind"], "hook");
            assert_eq!(point["mode"], "veto");
            assert_eq!(point["version"], "0.1.0");
            assert_eq!(point["deadline_ms"], 20);
            assert_eq!(point["max_handlers"], 8);
        }
    }

    /// REQ: EXT-052
    #[test]
    fn node_sorts_is_static_collect_with_64_entries() {
        let manifest = manifest();
        let point = extension_point(&manifest, "node-sorts").expect("node-sorts is declared");
        assert_eq!(point["kind"], "collect");
        assert_eq!(point["version"], "0.1.0");
        assert_eq!(point["max_entries"], 64);
        assert_eq!(point["dynamic"], false);
    }

    /// REQ: LIF-183
    #[test]
    fn tree_plugin_is_in_the_essential_set() {
        assert_eq!(ESSENTIAL_PLUGIN_IDS.len(), 4);
        let manifest = manifest();
        let plugin_id = manifest["id"].as_str().expect("manifest id is a string");
        assert!(ESSENTIAL_PLUGIN_IDS.contains(&plugin_id));
    }

    /// REQ: EXT-052
    #[test]
    fn undeclared_extension_point_is_absent() {
        assert!(extension_point(&manifest(), "after-close").is_none());
    }

    #[test]
    fn empty_event_batch_submits_nothing() {
        let submissions = TreeServer.on_events(Vec::new());
        assert!(submissions.is_empty());
    }
}
