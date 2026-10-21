//! Core plugin `ilium.core.recap` (manual page core-plugin-recap.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, JobRunner, Lifecycle, ServiceProvider};

/// The server side of `ilium.core.recap` (core-plugin-recap.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["job-runner", "service-provider"])]
#[derive(Default)]
pub struct RecapServer;

impl Lifecycle for RecapServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.recap: server lifecycle.activate")
    }
}

impl Handler for RecapServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.recap: server handler.on_events")
    }
}

impl JobRunner for RecapServer {
    fn run(&mut self, _kind: String, _input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        unimplemented!("ilium.core.recap: server job-runner.run")
    }
}

impl ServiceProvider for RecapServer {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.recap: server service-provider.handle")
    }
}

/// The client side of `ilium.core.recap` (core-plugin-recap.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct RecapClient;

impl Lifecycle for RecapClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.recap: client lifecycle.activate")
    }
}

impl Handler for RecapClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.recap: client handler.on_events")
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
            .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    }

    fn named_entry<'a>(list: &'a Value, key: &str, wanted: &str) -> Option<&'a Value> {
        list.as_array()
            .and_then(|entries| entries.iter().find(|entry| entry[key] == wanted))
    }

    fn has_cli_activation(manifest: &Value, command_name: &str) -> bool {
        let trigger = format!("cli:{command_name}");
        string_list(&manifest["activation"]).iter().any(|item| *item == trigger)
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_recap_plugin() {
        let manifest = manifest();
        assert_eq!(manifest["id"], "ilium.core.recap");
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["version"], "0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_both_sides_with_their_worlds_and_exports() {
        let manifest = manifest();
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(
            string_list(&manifest["server"]["exports"]),
            vec!["job-runner", "service-provider"]
        );
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert!(string_list(&manifest["client"]["exports"]).is_empty());
    }

    /// REQ: COR-050
    #[test]
    fn recap_cli_is_jsonl_and_needs_the_server() {
        let manifest = manifest();
        let cli = named_entry(&manifest["contributes"]["cli"], "name", "recap")
            .expect("recap CLI command is contributed");
        assert_eq!(cli["output"], "jsonl");
        assert_eq!(cli["stdin"], "none");
        assert_eq!(cli["requires_server"], true);
        assert_eq!(string_list(&cli["argument_groups"]), vec!["pane-selector"]);
    }

    /// REQ: COR-050
    #[test]
    fn recap_pane_job_declares_deadline_and_memory() {
        let manifest = manifest();
        let job = named_entry(&manifest["contributes"]["jobs"], "kind", "recap-pane")
            .expect("recap-pane job is contributed");
        assert_eq!(job["deadline_ms"], 5000);
        assert_eq!(job["memory_mib"], 64);
    }

    /// REQ: COR-050
    #[test]
    fn manifest_description_states_both_length_limits() {
        let manifest = manifest();
        let description = manifest["description"].as_str().expect("description is text");
        assert!(description.contains("at most 40 words"));
        assert!(description.contains("at most 250 words"));
    }

    /// REQ: COR-050
    #[test]
    fn onboarding_state_client_request_is_declared() {
        let manifest = manifest();
        assert!(named_entry(&manifest["contributes"]["client_requests"], "kind", "onboarding-state").is_some());
    }

    /// REQ: COR-050
    #[test]
    fn server_holds_grants_for_consent_gate_and_model_call() {
        let manifest = manifest();
        let server = string_list(&manifest["capabilities"]["server"]);
        for grant in [
            "settings.read-kernel:inference",
            "inference",
            "agent.transcript",
            "terminal.read-screen",
        ] {
            assert!(server.iter().any(|held| held == grant), "missing grant {grant}");
        }
    }

    /// REQ: COR-050
    #[test]
    fn extension_points_are_declared_with_their_kinds() {
        let manifest = manifest();
        let points = &manifest["contributes"]["extension_points"];
        let fact_sources = named_entry(points, "id", "fact-sources").expect("fact-sources declared");
        assert_eq!(fact_sources["kind"], "collect");
        let before_recap = named_entry(points, "id", "before-recap").expect("before-recap declared");
        assert_eq!(before_recap["kind"], "transform");
    }

    /// REQ: COR-050
    #[test]
    fn empty_or_unknown_contribution_names_match_nothing() {
        let manifest = manifest();
        assert!(named_entry(&manifest["contributes"]["cli"], "name", "").is_none());
        assert!(named_entry(&manifest["contributes"]["cli"], "name", "recaps").is_none());
        assert!(named_entry(&manifest["contributes"]["jobs"], "kind", "recap-pane-unknown").is_none());
        assert!(named_entry(&manifest["contributes"]["extension_points"], "id", "").is_none());
    }

    /// REQ: COR-050
    #[test]
    fn missing_cli_activation_trigger_is_reported() {
        let mut manifest = manifest();
        assert!(has_cli_activation(&manifest, "recap"));
        let activation = manifest["activation"]
            .as_array_mut()
            .expect("activation is a list");
        activation.retain(|item| item.as_str() != Some("cli:recap"));
        assert!(!has_cli_activation(&manifest, "recap"));
        assert!(string_list(&manifest["activation"]).iter().any(|item| item == "command:recap"));
    }
}
