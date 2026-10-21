//! Core plugin `ilium.core.scheduler` (manual page core-plugin-scheduler.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.scheduler` (core-plugin-scheduler.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct SchedulerServer;

impl Lifecycle for SchedulerServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.scheduler: server lifecycle.activate")
    }
}

impl Handler for SchedulerServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.scheduler: server handler.on_events")
    }
}

/// The client side of `ilium.core.scheduler` (core-plugin-scheduler.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct SchedulerClient;

impl Lifecycle for SchedulerClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.scheduler: client lifecycle.activate")
    }
}

impl Handler for SchedulerClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.scheduler: client handler.on_events")
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::Value;

    /// The manifest the plugin names; the spec tests read the declared contract from it.
    fn manifest() -> Value {
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json is valid JSON")
    }

    fn text_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
        value.pointer(pointer).and_then(Value::as_str)
    }

    /// Collects the string entries of the JSON array at `pointer`; a missing array yields an empty list.
    fn string_list_at(value: &Value, pointer: &str) -> Vec<String> {
        value
            .pointer(pointer)
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// REQ: core-plugin-scheduler.md "What it covers", the id is `ilium.core.scheduler` on the `ilium:plugin@0.1.0` api (the page carries no bracket ID here)
    #[test]
    fn manifest_declares_the_scheduler_id_on_the_plugin_api() {
        let manifest = manifest();
        assert_eq!(text_at(&manifest, "/api"), Some("ilium:plugin@0.1.0"));
        assert_eq!(text_at(&manifest, "/id"), Some("ilium.core.scheduler"));
    }

    /// REQ: core-plugin-scheduler.md "What it covers", the plugin runs on both sides: the server world owns timers and writes, the client world draws the dialogs (the page carries no bracket ID here)
    #[test]
    fn manifest_declares_a_server_side_and_a_client_side_with_their_worlds() {
        let manifest = manifest();
        assert_eq!(text_at(&manifest, "/server/world"), Some("ilium:server/base@0.1.0"));
        assert_eq!(text_at(&manifest, "/client/world"), Some("ilium:client/base@0.1.0"));
    }

    /// REQ: COR-050 (core-plugin-scheduler.md "Complete manifest": activation follows startup and lazy activation, with the triggers `startup` and `view:scheduler.queue`)
    #[test]
    fn activation_triggers_are_startup_and_the_queue_panel_view() {
        let manifest = manifest();
        assert_eq!(
            string_list_at(&manifest, "/activation"),
            vec!["startup".to_string(), "view:scheduler.queue".to_string()]
        );
    }

    /// REQ: scheduler-B08 (core-plugin-scheduler.md "Behaviour contract": text and Enter are written while the pane input gate is held, so the server needs terminal.write and terminal.input-lock)
    #[test]
    fn server_side_can_write_into_a_pane_under_the_input_gate() {
        let manifest = manifest();
        let server_grants = string_list_at(&manifest, "/capabilities/server");
        assert!(server_grants.iter().any(|grant| grant == "terminal.write"));
        assert!(server_grants.iter().any(|grant| grant == "terminal.input-lock"));
    }

    /// REQ: scheduler-B16 (core-plugin-scheduler.md "Behaviour contract": a queued prompt is sent when an agent finishes a turn, which needs agent.read; Sample 4 needs journal.read to check uncertain deliveries)
    #[test]
    fn server_side_can_read_agent_turns_and_the_input_journal() {
        let manifest = manifest();
        let server_grants = string_list_at(&manifest, "/capabilities/server");
        assert!(server_grants.iter().any(|grant| grant == "agent.read"));
        assert!(server_grants.iter().any(|grant| grant == "journal.read"));
    }

    /// REQ: core-plugin-scheduler.md "Manifest", `terminal.raw-input` is a high-risk grant that the user confirms at install (the page carries no bracket ID here)
    #[test]
    fn terminal_raw_input_is_declared_on_the_server_side() {
        let manifest = manifest();
        let server_grants = string_list_at(&manifest, "/capabilities/server");
        assert!(server_grants.iter().any(|grant| grant == "terminal.raw-input"));
    }

    /// REQ: core-plugin-scheduler.md "Complete manifest", the client side asks for `ui.panels` only (the page carries no bracket ID here)
    #[test]
    fn client_side_asks_for_ui_panels_only() {
        let manifest = manifest();
        assert_eq!(
            string_list_at(&manifest, "/capabilities/client"),
            vec!["ui.panels".to_string()]
        );
    }

    /// REQ: EXT-052 (core-plugin-scheduler.md "Extension points this plugin owns": both points are server-side, granted to the plugin and declared with the kinds `collect` and `hook` with mode `veto`)
    #[test]
    fn extension_points_are_granted_and_declared_with_their_kinds() {
        let manifest = manifest();
        let server_grants = string_list_at(&manifest, "/capabilities/server");
        assert!(server_grants
            .iter()
            .any(|grant| grant == "extensions.provide:ilium.core.scheduler/schedule-sources"));
        assert!(server_grants
            .iter()
            .any(|grant| grant == "extensions.provide:ilium.core.scheduler/before-send-scheduled"));

        let points = manifest
            .pointer("/contributes/extension_points")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let point_with_id = |point_id: &str| {
            points
                .iter()
                .find(|point| text_at(point, "/id") == Some(point_id))
                .cloned()
        };
        let schedule_sources = point_with_id("schedule-sources").expect("schedule-sources is declared");
        let before_send = point_with_id("before-send-scheduled").expect("before-send-scheduled is declared");
        assert_eq!(text_at(&schedule_sources, "/kind"), Some("collect"));
        assert_eq!(text_at(&before_send, "/kind"), Some("hook"));
        assert_eq!(text_at(&before_send, "/mode"), Some("veto"));
    }

    /// REQ: core-plugin-scheduler.md "Complete manifest", the manifest is a JSON document; empty text is rejected rather than read as a plugin with no contract (the page carries no bracket ID here)
    #[test]
    fn empty_manifest_text_is_not_a_valid_manifest() {
        assert!(serde_json::from_str::<Value>("").is_err());
    }

    /// REQ: core-plugin-scheduler.md "Complete manifest", every `view:<panel>` activation must name a panel the manifest declares (the page carries no bracket ID here)
    #[test]
    fn every_view_activation_names_a_declared_panel() {
        let manifest = manifest();
        let panel_ids: Vec<String> = manifest
            .pointer("/contributes/panels")
            .and_then(Value::as_array)
            .map(|panels| {
                panels
                    .iter()
                    .filter_map(|panel| text_at(panel, "/id").map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let view_panels: Vec<String> = string_list_at(&manifest, "/activation")
            .into_iter()
            .filter_map(|activation| activation.strip_prefix("view:").map(str::to_string))
            .collect();
        assert!(!view_panels.is_empty());
        for view_panel in &view_panels {
            assert!(
                panel_ids.contains(view_panel),
                "view activation names undeclared panel {view_panel}"
            );
        }
    }
}
