//! Core plugin `ilium.core.agent-setup` (manual page core-plugin-agent-setup.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

pub mod config_writer;

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

/// The server side of `ilium.core.agent-setup` (core-plugin-agent-setup.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
pub struct AgentSetupServer;

impl Lifecycle for AgentSetupServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.agent-setup: server lifecycle.activate")
    }
}

impl Handler for AgentSetupServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.agent-setup: server handler.on_events")
    }
}

impl ServiceProvider for AgentSetupServer {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.agent-setup: server service-provider.handle")
    }
}

/// The client side of `ilium.core.agent-setup` (core-plugin-agent-setup.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct AgentSetupClient;

impl Lifecycle for AgentSetupClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.agent-setup: client lifecycle.activate")
    }
}

impl Handler for AgentSetupClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.agent-setup: client handler.on_events")
    }
}

impl Views for AgentSetupClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.agent-setup: client views.render")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// The shipped manifest, read at compile time so the contract checks need no host.
    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    fn manifest() -> serde_json::Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json parses as JSON")
    }

    fn string_items(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .expect("expected a JSON array")
            .iter()
            .map(|item| item.as_str().expect("expected a JSON string").to_string())
            .collect()
    }

    fn side_capabilities(manifest: &serde_json::Value, side: &str) -> Vec<String> {
        string_items(&manifest["capabilities"][side])
    }

    /// REQ: COR-050
    #[test]
    fn manifest_carries_every_required_key() {
        let manifest = manifest();
        for key in [
            "api",
            "id",
            "name",
            "version",
            "publisher",
            "server",
            "client",
            "capabilities",
            "activation",
            "limits_tier",
        ] {
            assert!(manifest.get(key).is_some(), "manifest is missing `{key}`");
        }
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.agent-setup");
        assert_eq!(manifest["limits_tier"], "first-party");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_server_and_client_sides() {
        let manifest = manifest();
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(
            string_items(&manifest["server"]["exports"]),
            vec!["service-provider"]
        );
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(string_items(&manifest["client"]["exports"]), vec!["views"]);
        let _server = AgentSetupServer;
        let _client = AgentSetupClient;
    }

    /// REQ: COR-050
    #[test]
    fn manifest_activation_lists_the_three_triggers() {
        assert_eq!(
            string_items(&manifest()["activation"]),
            vec!["settings:setup", "command:refresh", "service:install"]
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_surface_names_exactly_the_replaceable_contributions() {
        let manifest = manifest();
        assert_eq!(manifest["replaceable"], true);
        assert_eq!(
            string_items(&manifest["surface"]),
            vec![
                "services:install",
                "points:instruction-blocks",
                "points:targets",
                "points:before-install",
                "commands:refresh",
            ]
        );
    }

    /// REQ: COR-011
    #[test]
    fn client_side_holds_no_file_grants() {
        let manifest = manifest();
        let client = side_capabilities(&manifest, "client");
        assert!(
            client.iter().all(|grant| !grant.starts_with("fs.")),
            "client grants: {client:?}"
        );
    }

    /// REQ: COR-050
    #[test]
    fn no_direct_agent_config_write_grant_is_declared() {
        let manifest = manifest();
        let server = side_capabilities(&manifest, "server");
        let direct_writes: Vec<&String> = server
            .iter()
            .filter(|grant| grant.starts_with("fs.write:agent-config:"))
            .collect();
        assert!(
            direct_writes.is_empty(),
            "direct agent-config writes: {direct_writes:?}"
        );
        assert!(server
            .iter()
            .any(|grant| grant == "agent-config.propose:claude"));
        assert!(server
            .iter()
            .any(|grant| grant == "agent-config.propose:codex"));
    }

    /// REQ: COR-033
    #[test]
    fn exactly_one_grant_writes_the_setup_tab() {
        let manifest = manifest();
        let all_grants: Vec<String> = side_capabilities(&manifest, "server")
            .into_iter()
            .chain(side_capabilities(&manifest, "client"))
            .collect();
        let kernel_writes: Vec<String> = all_grants
            .into_iter()
            .filter(|grant| grant.starts_with("settings.write-kernel:"))
            .collect();
        assert_eq!(kernel_writes, vec!["settings.write-kernel:setup"]);
    }

    /// REQ: SEC-293
    #[test]
    fn before_install_hook_is_a_high_risk_veto() {
        let manifest = manifest();
        let points = manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points is an array");
        let before_install = points
            .iter()
            .find(|point| point["id"] == "before-install")
            .expect("before-install point is declared");
        assert_eq!(before_install["kind"], "hook");
        assert_eq!(before_install["mode"], "veto");
        assert_eq!(before_install["risk"], "high");
        assert_eq!(before_install["deadline_ms"], 20);
        assert_eq!(before_install["max_handlers"], 4);
    }

    /// REQ: COR-050
    #[test]
    fn collect_points_keep_their_documented_caps() {
        let manifest = manifest();
        let points = manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points is an array");
        let cap_of = |point_id: &str| -> u64 {
            points
                .iter()
                .find(|point| point["id"] == point_id)
                .and_then(|point| point["max_entries"].as_u64())
                .expect("collect point declares a positive entry cap")
        };
        assert_eq!(cap_of("instruction-blocks"), 8);
        assert_eq!(cap_of("targets"), 16);
        assert!(cap_of("targets") > 0);
    }

    /// REQ: COR-050
    #[test]
    fn install_and_remove_params_require_block_and_target() {
        let manifest = manifest();
        let service = &manifest["contributes"]["services"]["ilium.core.agent-setup/install"];
        for operation in ["install", "remove"] {
            let required = string_items(&service[operation]["params"]["required"]);
            assert_eq!(required, vec!["block", "target"], "operation `{operation}`");
        }
    }
}
