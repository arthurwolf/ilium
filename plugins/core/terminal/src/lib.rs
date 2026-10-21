//! Core plugin `ilium.core.terminal` (manual page core-plugin-terminal.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.terminal` (core-plugin-terminal.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct TerminalServer;

impl Lifecycle for TerminalServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.terminal: server lifecycle.activate")
    }
}

impl Handler for TerminalServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.terminal: server handler.on_events")
    }
}

/// The client side of `ilium.core.terminal` (core-plugin-terminal.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct TerminalClient;

impl Lifecycle for TerminalClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.terminal: client lifecycle.activate")
    }
}

impl Handler for TerminalClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.terminal: client handler.on_events")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use serde_json::Value;

    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    fn manifest() -> Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json is valid JSON")
    }

    fn string_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("manifest value is an array")
            .iter()
            .map(|entry| entry.as_str().expect("array entry is a string").to_string())
            .collect()
    }

    fn extension_point(manifest: &Value, point_id: &str) -> Value {
        manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points is an array")
            .iter()
            .find(|point| point["id"] == point_id)
            .cloned()
            .expect("extension point is declared in the manifest")
    }

    fn path_line_regex() -> regex::Regex {
        let manifest = manifest();
        let detector = &manifest["contributes"]["link_detectors"][0];
        let pattern = detector["regex"].as_str().expect("path-line regex is a string");
        regex::Regex::new(pattern).expect("path-line regex compiles")
    }

    /// REQ: COR-050 (manual "Complete manifest": plugin identity at ilium:plugin@0.1.0)
    #[test]
    fn manifest_identity_matches_the_manual() {
        let manifest = manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.terminal");
        assert_eq!(manifest["name"], "Terminal");
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["publisher"], "Ilium");
        assert_eq!(manifest["license"], "MIT");
    }

    /// REQ: COR-050 (manual "Complete manifest": one server and one client entry, each with its world; one struct per side)
    #[test]
    fn each_side_has_an_entry_a_world_and_a_struct() {
        let manifest = manifest();
        assert_eq!(manifest["server"]["entry"], "server.wasm");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["entry"], "client.wasm");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        let _server: TerminalServer = TerminalServer::default();
        let _client: TerminalClient = TerminalClient::default();
    }

    /// REQ: COR-050 (manual "Complete manifest": activation on startup and on screen-changed)
    #[test]
    fn activation_triggers_are_startup_and_screen_changed() {
        let mut activation = string_list(&manifest()["activation"]);
        activation.sort();
        assert_eq!(activation, vec!["event:screen-changed", "startup"]);
    }

    /// REQ: COR-033 (manual "Kernel tabs this plugin owns": the client side holds settings.write-kernel:terminal)
    #[test]
    fn client_side_holds_the_write_grant_for_the_terminal_settings_tab() {
        let manifest = manifest();
        let client_caps = string_list(&manifest["capabilities"]["client"]);
        assert!(client_caps.iter().any(|cap| cap == "settings.write-kernel:terminal"));
    }

    /// REQ: COR-050 (manual "Complete manifest": server capabilities for terminal, pane and tree access)
    #[test]
    fn server_side_grants_terminal_pane_and_tree_capabilities() {
        let manifest = manifest();
        let server_caps = string_list(&manifest["capabilities"]["server"]);
        for expected in [
            "terminal.read-screen",
            "terminal.read-cells",
            "terminal.history",
            "terminal.write",
            "panes.read",
            "panes.signal",
            "tree.write",
        ] {
            assert!(server_caps.iter().any(|cap| cap == expected), "missing {expected}");
        }
    }

    /// REQ: EXT-052 (manual "Extension points this plugin owns": 20 ms deadline and 8 handlers for the two hooks, 64 entries for link-actions)
    #[test]
    fn extension_point_limits_match_the_manual_table() {
        let manifest = manifest();
        let before_send = extension_point(&manifest, "before-send-input");
        assert_eq!(before_send["deadline_ms"], 20);
        assert_eq!(before_send["max_handlers"], 8);
        let open_link = extension_point(&manifest, "open-link");
        assert_eq!(open_link["deadline_ms"], 20);
        assert_eq!(open_link["max_handlers"], 8);
        let link_actions = extension_point(&manifest, "link-actions");
        assert_eq!(link_actions["max_entries"], 64);
    }

    /// REQ: SEC-293 (manual "Extension points": before-send-input and open-link are high risk, link-actions standard)
    #[test]
    fn extension_point_risk_marks_follow_the_corrected_table() {
        let manifest = manifest();
        assert_eq!(extension_point(&manifest, "before-send-input")["risk"], "high");
        assert_eq!(extension_point(&manifest, "open-link")["risk"], "high");
        assert_eq!(extension_point(&manifest, "link-actions")["risk"], "standard");
    }

    /// REQ: EXT-052 (manual "Extension points this plugin owns": open-link is provided from the client side only)
    #[test]
    fn open_link_is_provided_from_the_client_side_not_the_server() {
        let manifest = manifest();
        let server_caps = string_list(&manifest["capabilities"]["server"]);
        let client_caps = string_list(&manifest["capabilities"]["client"]);
        assert!(!server_caps
            .iter()
            .any(|cap| cap.starts_with("extensions.provide:ilium.core.terminal/open-link")));
        assert!(client_caps
            .iter()
            .any(|cap| cap == "extensions.provide:ilium.core.terminal/open-link"));
    }

    /// REQ: none bracketed; manual section "Clickable links": the path-line detector matches file:line text and maps to link-open
    #[test]
    fn path_line_detector_matches_a_path_with_a_line_suffix() {
        let manifest = manifest();
        let detector = &manifest["contributes"]["link_detectors"][0];
        assert_eq!(detector["id"], "path-line");
        assert_eq!(detector["command"], "link-open");
        assert!(path_line_regex().is_match("src/server/routes.rs:42"));
    }

    /// REQ: none bracketed; manual section "Clickable links": empty text and paths without a line number are not path links
    #[test]
    fn path_line_detector_rejects_empty_text_and_paths_without_a_line_number() {
        let regex = path_line_regex();
        assert!(!regex.is_match(""));
        assert!(!regex.is_match("README.md"));
        assert!(!regex.is_match("src/server/routes.rs:"));
    }
}
