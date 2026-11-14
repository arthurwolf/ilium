//! Core plugin `ilium.core.scheduler` (manual page core-plugin-scheduler.md): input that is sent
//! to a pane later. Two features share the plugin.
//!
//! - A countdown types text and/or Enter into one pane when a durable timer fires, even after
//!   Ilium was stopped in between.
//! - A prompt queue holds prompts per pane and sends one each time the agent in that pane
//!   finishes a turn.
//!
//! The server side ([`server`]) owns the state in plugin storage and writes with `send-input`; the
//! client side ([`client`]) owns the two dialogs and the queue panel. [`wire`] is the message and
//! storage format between them; [`schedule`] and [`queue`] are the pure rules.

pub mod client;
pub mod effects;
pub mod queue;
pub mod schedule;
pub mod server;
pub mod wire;

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::{Activation, Area, PluginError, UiTree, ViewTarget};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, Lifecycle, Views};

/// The plugin id, also the target of the messages between its two sides.
pub const PLUGIN_ID: &str = "ilium.core.scheduler";

/// The server side of `ilium.core.scheduler` (core-plugin-scheduler.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct SchedulerServer;

impl Lifecycle for SchedulerServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        let mut api = host();
        server::start(&mut *api);
        Ok(())
    }
}

impl Handler for SchedulerServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut api = host();
        batch
            .into_iter()
            .flat_map(|event| server::on_event(&mut *api, event))
            .collect()
    }
}

/// The client side of `ilium.core.scheduler` (core-plugin-scheduler.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct SchedulerClient;

impl Lifecycle for SchedulerClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        client::reset();
        Ok(())
    }
}

impl Handler for SchedulerClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        batch.into_iter().flat_map(client::on_event).collect()
    }
}

impl Views for SchedulerClient {
    fn render(&mut self, view: ViewTarget, _area: Area) -> UiTree {
        client::render(&view)
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

    /// Manual: the page carries no bracket ID here
    #[test]
    fn manifest_declares_the_scheduler_id_on_the_plugin_api() {
        let manifest = manifest();
        assert_eq!(text_at(&manifest, "/api"), Some("ilium:plugin@0.1.0"));
        assert_eq!(text_at(&manifest, "/id"), Some("ilium.core.scheduler"));
    }

    /// Manual: the page carries no bracket ID here
    #[test]
    fn manifest_declares_a_server_side_and_a_client_side_with_their_worlds() {
        let manifest = manifest();
        assert_eq!(
            text_at(&manifest, "/server/world"),
            Some("ilium:server/base@0.1.0")
        );
        assert_eq!(
            text_at(&manifest, "/client/world"),
            Some("ilium:client/base@0.1.0")
        );
    }

    /// REQ: COR-050
    #[test]
    fn activation_triggers_are_startup_and_the_queue_panel_view() {
        let manifest = manifest();
        assert_eq!(
            string_list_at(&manifest, "/activation"),
            vec![
                "startup".to_string(),
                "view:ilium.core.scheduler/scheduler.queue".to_string()
            ]
        );
    }

    /// REQ: scheduler-B08
    #[test]
    fn server_side_can_write_into_a_pane_under_the_input_gate() {
        let manifest = manifest();
        let server_grants = string_list_at(&manifest, "/capabilities/server");
        assert!(server_grants.iter().any(|grant| grant == "terminal.write"));
        assert!(server_grants
            .iter()
            .any(|grant| grant == "terminal.input-lock"));
    }

    /// REQ: scheduler-B16
    #[test]
    fn server_side_can_read_agent_turns_and_the_input_journal() {
        let manifest = manifest();
        let server_grants = string_list_at(&manifest, "/capabilities/server");
        assert!(server_grants.iter().any(|grant| grant == "agent.read"));
        assert!(server_grants.iter().any(|grant| grant == "journal.read"));
    }

    /// Manual: the page carries no bracket ID here
    #[test]
    fn terminal_raw_input_is_declared_on_the_server_side() {
        let manifest = manifest();
        let server_grants = string_list_at(&manifest, "/capabilities/server");
        assert!(server_grants
            .iter()
            .any(|grant| grant == "terminal.raw-input"));
    }

    /// Manual: the page carries no bracket ID here
    #[test]
    fn client_side_asks_for_ui_panels_only() {
        let manifest = manifest();
        assert_eq!(
            string_list_at(&manifest, "/capabilities/client"),
            vec!["ui.panels".to_string()]
        );
    }

    /// REQ: EXT-052
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
        let schedule_sources =
            point_with_id("schedule-sources").expect("schedule-sources is declared");
        let before_send =
            point_with_id("before-send-scheduled").expect("before-send-scheduled is declared");
        assert_eq!(text_at(&schedule_sources, "/kind"), Some("collect"));
        assert_eq!(text_at(&before_send, "/kind"), Some("hook"));
        assert_eq!(text_at(&before_send, "/mode"), Some("veto"));
    }

    /// Manual: the page carries no bracket ID here
    #[test]
    fn empty_manifest_text_is_not_a_valid_manifest() {
        assert!(serde_json::from_str::<Value>("").is_err());
    }

    /// Manual: the page carries no bracket ID here
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
