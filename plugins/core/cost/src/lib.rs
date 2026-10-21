//! Core plugin `ilium.core.cost` (manual page core-plugin-cost.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, ServiceProvider, Views};

/// The server side of `ilium.core.cost` (core-plugin-cost.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
pub struct CostServer;

impl Lifecycle for CostServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.cost: server lifecycle.activate")
    }
}

impl Handler for CostServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.cost: server handler.on_events")
    }
}

impl ServiceProvider for CostServer {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.cost: server service-provider.handle")
    }
}

/// The client side of `ilium.core.cost` (core-plugin-cost.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct CostClient;

impl Lifecycle for CostClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.cost: client lifecycle.activate")
    }
}

impl Handler for CostClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.cost: client handler.on_events")
    }
}

impl Views for CostClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.cost: client views.render")
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
            .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
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

    /// REQ: COR-050
    #[test]
    fn manifest_declares_cost_id_and_both_sides() {
        let manifest = manifest();
        assert_eq!(manifest["id"], "ilium.core.cost");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(string_list(&manifest["server"]["exports"]), vec!["service-provider"]);
        assert_eq!(string_list(&manifest["client"]["exports"]), vec!["views"]);
    }

    /// REQ: COR-050
    #[test]
    fn manifest_activates_at_startup_and_for_cost_panel() {
        let manifest = manifest();
        let activation = string_list(&manifest["activation"]);
        assert!(activation.contains(&"startup".to_string()));
        assert!(activation.contains(&"view:cost.panel".to_string()));
    }

    /// REQ: COR-050
    #[test]
    fn server_capabilities_grant_usage_service_and_agent_cost_tab() {
        let server = capability_list(&manifest(), "server");
        assert!(server.contains(&"services.provide:ilium.core.cost/usage".to_string()));
        assert!(server.contains(&"tree.decorate".to_string()));
        assert!(server.contains(&"settings.contribute:agent-cost".to_string()));
    }

    /// REQ: COR-030
    #[test]
    fn server_capabilities_grant_reset_planning_kernel_tab() {
        let server = capability_list(&manifest(), "server");
        assert!(server.contains(&"settings.write-kernel:reset-planning".to_string()));
    }

    /// REQ: COR-033
    #[test]
    fn each_owned_kernel_tab_has_exactly_one_holder() {
        let manifest = manifest();
        let combined = all_capabilities(&manifest);
        for tab in ["agent-cost", "reset-planning"] {
            let grant = format!("settings.write-kernel:{tab}");
            let holders = combined.iter().filter(|capability| **capability == grant).count();
            assert_eq!(holders, 1, "tab {tab} must have exactly one holder");
        }
    }

    /// REQ: COR-050
    #[test]
    fn server_capabilities_provide_price_and_quota_extension_points() {
        let server = capability_list(&manifest(), "server");
        assert!(server.contains(&"extensions.provide:ilium.core.cost/price-sources".to_string()));
        assert!(server.contains(&"extensions.provide:ilium.core.cost/quota-sources".to_string()));
    }

    /// REQ: COR-050
    #[test]
    fn unit_setting_defaults_to_dollars_with_quota_option() {
        let manifest = manifest();
        let unit_row = &manifest["contributes"]["settings"][0]["rows"][0];
        assert_eq!(unit_row["type"], "choice");
        assert_eq!(unit_row["key"], "unit");
        assert_eq!(unit_row["default"], "dollars");
        let option_values: Vec<String> = unit_row["options"]
            .as_array()
            .expect("unit row lists options")
            .iter()
            .filter_map(|option| option["value"].as_str().map(str::to_owned))
            .collect();
        assert_eq!(option_values, vec!["dollars", "quota"]);
    }

    /// REQ: COR-050
    #[test]
    fn usage_reply_without_panes_violates_declared_result_schema() {
        let manifest = manifest();
        let result_schema = &manifest["contributes"]["services"]["ilium.core.cost/usage"]["usage"]["result"];
        let required = string_list(&result_schema["required"]);
        assert!(required.contains(&"panes".to_string()));
        let empty_reply = serde_json::json!({});
        let missing: Vec<&String> = required
            .iter()
            .filter(|key| empty_reply.get(key.as_str()).is_none())
            .collect();
        assert_eq!(missing, vec![&"panes".to_string()]);
    }

    /// REQ: COR-050
    #[test]
    fn truncated_manifest_text_does_not_parse() {
        let truncated: String = MANIFEST_TEXT.chars().take(MANIFEST_TEXT.chars().count() / 2).collect();
        assert!(serde_json::from_str::<Value>(&truncated).is_err());
    }

    /// REQ: COR-050
    #[test]
    fn server_and_client_sides_are_declared_structs() {
        let _server: CostServer = CostServer::default();
        let _client: CostClient = CostClient::default();
    }
}
