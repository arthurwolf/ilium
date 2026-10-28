//! Core plugin `ilium.core.fleet` (manual page core-plugin-fleet.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! Minimal conforming bodies: no contributions are produced until the feature logic moves here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.fleet` (core-plugin-fleet.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct FleetServer;

impl Lifecycle for FleetServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for FleetServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// The client side of `ilium.core.fleet` (core-plugin-fleet.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct FleetClient;

impl Lifecycle for FleetClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for FleetClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::{json, Value};

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
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn capability_list(manifest: &Value, side: &str) -> Vec<String> {
        string_list(&manifest["capabilities"][side])
    }

    fn all_capabilities(manifest: &Value) -> Vec<String> {
        let mut combined = capability_list(manifest, "server");
        combined.extend(capability_list(manifest, "client"));
        combined
    }

    fn cli_entry<'a>(manifest: &'a Value, name: &str) -> Option<&'a Value> {
        manifest["contributes"]["cli"]
            .as_array()
            .and_then(|entries| entries.iter().find(|entry| entry["name"] == name))
    }

    fn extension_point<'a>(manifest: &'a Value, point_id: &str) -> Option<&'a Value> {
        manifest["contributes"]["extension_points"]
            .as_array()
            .and_then(|points| points.iter().find(|point| point["id"] == point_id))
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_fleet_id_and_both_sides() {
        let manifest = manifest();
        assert_eq!(manifest["id"], "ilium.core.fleet");
        assert_eq!(manifest["server"]["entry"], "server.wasm");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["entry"], "client.wasm");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_targets_plugin_api_0_1_0() {
        let manifest = manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["version"], "0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn cli_contributions_are_panes_broadcast_and_new_pane() {
        let manifest = manifest();
        for name in ["panes", "broadcast", "new-pane"] {
            let entry = cli_entry(&manifest, name).expect("cli contribution is declared");
            assert_eq!(entry["requires_server"], json!(true));
        }
        let broadcast = cli_entry(&manifest, "broadcast").expect("broadcast is declared");
        assert_eq!(broadcast["argument_groups"][0], "pane-selector");
    }

    /// REQ: COR-050
    #[test]
    fn activation_lists_every_wake_trigger() {
        let manifest = manifest();
        assert_eq!(
            string_list(&manifest["activation"]),
            vec![
                "cli:panes",
                "cli:broadcast",
                "http:/create_agent",
                "service:broadcast",
                "command:ask-for-update",
            ]
        );
    }

    /// REQ: COR-050
    #[test]
    fn server_capabilities_include_both_extension_point_grants() {
        let manifest = manifest();
        let server = capability_list(&manifest, "server");
        assert!(server.contains(&"extensions.provide:ilium.core.fleet/selectors".to_owned()));
        assert!(server.contains(&"extensions.provide:ilium.core.fleet/before-broadcast".to_owned()));
    }

    /// REQ: COR-050
    #[test]
    fn extension_points_declare_manual_limits() {
        let manifest = manifest();
        let selectors =
            extension_point(&manifest, "selectors").expect("selectors point is declared");
        assert_eq!(selectors["kind"], "collect");
        assert_eq!(selectors["max_entries"], json!(64));
        assert_eq!(selectors["dynamic"], json!(false));

        let before_broadcast = extension_point(&manifest, "before-broadcast")
            .expect("before-broadcast point is declared");
        assert_eq!(before_broadcast["kind"], "hook");
        assert_eq!(before_broadcast["risk"], "high");
        assert_eq!(before_broadcast["deadline_ms"], json!(20));
        assert_eq!(before_broadcast["max_handlers"], json!(8));
    }

    /// REQ: COR-033
    #[test]
    fn api_settings_tab_has_exactly_one_holder() {
        let manifest = manifest();
        let holders = all_capabilities(&manifest)
            .iter()
            .filter(|grant| grant.as_str() == "settings.write-kernel:api")
            .count();
        assert_eq!(holders, 1);
    }

    /// REQ: COR-010
    #[test]
    fn http_route_is_post_create_agent_only() {
        let manifest = manifest();
        let routes = manifest["contributes"]["http_routes"]
            .as_array()
            .expect("http_routes is an array");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0]["method"], "POST");
        assert_eq!(routes[0]["path"], "/create_agent");
        assert!(capability_list(&manifest, "server")
            .contains(&"http-api.route:/create_agent".to_owned()));
    }

    /// REQ: COR-050
    #[test]
    fn missing_capability_side_yields_no_grants() {
        let without_capabilities = json!({ "id": "ilium.core.fleet" });
        assert!(capability_list(&without_capabilities, "server").is_empty());
        assert!(capability_list(&json!({ "capabilities": {} }), "client").is_empty());
    }

    /// REQ: COR-050
    #[test]
    fn truncated_manifest_text_is_rejected() {
        let truncated = &MANIFEST_TEXT[..MANIFEST_TEXT.len() / 2];
        assert!(serde_json::from_str::<Value>(truncated).is_err());
        assert!(manifest().is_object());
    }
}
