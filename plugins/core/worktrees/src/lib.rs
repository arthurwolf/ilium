//! Core plugin `ilium.core.worktrees` (manual page core-plugin-worktrees.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{Area, UiTree, ViewTarget};
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, Views};

/// The server side of `ilium.core.worktrees` (core-plugin-worktrees.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct WorktreesServer;

impl Lifecycle for WorktreesServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.worktrees: server lifecycle.activate")
    }
}

impl Handler for WorktreesServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.worktrees: server handler.on_events")
    }
}

/// The client side of `ilium.core.worktrees` (core-plugin-worktrees.md "runs on both sides", world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct WorktreesClient;

impl Lifecycle for WorktreesClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.worktrees: client lifecycle.activate")
    }
}

impl Handler for WorktreesClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.worktrees: client handler.on_events")
    }
}

impl Views for WorktreesClient {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        unimplemented!("ilium.core.worktrees: client views.render")
    }
}

#[cfg(test)]
mod spec_tests {

    const MANIFEST_JSON: &str = include_str!("../manifest.json");

    fn manifest() -> serde_json::Value {
        serde_json::from_str(MANIFEST_JSON).expect("manifest.json is valid JSON")
    }

    fn string_list(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    }

    fn extension_point<'a>(manifest: &'a serde_json::Value, point_id: &str) -> &'a serde_json::Value {
        manifest["contributes"]["extension_points"]
            .as_array()
            .and_then(|points| points.iter().find(|point| point["id"] == point_id))
            .expect("the extension point is declared in contributes.extension_points")
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_worktrees_identity_and_api_version() {
        let manifest = manifest();
        assert_eq!(manifest["id"], "ilium.core.worktrees");
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn activation_triggers_include_new_agent_command_and_create_service() {
        let activation = string_list(&manifest()["activation"]);
        assert!(activation.iter().any(|item| item == "command:new-agent"));
        assert!(activation.iter().any(|item| item == "service:create"));
    }

    /// REQ: COR-050
    #[test]
    fn server_side_holds_the_git_worktree_and_pane_grants() {
        let manifest = manifest();
        let server = string_list(&manifest["capabilities"]["server"]);
        let required = [
            "workspace.manage",
            "process.run:git",
            "process.run:sh",
            "panes.create",
            "panes.read",
            "fs.read:project",
            "fs.write:worktrees",
        ];
        for grant in required {
            assert!(server.iter().any(|item| item == grant), "server side is missing {grant}");
        }
    }

    /// REQ: COR-050
    #[test]
    fn client_side_holds_no_process_grant_because_git_runs_on_the_server() {
        let manifest = manifest();
        let client = string_list(&manifest["capabilities"]["client"]);
        assert!(client.iter().any(|item| item == "ui.panels"));
        assert!(
            client.iter().all(|item| !item.starts_with("process.run")),
            "the client side must not run processes; Git runs on the server side"
        );
    }

    /// REQ: COR-030
    #[test]
    fn exactly_one_side_holds_the_kernel_writer_grant_for_the_git_tab() {
        let manifest = manifest();
        let holders = ["server", "client"]
            .into_iter()
            .filter(|side| {
                string_list(&manifest["capabilities"][*side])
                    .iter()
                    .any(|item| item == "settings.write-kernel:git")
            })
            .count();
        assert_eq!(holders, 1, "the git Settings tab has one kernel writer");
    }

    /// REQ: EXT-052
    #[test]
    fn three_extension_points_are_declared_with_kind_and_mode() {
        let manifest = manifest();
        assert_eq!(extension_point(&manifest, "before-create")["kind"], "hook");
        assert_eq!(extension_point(&manifest, "before-create")["mode"], "transform");
        assert_eq!(extension_point(&manifest, "post-create-steps")["kind"], "collect");
        assert_eq!(extension_point(&manifest, "before-remove")["kind"], "hook");
        assert_eq!(extension_point(&manifest, "before-remove")["mode"], "veto");
    }

    /// REQ: EXT-052
    #[test]
    fn extension_point_deadlines_and_limits_match_the_manual_table() {
        let manifest = manifest();
        assert_eq!(extension_point(&manifest, "before-create")["deadline_ms"], 20);
        assert_eq!(extension_point(&manifest, "before-create")["max_handlers"], 8);
        assert_eq!(extension_point(&manifest, "post-create-steps")["max_entries"], 64);
        assert_eq!(extension_point(&manifest, "post-create-steps")["dynamic"], false);
        assert_eq!(extension_point(&manifest, "before-remove")["deadline_ms"], 20);
        assert_eq!(extension_point(&manifest, "before-remove")["max_handlers"], 8);
    }

    /// REQ: SEC-293
    #[test]
    fn extension_point_risk_marks_follow_the_corrected_table() {
        let manifest = manifest();
        assert_eq!(extension_point(&manifest, "before-create")["risk"], "standard");
        assert_eq!(extension_point(&manifest, "post-create-steps")["risk"], "high");
        assert_eq!(extension_point(&manifest, "before-remove")["risk"], "standard");
    }

    /// REQ: EXT-052
    #[test]
    fn post_create_entry_schema_requires_id_title_command_and_working_directory() {
        let manifest = manifest();
        let mut required = string_list(&extension_point(&manifest, "post-create-steps")["entry_schema"]["required"]);
        required.sort();
        assert_eq!(required, ["command", "id", "title", "working_directory"]);
    }

    /// REQ: EXT-052
    #[test]
    fn before_create_reply_must_carry_a_branch_name() {
        let manifest = manifest();
        let required = string_list(&extension_point(&manifest, "before-create")["reply_schema"]["required"]);
        assert_eq!(required, ["branch"]);
    }
}
