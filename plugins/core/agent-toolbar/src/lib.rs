//! Core plugin `ilium.core.agent-toolbar` (manual page core-plugin-agent-toolbar.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.agent-toolbar` (core-plugin-agent-toolbar.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct AgentToolbarServer;

impl Lifecycle for AgentToolbarServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.agent-toolbar: server lifecycle.activate")
    }
}

impl Handler for AgentToolbarServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.agent-toolbar: server handler.on_events")
    }
}

/// The client side of `ilium.core.agent-toolbar` (core-plugin-agent-toolbar.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct AgentToolbarClient;

impl Lifecycle for AgentToolbarClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.agent-toolbar: client lifecycle.activate")
    }
}

impl Handler for AgentToolbarClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.agent-toolbar: client handler.on_events")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use std::collections::BTreeSet;

    fn manifest() -> serde_json::Value {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("manifest.json must be valid JSON")
    }

    fn to_set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn string_set(values: &serde_json::Value) -> BTreeSet<String> {
        values
            .as_array()
            .expect("expected a JSON array")
            .iter()
            .map(|value| value.as_str().expect("expected a string").to_string())
            .collect()
    }

    fn command_ids(manifest: &serde_json::Value) -> BTreeSet<String> {
        manifest["contributes"]["commands"]
            .as_array()
            .expect("commands array")
            .iter()
            .map(|command| command["id"].as_str().expect("command id").to_string())
            .collect()
    }

    /// REQ: COR-050
    #[test]
    fn manifest_names_plugin_id_api_and_both_sides_with_their_worlds() {
        let _server: AgentToolbarServer = Default::default();
        let _client: AgentToolbarClient = Default::default();
        let manifest = manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.agent-toolbar");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn server_side_capabilities_match_the_manual_table() {
        let manifest = manifest();
        let expected = to_set(&[
            "agent.read",
            "agent.read-prompts",
            "agent.transcript",
            "agent.control",
            "journal.read",
        ]);
        assert_eq!(string_set(&manifest["capabilities"]["server"]), expected);
    }

    /// REQ: COR-050
    #[test]
    fn client_side_grants_include_both_extension_point_provides() {
        let client = string_set(&manifest()["capabilities"]["client"]);
        assert!(client.contains("extensions.provide:ilium.core.agent-toolbar/toolbar-actions"));
        assert!(client.contains("extensions.provide:ilium.core.agent-toolbar/banner-segments"));
    }

    /// REQ: COR-050
    #[test]
    fn server_only_capabilities_are_not_granted_to_the_client_side() {
        let client = string_set(&manifest()["capabilities"]["client"]);
        assert!(!client.contains("agent.control"));
        assert!(!client.contains("journal.read"));
    }

    /// REQ: COR-050
    #[test]
    fn both_collect_extension_points_are_declared_with_the_manual_limits() {
        let manifest = manifest();
        let points = manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points array");
        let ids: BTreeSet<String> = points
            .iter()
            .map(|point| point["id"].as_str().expect("point id").to_string())
            .collect();
        assert_eq!(ids, to_set(&["toolbar-actions", "banner-segments"]));
        for point in points {
            assert_eq!(point["kind"], "collect");
            assert_eq!(point["max_entries"], 64);
            assert_eq!(point["dynamic"], false);
        }
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_startup_and_agent_activity_only() {
        let activation = string_set(&manifest()["activation"]);
        assert_eq!(
            activation,
            to_set(&["startup", "event:agent-activity-changed"])
        );
    }

    /// REQ: COR-050
    #[test]
    fn contributed_commands_and_frame_slots_match_the_manual_tables() {
        let manifest = manifest();
        assert_eq!(command_ids(&manifest), to_set(&["compact", "history"]));
        let slots: Vec<(String, String)> = manifest["contributes"]["pane_frame_slots"]
            .as_array()
            .expect("pane_frame_slots array")
            .iter()
            .map(|slot| {
                (
                    slot["id"].as_str().expect("slot id").to_string(),
                    slot["placement"]
                        .as_str()
                        .expect("slot placement")
                        .to_string(),
                )
            })
            .collect();
        assert_eq!(
            slots,
            vec![
                ("prompt".to_string(), "banner".to_string()),
                ("model".to_string(), "header-text".to_string()),
                ("recover".to_string(), "footer-row".to_string()),
            ]
        );
    }

    /// REQ: COR-011
    #[test]
    fn toolbar_compact_button_keeps_its_icon_label_and_command() {
        let manifest = manifest();
        let button = &manifest["contributes"]["tree_toolbar"][0];
        assert_eq!(button["id"], "compact");
        assert_eq!(button["icon"], "arrows-minimize");
        assert_eq!(button["label"], "Compact");
        assert_eq!(button["command"], "compact");
    }

    /// REQ: COR-011
    #[test]
    fn debug_history_row_action_keeps_its_label_scope_and_order() {
        let manifest = manifest();
        let history = &manifest["contributes"]["row_actions"][0];
        assert_eq!(history["id"], "history");
        assert_eq!(history["label"], "Debug history");
        assert_eq!(history["command"], "history");
        assert_eq!(history["applies_to"], serde_json::json!(["agent"]));
        assert_eq!(history["order"], 200);
    }

    /// REQ: COR-011
    #[test]
    fn every_toolbar_and_row_action_points_at_a_declared_command() {
        let manifest = manifest();
        let declared = command_ids(&manifest);
        let referenced: Vec<&str> = manifest["contributes"]["tree_toolbar"]
            .as_array()
            .expect("tree_toolbar array")
            .iter()
            .chain(
                manifest["contributes"]["row_actions"]
                    .as_array()
                    .expect("row_actions array")
                    .iter(),
            )
            .map(|entry| entry["command"].as_str().expect("command reference"))
            .collect();
        assert!(!referenced.is_empty());
        for command in referenced {
            assert!(declared.contains(command), "undeclared command {command}");
        }
    }
}
