//! Core plugin `ilium.core.resources` (manual page core-plugin-resources.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Minimal conforming bodies: no contributions are produced until the feature logic moves here.

pub mod identity;

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.resources` (core-plugin-resources.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct ResourcesServer;

impl Lifecycle for ResourcesServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for ResourcesServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// The client side of `ilium.core.resources` (core-plugin-resources.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct ResourcesClient;

impl Lifecycle for ResourcesClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for ResourcesClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::Value;

    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    fn manifest() -> Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json is valid JSON")
    }

    fn string_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_the_resources_plugin() {
        let manifest = manifest();
        assert_eq!(manifest["id"].as_str(), Some("ilium.core.resources"));
        assert_eq!(manifest["api"].as_str(), Some("ilium:plugin@0.1.0"));
        assert_eq!(manifest["version"].as_str(), Some("0.1.0"));
        assert_eq!(manifest["name"].as_str(), Some("Shared resources"));
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_both_sides_with_their_worlds() {
        let manifest = manifest();
        assert_eq!(manifest["server"]["entry"].as_str(), Some("server.wasm"));
        assert_eq!(
            manifest["server"]["world"].as_str(),
            Some("ilium:server/base@0.1.0")
        );
        assert_eq!(manifest["client"]["entry"].as_str(), Some("client.wasm"));
        assert_eq!(
            manifest["client"]["world"].as_str(),
            Some("ilium:client/base@0.1.0")
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_activates_on_startup() {
        let manifest = manifest();
        assert_eq!(string_list(&manifest["activation"]), vec!["startup"]);
    }

    /// REQ: COR-050
    #[test]
    fn manifest_grants_its_extension_points() {
        let manifest = manifest();
        let mut granted = string_list(&manifest["capabilities"]["server"]);
        granted.extend(string_list(&manifest["capabilities"]["client"]));
        for point_id in ["resource-kinds", "before-grant"] {
            let grant = format!("extensions.provide:ilium.core.resources/{point_id}");
            assert!(granted.contains(&grant), "missing grant {grant}");
        }
    }
}
