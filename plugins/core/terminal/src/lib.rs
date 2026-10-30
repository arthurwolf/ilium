//! Core plugin `ilium.core.terminal` (manual page core-plugin-terminal.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Lifecycle and handler bodies keep the
//! activation state and answer event batches with no submissions.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::host::log::Level;
use ilium_plugin_sdk::types::{
    EditorOpenSpec, EditorPlacement, FsPath, Intent, RenameNodeSpec, Submission, TitleOrigin,
    TitleSource,
};
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.terminal` (core-plugin-terminal.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct TerminalServer {
    activated: bool,
}

impl Lifecycle for TerminalServer {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let _ = &context;
        self.activated = true;
        Ok(())
    }
}

impl Handler for TerminalServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::PaneExited(exit) => {
                    ilium_plugin_sdk::log::log(
                        Level::Info,
                        &format!(
                            "pane {} exited status={:?} signal={:?}",
                            exit.node.0, exit.status, exit.signal
                        ),
                    );
                }
                Event::PaneTitleChanged(change) if change.source == TitleOrigin::Osc => {
                    submissions.push(Submission::keyed(
                        Intent::RenameNode(RenameNodeSpec {
                            node: change.node,
                            title: change.title,
                            source: TitleSource::Inferred,
                            expected_revision: None,
                        }),
                        format!("osc-title-{}", change.node.0),
                    ));
                }
                _ => {}
            }
        }
        submissions
    }
}

/// Splits `path:line` text of the `path-line` detector; the line must be a `u32`.
fn parse_path_line(text: &str) -> Option<(&str, u32)> {
    let (path, line) = text.rsplit_once(':')?;
    if path.is_empty() || line.is_empty() || !line.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((path, line.parse().ok()?))
}

/// The client side of `ilium.core.terminal` (core-plugin-terminal.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct TerminalClient {
    activated: bool,
}

impl Lifecycle for TerminalClient {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let _ = &context;
        self.activated = true;
        Ok(())
    }
}

impl Handler for TerminalClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::LinkActivated(link) = event else {
                continue;
            };
            if link.detector_id != "path-line" {
                continue;
            }
            let Some((path, line)) = parse_path_line(&link.text) else {
                continue;
            };
            submissions.push(Submission::new(Intent::EditorOpen(EditorOpenSpec {
                path: FsPath {
                    scope: "project".to_string(),
                    path: path.to_string(),
                },
                line: Some(line.saturating_sub(1)),
                placement: EditorPlacement::Split,
            })));
        }
        submissions
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
        let pattern = detector["regex"]
            .as_str()
            .expect("path-line regex is a string");
        regex::Regex::new(pattern).expect("path-line regex compiles")
    }

    /// REQ: COR-050
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

    /// REQ: COR-050
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

    /// REQ: COR-050
    #[test]
    fn activation_triggers_are_startup_and_screen_changed() {
        let mut activation = string_list(&manifest()["activation"]);
        activation.sort();
        assert_eq!(activation, vec!["event:screen-changed", "startup"]);
    }

    /// REQ: COR-033
    #[test]
    fn client_side_holds_the_write_grant_for_the_terminal_settings_tab() {
        let manifest = manifest();
        let client_caps = string_list(&manifest["capabilities"]["client"]);
        assert!(client_caps
            .iter()
            .any(|cap| cap == "settings.write-kernel:terminal"));
    }

    /// REQ: COR-050
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
            assert!(
                server_caps.iter().any(|cap| cap == expected),
                "missing {expected}"
            );
        }
    }

    /// REQ: EXT-052
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

    /// REQ: SEC-293
    #[test]
    fn extension_point_risk_marks_follow_the_corrected_table() {
        let manifest = manifest();
        assert_eq!(
            extension_point(&manifest, "before-send-input")["risk"],
            "high"
        );
        assert_eq!(extension_point(&manifest, "open-link")["risk"], "high");
        assert_eq!(
            extension_point(&manifest, "link-actions")["risk"],
            "standard"
        );
    }

    /// REQ: EXT-052
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

    #[test]
    fn path_line_detector_matches_a_path_with_a_line_suffix() {
        let manifest = manifest();
        let detector = &manifest["contributes"]["link_detectors"][0];
        assert_eq!(detector["id"], "path-line");
        assert_eq!(detector["command"], "link-open");
        assert!(path_line_regex().is_match("src/server/routes.rs:42"));
    }

    #[test]
    fn path_line_detector_rejects_empty_text_and_paths_without_a_line_number() {
        let regex = path_line_regex();
        assert!(!regex.is_match(""));
        assert!(!regex.is_match("README.md"));
        assert!(!regex.is_match("src/server/routes.rs:"));
    }
}
