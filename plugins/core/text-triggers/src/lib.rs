//! Core plugin `ilium.core.text-triggers` (manual page core-plugin-text-triggers.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.text-triggers` (core-plugin-text-triggers.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct TextTriggersServer;

impl Lifecycle for TextTriggersServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.text-triggers: server lifecycle.activate")
    }
}

impl Handler for TextTriggersServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.text-triggers: server handler.on_events")
    }
}

/// The client side of `ilium.core.text-triggers` (core-plugin-text-triggers.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct TextTriggersClient;

impl Lifecycle for TextTriggersClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.text-triggers: client lifecycle.activate")
    }
}

impl Handler for TextTriggersClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.text-triggers: client handler.on_events")
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
            .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    fn capability_list(manifest: &Value, side: &str) -> Vec<String> {
        string_list(&manifest["capabilities"][side])
    }

    fn all_capabilities(manifest: &Value) -> Vec<String> {
        let mut all = capability_list(manifest, "server");
        all.extend(capability_list(manifest, "client"));
        all
    }

    fn extension_point<'a>(manifest: &'a Value, point_id: &str) -> Option<&'a Value> {
        manifest["contributes"]["extension_points"]
            .as_array()?
            .iter()
            .find(|point| point["id"].as_str() == Some(point_id))
    }

    /// REQ: COR-050 - the manifest names `ilium.core.text-triggers`, targets `ilium:plugin@0.1.0` and is version 0.1.0.
    #[test]
    fn manifest_identifies_the_text_triggers_plugin() {
        let manifest = manifest();
        assert_eq!(manifest["id"].as_str(), Some("ilium.core.text-triggers"));
        assert_eq!(manifest["api"].as_str(), Some("ilium:plugin@0.1.0"));
        assert_eq!(manifest["version"].as_str(), Some("0.1.0"));
        assert_eq!(manifest["name"].as_str(), Some("Text triggers"));
    }

    /// REQ: COR-050 - the plugin runs on both sides: the server in `ilium:server/base@0.1.0`, the client in `ilium:client/base@0.1.0`.
    #[test]
    fn manifest_declares_both_sides_with_their_worlds() {
        let manifest = manifest();
        assert_eq!(manifest["server"]["entry"].as_str(), Some("server.wasm"));
        assert_eq!(manifest["server"]["world"].as_str(), Some("ilium:server/base@0.1.0"));
        assert_eq!(manifest["client"]["entry"].as_str(), Some("client.wasm"));
        assert_eq!(manifest["client"]["world"].as_str(), Some("ilium:client/base@0.1.0"));
    }

    /// REQ: COR-050 - the plugin activates at `startup` and when the rules view opens, in that order.
    #[test]
    fn manifest_activates_on_startup_and_rules_view() {
        let manifest = manifest();
        assert_eq!(
            string_list(&manifest["activation"]),
            vec!["startup", "view:text-triggers.rules"]
        );
    }

    /// REQ: COR-050 - the server side holds `terminal.output-filter` (matches output) and `terminal.write` (sends the reply).
    #[test]
    fn server_side_holds_output_filter_and_write_capabilities() {
        let server = capability_list(&manifest(), "server");
        assert!(server.iter().any(|capability| capability == "terminal.output-filter"));
        assert!(server.iter().any(|capability| capability == "terminal.write"));
    }

    /// REQ: COR-050 - the server side grants the provider right for both extension points, `rule-packs` and `before-fire`.
    #[test]
    fn server_side_provides_both_extension_points() {
        let server = capability_list(&manifest(), "server");
        for point_id in ["rule-packs", "before-fire"] {
            let grant = format!("extensions.provide:ilium.core.text-triggers/{point_id}");
            assert!(server.contains(&grant), "missing grant {grant}");
        }
    }

    /// REQ: COR-030 - exactly one holder owns the `text-triggers` kernel tab, so the write grant appears once across both sides.
    #[test]
    fn exactly_one_holder_writes_the_text_triggers_tab() {
        let manifest = manifest();
        let grant = "settings.write-kernel:text-triggers";
        let occurrences = all_capabilities(&manifest)
            .iter()
            .filter(|capability| capability.as_str() == grant)
            .count();
        assert_eq!(occurrences, 1);
    }

    /// REQ: COR-050 - `before-fire` is a veto hook with a 20 ms deadline and at most 8 handlers.
    #[test]
    fn before_fire_is_a_veto_hook_with_deadline_and_handler_cap() {
        let manifest = manifest();
        let point = extension_point(&manifest, "before-fire").expect("before-fire is declared");
        assert_eq!(point["kind"].as_str(), Some("hook"));
        assert_eq!(point["mode"].as_str(), Some("veto"));
        assert_eq!(point["deadline_ms"].as_u64(), Some(20));
        assert_eq!(point["max_handlers"].as_u64(), Some(8));
    }

    /// REQ: SEC-293 - both extension points carry the `high` risk mark, and `rule-packs` holds at most 64 entries.
    #[test]
    fn extension_points_carry_high_risk_marks() {
        let manifest = manifest();
        for point_id in ["rule-packs", "before-fire"] {
            let point = extension_point(&manifest, point_id).expect("extension point is declared");
            assert_eq!(point["risk"].as_str(), Some("high"), "risk of {point_id}");
        }
        let rule_packs = extension_point(&manifest, "rule-packs").expect("rule-packs is declared");
        assert_eq!(rule_packs["max_entries"].as_u64(), Some(64));
    }

    /// REQ: COR-050 - an empty or unknown extension point name resolves to nothing, so no contribution is invented.
    #[test]
    fn empty_or_unknown_extension_point_is_not_found() {
        let manifest = manifest();
        assert!(extension_point(&manifest, "").is_none());
        assert!(extension_point(&manifest, "no-such-point").is_none());
    }

    /// REQ: COR-050 - a manifest without its activation list fails the activation check instead of passing silently.
    #[test]
    fn manifest_without_activation_fails_the_activation_check() {
        let mut broken = manifest();
        broken
            .as_object_mut()
            .expect("manifest is a JSON object")
            .remove("activation");
        let activation = string_list(&broken["activation"]);
        assert!(activation.is_empty());
        assert_ne!(activation, vec!["startup", "view:text-triggers.rules"]);
    }
}
