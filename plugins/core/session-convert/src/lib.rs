//! Core plugin `ilium.core.session-convert` (manual page core-plugin-session-convert.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::exports::menu_provider::MenuContext;
use ilium_plugin_sdk::generated::exports::menu_provider::MenuEntry;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceCall;
use ilium_plugin_sdk::generated::exports::service_provider::ServiceReply;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{
    ilium_plugin, Handler, JobRunner, Lifecycle, MenuProvider, ServiceProvider, Views,
};

/// The server side of `ilium.core.session-convert` (core-plugin-session-convert.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["job-runner", "service-provider"])]
#[derive(Default)]
pub struct SessionConvertServer;

impl Lifecycle for SessionConvertServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.session-convert: server lifecycle.activate")
    }
}

impl Handler for SessionConvertServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.session-convert: server handler.on_events")
    }
}

impl JobRunner for SessionConvertServer {
    fn run(&mut self, _kind: String, _input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        unimplemented!("ilium.core.session-convert: server job-runner.run")
    }
}

impl ServiceProvider for SessionConvertServer {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        unimplemented!("ilium.core.session-convert: server service-provider.handle")
    }
}

/// The client side of `ilium.core.session-convert` (core-plugin-session-convert.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views", "menu-provider"])]
#[derive(Default)]
pub struct SessionConvertClient;

impl Lifecycle for SessionConvertClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.session-convert: client lifecycle.activate")
    }
}

impl Handler for SessionConvertClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.session-convert: client handler.on_events")
    }
}

impl Views for SessionConvertClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.session-convert: client views.render")
    }
}

