//! Core plugin `ilium.core.agent-detect` (manual page core-plugin-agent-detect.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::exports::detector::Classification;
use ilium_plugin_sdk::generated::exports::detector::DetectorProcess;
use ilium_plugin_sdk::generated::exports::detector::DetectorScreen;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Detector, Handler, Lifecycle};

/// The server side of `ilium.core.agent-detect` (core-plugin-agent-detect.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["detector"])]
#[derive(Default)]
pub struct AgentDetectServer;

impl Lifecycle for AgentDetectServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.agent-detect: server lifecycle.activate")
    }
}

impl Handler for AgentDetectServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.agent-detect: server handler.on_events")
    }
}

impl Detector for AgentDetectServer {
    fn identify(&mut self, _processes: Vec<DetectorProcess>) -> Option<String> {
        unimplemented!("ilium.core.agent-detect: server detector.identify")
    }

    fn classify(&mut self, _signature: String, _screen: DetectorScreen) -> Classification {
        unimplemented!("ilium.core.agent-detect: server detector.classify")
    }
}

/// The client side of `ilium.core.agent-detect` (core-plugin-agent-detect.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct AgentDetectClient;

impl Lifecycle for AgentDetectClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.agent-detect: client lifecycle.activate")
    }
}

impl Handler for AgentDetectClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.agent-detect: client handler.on_events")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use ilium_plugin_sdk::generated::types::tree::AgentActivity;

    fn process_named(name: &str) -> DetectorProcess {
        DetectorProcess {
            name: name.to_string(),
            argv: vec![name.to_string()],
        }
    }

    fn screen_with_rows(rows: &[&str]) -> DetectorScreen {
        DetectorScreen {
            rows: rows.iter().map(|row| row.to_string()).collect(),
            ..Default::default()
        }
    }

    /// REQ: core-plugin-agent-detect.md "What it covers", agent identity from the process tree (the page carries no bracket ID here)
    #[test]
    fn identify_returns_none_for_an_empty_process_list() {
        let mut server = AgentDetectServer::default();
        assert_eq!(server.identify(Vec::new()), None);
    }

    /// REQ: core-plugin-agent-detect.md "Enums the plugin reports", agent class `codex` (the page carries no bracket ID here)
    #[test]
    fn identify_recognises_a_codex_process_in_the_tree() {
        let mut server = AgentDetectServer::default();
        let identified = server.identify(vec![process_named("bash"), process_named("codex")]);
        assert!(identified.is_some());
    }

    /// REQ: core-plugin-agent-detect.md "Enums the plugin reports", pane kind `plain-shell` is not an agent (failure case: no agent in the tree)
    #[test]
    fn identify_returns_none_when_only_a_plain_shell_runs() {
        let mut server = AgentDetectServer::default();
        assert_eq!(server.identify(vec![process_named("bash"), process_named("vim")]), None);
    }

    /// REQ: core-plugin-agent-detect.md "Samples", identify-and-classify sample (approval prompt maps to `awaiting-approval`)
    #[test]
    fn classify_reports_awaiting_approval_when_the_approval_question_is_shown() {
        let mut server = AgentDetectServer::default();
        let classification = server.classify(
            "example-agent".to_string(),
            screen_with_rows(&["Allow this command?", "> yes"]),
        );
        assert_eq!(classification.activity, AgentActivity::AwaitingApproval);
    }

    /// REQ: core-plugin-agent-detect.md "Enums the plugin reports", activity `working` (the page carries no bracket ID here)
    #[test]
    fn classify_reports_working_while_the_interrupt_hint_is_shown() {
        let mut server = AgentDetectServer::default();
        let classification = server.classify(
            "example-agent".to_string(),
            screen_with_rows(&["Thinking about the parser", "esc to interrupt"]),
        );
        assert_eq!(classification.activity, AgentActivity::Working);
    }

    /// REQ: core-plugin-agent-detect.md "Enums the plugin reports", activity `idle` when no working or approval marker is present (the page carries no bracket ID here)
    #[test]
    fn classify_reports_idle_when_no_marker_is_shown() {
        let mut server = AgentDetectServer::default();
        let classification = server.classify(
            "example-agent".to_string(),
            screen_with_rows(&["> ready for the next task"]),
        );
        assert_eq!(classification.activity, AgentActivity::Idle);
    }

    /// REQ: core-plugin-agent-detect.md "Samples", classify order (an approval question takes precedence over the working hint)
    #[test]
    fn classify_prefers_awaiting_approval_over_working_when_both_are_shown() {
        let mut server = AgentDetectServer::default();
        let classification = server.classify(
            "example-agent".to_string(),
            screen_with_rows(&["esc to interrupt", "Allow this command?"]),
        );
        assert_eq!(classification.activity, AgentActivity::AwaitingApproval);
    }

    /// REQ: core-plugin-agent-detect.md "Samples", classify returns no objective or goal for a plain screen (the page carries no bracket ID here)
    #[test]
    fn classify_leaves_objective_and_goal_unset_for_a_working_screen() {
        let mut server = AgentDetectServer::default();
        let classification = server.classify(
            "example-agent".to_string(),
            screen_with_rows(&["esc to interrupt"]),
        );
        assert_eq!(classification.objective, None);
        assert_eq!(classification.goal, None);
    }

    /// REQ: core-plugin-agent-detect.md "Samples", empty input screen falls through to `idle` (empty-input case)
    #[test]
    fn classify_an_empty_screen_is_idle() {
        let mut server = AgentDetectServer::default();
        let classification = server.classify("example-agent".to_string(), DetectorScreen::default());
        assert_eq!(classification.activity, AgentActivity::Idle);
    }

    /// REQ: core-plugin-agent-detect.md "Enums the plugin reports", activity enum values in manual order (the page carries no bracket ID here)
    #[test]
    fn activity_enum_case_names_match_the_manual_list() {
        assert_eq!(
            AgentActivity::CASE_NAMES,
            &["none", "working", "idle", "awaiting-approval", "done", "menu"]
        );
    }
}
