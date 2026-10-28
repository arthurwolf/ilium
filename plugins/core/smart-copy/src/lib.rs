//! Core plugin `ilium.core.smart-copy` (manual page core-plugin-smart-copy.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Lifecycle and handler bodies keep the
//! activation state and answer event batches with no submissions.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.smart-copy` (core-plugin-smart-copy.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct SmartCopyServer {
    activated: bool,
}

impl Lifecycle for SmartCopyServer {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let _ = &context;
        self.activated = true;
        Ok(())
    }
}

impl Handler for SmartCopyServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// The client side of `ilium.core.smart-copy` (core-plugin-smart-copy.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct SmartCopyClient {
    activated: bool,
}

impl Lifecycle for SmartCopyClient {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let _ = &context;
        self.activated = true;
        Ok(())
    }
}

impl Handler for SmartCopyClient {
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
            .expect("value is an array")
            .iter()
            .map(|item| item.as_str().expect("item is a string").to_string())
            .collect()
    }

    fn extension_point(manifest: &Value, point_id: &str) -> Value {
        manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points is an array")
            .iter()
            .find(|point| point["id"] == point_id)
            .cloned()
            .expect("extension point is declared")
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_smart_copy_and_both_worlds() {
        let manifest = manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.smart-copy");
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_the_smart_copy_command_and_the_terminal_menu_runs_it() {
        let manifest = manifest();
        assert_eq!(
            string_list(&manifest["activation"]),
            vec!["command:smart-copy".to_string()]
        );
        let entries = manifest["contributes"]["context_menu"]
            .as_array()
            .expect("context_menu is an array");
        let smart_copy = entries
            .iter()
            .find(|entry| entry["id"] == "smart-copy")
            .expect("smart-copy menu entry exists");
        assert_eq!(smart_copy["command"], "smart-copy");
        assert_eq!(smart_copy["target"], "terminal");
    }

    /// REQ: COR-050
    #[test]
    fn server_capabilities_include_the_read_grants_and_both_extension_grants() {
        let manifest = manifest();
        let server = string_list(&manifest["capabilities"]["server"]);
        for expected in [
            "terminal.read-cells",
            "terminal.read-screen",
            "inference",
            "extensions.provide:ilium.core.smart-copy/recognisers",
            "extensions.provide:ilium.core.smart-copy/after-copy",
        ] {
            assert!(
                server.iter().any(|grant| grant == expected),
                "missing server grant {expected}"
            );
        }
    }

    /// REQ: EXT-052
    #[test]
    fn recognisers_point_collects_up_to_64_entries_with_the_five_required_fields() {
        let point = extension_point(&manifest(), "recognisers");
        assert_eq!(point["kind"], "collect");
        assert_eq!(point["max_entries"], 64);
        assert_eq!(point["version"], "0.1.0");
        assert_eq!(
            string_list(&point["entry_schema"]["required"]),
            vec!["id", "kind", "regex", "multiline", "label"]
        );
    }

    /// REQ: EXT-052
    #[test]
    fn after_copy_point_is_a_transform_hook_with_a_20_ms_deadline_and_8_handlers() {
        let point = extension_point(&manifest(), "after-copy");
        assert_eq!(point["kind"], "hook");
        assert_eq!(point["mode"], "transform");
        assert_eq!(point["deadline_ms"], 20);
        assert_eq!(point["max_handlers"], 8);
    }

    /// REQ: SEC-293
    #[test]
    fn after_copy_is_marked_high_risk() {
        let point = extension_point(&manifest(), "after-copy");
        assert_eq!(point["risk"], "high");
    }

    /// REQ: EXT-052
    #[test]
    fn recogniser_entry_without_a_regex_is_invalid_under_its_schema() {
        let point = extension_point(&manifest(), "recognisers");
        let required = string_list(&point["entry_schema"]["required"]);
        let entry: Value = serde_json::json!({
            "id": "path",
            "kind": "path",
            "multiline": false,
            "label": "Path"
        });
        let missing: Vec<&String> = required
            .iter()
            .filter(|field| entry.get(field.as_str()).is_none())
            .collect();
        assert_eq!(missing, vec!["regex"]);
    }

    /// REQ: EXT-052
    #[test]
    fn after_copy_reply_without_text_is_rejected_under_its_schema() {
        let point = extension_point(&manifest(), "after-copy");
        let required = string_list(&point["reply_schema"]["required"]);
        assert_eq!(required, vec!["text"]);
        let empty_reply: Value = serde_json::json!({});
        assert!(empty_reply.get("text").is_none());
    }

    /// REQ: COR-050
    #[test]
    fn plugin_is_replaceable_and_its_surface_lists_the_copied_topic() {
        let manifest = manifest();
        assert_eq!(manifest["replaceable"], true);
        let surface = string_list(&manifest["surface"]);
        assert!(surface.iter().any(|entry| entry == "topics:copied"));
        assert!(surface.iter().any(|entry| entry == "commands:smart-copy"));
    }
}