impl MenuProvider for SessionConvertClient {
    fn menu_items(&mut self, _context: MenuContext) -> Vec<MenuEntry> {
        unimplemented!("ilium.core.session-convert: client menu-provider.menu_items")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    const MANIFEST_JSON: &str = include_str!("../manifest.json");

    fn manifest() -> serde_json::Value {
        serde_json::from_str(MANIFEST_JSON).expect("manifest.json parses as JSON")
    }

    fn string_list(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .expect("value is a JSON array")
            .iter()
            .map(|item| item.as_str().expect("array item is a string").to_string())
            .collect()
    }

    /// Manual: the complete manifest names the plugin id, the api version and the display name.
    /// REQ: COR-050
    #[test]
    fn manifest_declares_the_plugin_id_api_and_name() {
        let manifest_value = manifest();
        assert_eq!(manifest_value["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest_value["id"], "ilium.core.session-convert");
        assert_eq!(manifest_value["publisher"], "Ilium");
        assert_eq!(manifest_value["name"], "Session conversion");
        assert_eq!(manifest_value["replaceable"], true);
    }

    /// Manual: both sides exist, with their worlds and the exports each side lists.
    /// REQ: COR-050
    #[test]
    fn sides_worlds_and_exports_match_the_manual() {
        let _server = SessionConvertServer::default();
        let _client = SessionConvertClient::default();
        let manifest_value = manifest();
        assert_eq!(manifest_value["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(
            string_list(&manifest_value["server"]["exports"]),
            vec!["job-runner", "service-provider"]
        );
        assert_eq!(manifest_value["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(
            string_list(&manifest_value["client"]["exports"]),
            vec!["views", "menu-provider"]
        );
    }

    /// Manual: activation triggers and the surface list are part of the complete manifest.
    /// REQ: COR-050
    #[test]
    fn activation_triggers_and_surface_match_the_manual() {
        let manifest_value = manifest();
        assert_eq!(
            string_list(&manifest_value["activation"]),
            vec![
                "command:convert",
                "service:convert",
                "view:session-convert.progress"
            ]
        );
        assert_eq!(
            string_list(&manifest_value["surface"]),
            vec![
                "services:convert",
                "points:formats",
                "points:after-convert",
                "commands:convert"
            ]
        );
    }

    /// Manual: the capability grants of each side are exactly the grants listed on the page.
    /// REQ: COR-050
    #[test]
    fn capability_grants_match_the_manual_complete_manifest() {
        let manifest_value = manifest();
        let mut server_grants = string_list(&manifest_value["capabilities"]["server"]);
        server_grants.sort();
        let mut expected_server_grants = vec![
            "agent-config.propose:claude",
            "agent.read",
            "extensions.provide:ilium.core.session-convert/after-convert",
            "extensions.provide:ilium.core.session-convert/formats",
            "fs.read:agent-config:claude",
            "fs.read:agent-sessions:claude",
            "fs.read:agent-sessions:codex",
            "jobs.run",
            "notify.toast",
            "panes.create",
            "panes.create-command:claude",
            "panes.create-command:codex",
            "panes.lifecycle",
            "panes.read",
            "process.long-lived",
            "process.run:codex",
            "services.provide:ilium.core.session-convert/convert",
            "tree.write",
        ];
        expected_server_grants.sort();
        assert_eq!(server_grants, expected_server_grants);

        let mut client_grants = string_list(&manifest_value["capabilities"]["client"]);
        client_grants.sort();
        assert_eq!(client_grants, vec!["agent.read", "ui.panels"]);
    }

    /// Manual: the plugin holds no write grant on Claude's folders; it proposes the transcript
    /// through agent-config.propose, and the original transcript is read and never changed.
    /// REQ: COR-011
    #[test]
    fn plugin_holds_no_write_grant_on_agent_folders() {
        let manifest_value = manifest();
        let grants = string_list(&manifest_value["capabilities"]["server"]);
        let write_grants: Vec<&String> = grants
            .iter()
            .filter(|grant| grant.starts_with("fs.write:"))
            .collect();
        assert!(
            write_grants.is_empty(),
            "unexpected write grants: {write_grants:?}"
        );
        assert!(grants.contains(&"agent-config.propose:claude".to_string()));
        assert!(grants.contains(&"fs.read:agent-sessions:claude".to_string()));
        assert!(grants.contains(&"fs.read:agent-sessions:codex".to_string()));
    }

    /// Manual: the convert service takes a pane and only the codex or claude target; antigravity
    /// sessions cannot be converted, so an invalid target fails the schema.
    /// REQ: F-077
    #[test]
    fn convert_service_takes_pane_and_only_codex_or_claude_targets() {
        let manifest_value = manifest();
        let start = &manifest_value["contributes"]["services"]
            ["ilium.core.session-convert/convert"]["start"];
        let params = &start["params"];
        assert_eq!(string_list(&params["required"]), vec!["pane", "target"]);
        assert_eq!(params["properties"]["pane"]["type"], "integer");
        let target_values = string_list(&params["properties"]["target"]["enum"]);
        assert_eq!(target_values, vec!["codex", "claude"]);
        assert!(!target_values.contains(&"antigravity".to_string()));
        assert_eq!(
            string_list(&start["result"]["required"]),
            vec!["conversion"]
        );
    }

    /// Manual: the codex-to-claude job runs under a 60 second deadline and a 256 MiB memory limit.
    /// REQ: COR-050
    #[test]
    fn codex_to_claude_job_has_its_deadline_and_memory_limit() {
        let manifest_value = manifest();
        let job = &manifest_value["contributes"]["jobs"][0];
        assert_eq!(job["kind"], "codex-to-claude");
        assert_eq!(job["deadline_ms"], 60000);
        assert_eq!(job["memory_mib"], 256);
    }

    /// Manual: formats collects at most 8 entries; after-convert is an observe-only hook with a
    /// 20 ms deadline and at most 8 handlers.
    /// REQ: COR-050
    #[test]
    fn extension_points_match_the_manual_limits() {
        let manifest_value = manifest();
        let points = &manifest_value["contributes"]["extension_points"];
        let formats = &points[0];
        assert_eq!(formats["id"], "formats");
        assert_eq!(formats["kind"], "collect");
        assert_eq!(formats["max_entries"], 8);
        assert_eq!(formats["dynamic"], false);
        assert_eq!(
            string_list(&formats["entry_schema"]["required"]),
            vec!["id", "title", "direction", "service"]
        );
        let hook = &points[1];
        assert_eq!(hook["id"], "after-convert");
        assert_eq!(hook["kind"], "hook");
        assert_eq!(hook["mode"], "observe");
        assert_eq!(hook["risk"], "standard");
        assert_eq!(hook["deadline_ms"], 20);
        assert_eq!(hook["max_handlers"], 8);
        assert_eq!(
            hook["payload_schema"]["properties"]["messages"]["type"],
            "integer"
        );
        assert_eq!(
            hook["payload_schema"]["properties"]["path"]["type"],
            "string"
        );
    }

    /// Manual: the Convert session command appears as the pane context menu entry and opens the
    /// modal Convert session progress panel, which the activation list refers to.
    /// REQ: F-077
    #[test]
    fn command_menu_entry_and_progress_panel_match_the_manual() {
        let manifest_value = manifest();
        let contributes = &manifest_value["contributes"];
        assert_eq!(contributes["commands"][0]["id"], "convert");
        assert_eq!(contributes["commands"][0]["title"], "Convert session");
        assert_eq!(contributes["commands"][0]["category"], "Agents");
        let menu_entry = &contributes["context_menu"][0];
        assert_eq!(menu_entry["id"], "pane-convert");
        assert_eq!(string_list(&menu_entry["applies_to"]), vec!["pane"]);
        assert_eq!(menu_entry["dynamic"], true);
        assert_eq!(menu_entry["command"], "convert");
        let panel = &contributes["panels"][0];
        assert_eq!(panel["id"], "progress");
        assert_eq!(panel["title"], "Convert session");
        assert_eq!(panel["modal"], true);
        assert!(string_list(&manifest_value["activation"])
            .contains(&"view:session-convert.progress".to_string()));
    }
}
