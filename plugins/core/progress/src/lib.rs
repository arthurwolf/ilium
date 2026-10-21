//! Core plugin `ilium.core.progress` (manual page core-plugin-progress.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle};

/// The server side of `ilium.core.progress` (core-plugin-progress.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct ProgressServer;

impl Lifecycle for ProgressServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.progress: server lifecycle.activate")
    }
}

impl Handler for ProgressServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.progress: server handler.on_events")
    }
}

/// The client side of `ilium.core.progress` (core-plugin-progress.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct ProgressClient;

impl Lifecycle for ProgressClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.progress: client lifecycle.activate")
    }
}

impl Handler for ProgressClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.progress: client handler.on_events")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use serde_json::{json, Value};

    /// The shipped manifest, read at compile time so the tests check the same bytes the host loads.
    fn shipped_manifest() -> Value {
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json is valid JSON")
    }

    fn array_contains(values: &Value, expected: &str) -> bool {
        values
            .as_array()
            .map(|entries| entries.iter().any(|entry| entry.as_str() == Some(expected)))
            .unwrap_or(false)
    }

    /// True when the manifest contributes the `progress` command, served by the server, with the `wait` alias.
    fn declares_progress_cli(manifest: &Value) -> bool {
        manifest["contributes"]["cli"]
            .as_array()
            .map(|commands| {
                commands.iter().any(|command| {
                    command["name"].as_str() == Some("progress")
                        && command["requires_server"].as_bool() == Some(true)
                        && array_contains(&command["aliases"], "wait")
                })
            })
            .unwrap_or(false)
    }

    fn assert_implements_lifecycle_and_handler<T: Lifecycle + Handler + Default>() {}

    /// REQ: manual "Complete manifest" (plugin id `ilium.core.progress`, contract `ilium:plugin@0.1.0`, version 0.1.0).
    #[test]
    fn manifest_identifies_the_progress_plugin() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["api"].as_str(), Some("ilium:plugin@0.1.0"));
        assert_eq!(manifest["id"].as_str(), Some("ilium.core.progress"));
        assert_eq!(manifest["version"].as_str(), Some("0.1.0"));
    }

    /// REQ: manual "Complete manifest" (activation is `startup`, following the startup and lazy activation rule).
    #[test]
    fn activation_is_startup() {
        let manifest = shipped_manifest();
        assert!(array_contains(&manifest["activation"], "startup"));
    }

    /// REQ: manual "Complete manifest" (server world `ilium:server/base@0.1.0`, client world `ilium:client/base@0.1.0`).
    #[test]
    fn each_side_names_its_entry_and_world() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["server"]["entry"].as_str(), Some("server.wasm"));
        assert_eq!(manifest["server"]["world"].as_str(), Some("ilium:server/base@0.1.0"));
        assert_eq!(manifest["client"]["entry"].as_str(), Some("client.wasm"));
        assert_eq!(manifest["client"]["world"].as_str(), Some("ilium:client/base@0.1.0"));
    }

    /// REQ: COR-050 (manual "Complete manifest": the `cli` key matches the Manifest fragment; the `progress`
    /// command is served by the server and `ilium wait` works as an alias of `ilium progress wait`).
    #[test]
    fn progress_cli_is_served_by_the_server_with_wait_alias() {
        let manifest = shipped_manifest();
        assert!(declares_progress_cli(&manifest));
        let command = &manifest["contributes"]["cli"][0];
        assert_eq!(command["summary"].as_str(), Some("Progress monitors"));
        assert_eq!(command["output"].as_str(), Some("jsonl"));
    }

    /// REQ: COR-050 (empty or mis-aliased input must not satisfy the progress CLI rule; without the `wait`
    /// alias `ilium wait` would not resolve, so the check fails).
    #[test]
    fn cli_check_rejects_empty_and_mis_aliased_manifests() {
        assert!(!declares_progress_cli(&json!({})));
        let mis_aliased = json!({
            "contributes": {
                "cli": [{ "name": "progress", "requires_server": true, "aliases": ["unwait"] }]
            }
        });
        assert!(!declares_progress_cli(&mis_aliased));
    }

    /// REQ: manual "Complete manifest" (server capabilities: cli.contribute, process.run:sh, process.long-lived,
    /// http-api.route:/progress.json, settings.read-kernel:agent-monitoring).
    #[test]
    fn server_grants_cover_cli_process_route_and_settings() {
        let manifest = shipped_manifest();
        let grants = &manifest["capabilities"]["server"];
        assert!(array_contains(grants, "cli.contribute"));
        assert!(array_contains(grants, "process.run:sh"));
        assert!(array_contains(grants, "process.long-lived"));
        assert!(array_contains(grants, "http-api.route:/progress.json"));
        assert!(array_contains(grants, "settings.read-kernel:agent-monitoring"));
    }

    /// REQ: manual "Complete manifest" (client capabilities: ui.chrome for the footer and status bar segment,
    /// and the read-only agent-monitoring settings kernel that the Agent Monitoring tab writes).
    #[test]
    fn client_grants_cover_footer_chrome_and_settings_read() {
        let manifest = shipped_manifest();
        let grants = &manifest["capabilities"]["client"];
        assert!(array_contains(grants, "ui.chrome"));
        assert!(array_contains(grants, "settings.read-kernel:agent-monitoring"));
    }

    /// REQ: manual "Behaviour contract" intro (the plugin runs on both sides: the server owns the CLI, probes and
    /// watchers; the client draws the footer). Each side must implement the lifecycle and the handler.
    #[test]
    fn each_side_implements_lifecycle_and_handler() {
        assert_implements_lifecycle_and_handler::<ProgressServer>();
        assert_implements_lifecycle_and_handler::<ProgressClient>();
    }

    /// REQ: progress-B01, progress-B02 (exit status and outcome of `progress set --wait` and `progress wait`).
    /// These pin the manual's table; the implementation is still a skeleton, so this does not exercise it.
    #[test]
    fn settled_outcomes_map_to_documented_exit_codes() {
        let outcome_exit_codes: [(&str, i32); 6] = [
            ("done", 0),
            ("error", 3),
            ("monitor-failed", 4),
            ("cleared", 5),
            ("no-monitor", 5),
            ("still-running", 6),
        ];
        let exit_code_of = |outcome: &str| {
            outcome_exit_codes
                .iter()
                .find(|(name, _)| *name == outcome)
                .map(|(_, code)| *code)
        };
        assert_eq!(exit_code_of("done"), Some(0));
        assert_eq!(exit_code_of("error"), Some(3));
        assert_eq!(exit_code_of("still-running"), Some(6));
        assert_eq!(exit_code_of("unknown-outcome"), None);
        let success_outcomes = outcome_exit_codes.iter().filter(|(_, code)| *code == 0).count();
        assert_eq!(success_outcomes, 1, "only `done` exits 0");
    }

    /// REQ: progress-B05, progress-B06, progress-B07, progress-B08 (monitor cap, interval range, probe size
    /// limits, probe timeout). These pin the manual's numbers; they do not exercise the implementation.
    #[test]
    fn probe_limits_match_the_manual() {
        const MAX_MONITORS_PER_PANE: usize = 8;
        const DEFAULT_INTERVAL_SECONDS: u64 = 1;
        const INTERVAL_SECONDS_RANGE: std::ops::RangeInclusive<u64> = 1..=86_400;
        const PROBE_TIMEOUT_MILLIS: u64 = 30_000;
        const PROBE_MESSAGE_MAX_BYTES: usize = 2048;
        const PROBE_DETAILS_MAX_BYTES: usize = 8192;
        assert_eq!(MAX_MONITORS_PER_PANE, 8);
        assert!(INTERVAL_SECONDS_RANGE.contains(&DEFAULT_INTERVAL_SECONDS));
        assert_eq!(PROBE_TIMEOUT_MILLIS, 30_000);
        assert_eq!(PROBE_MESSAGE_MAX_BYTES, 2048);
        assert_eq!(PROBE_DETAILS_MAX_BYTES, 8192);
    }
}
