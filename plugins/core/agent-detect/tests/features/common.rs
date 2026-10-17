//! Shared helpers: hosts for both sides, screens built from the captured detector fixtures, readers.

pub use crate::host_ext::*;
use std::path::PathBuf;

use ilium_plugin_core_agent_detect::{AgentDetectClient, AgentDetectServer};
use ilium_plugin_sdk::generated::exports::detector::{
    Classification, DetectorProcess, DetectorScreen,
};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{AgentActivity, AgentView, Intent, NodeClass, NodeId, Submission};
use ilium_plugin_sdk::Detector;

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/agent-detect).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-agent-detect.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// The feature traceability page of the manual.
pub fn traceability_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/reference-feature-traceability.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// A captured screen of the existing detector suite (ilium-detect/tests/fixtures).
pub fn fixture(name: &str) -> String {
    let path = workspace_root()
        .join("ilium-detect/tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("fixture {path:?}: {error}"))
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<AgentDetectServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<AgentDetectClient>();
    host
}

/// Builds the detector screen the kernel hands to `classify` from plain screen text.
pub fn screen_from_text(text: &str) -> DetectorScreen {
    let rows: Vec<String> = text.lines().map(str::to_string).collect();
    let columns = rows
        .iter()
        .map(|row| row.chars().count())
        .max()
        .unwrap_or(0)
        .max(80);
    let cursor_row = u16::try_from(rows.len().saturating_sub(1)).unwrap_or(u16::MAX);
    DetectorScreen {
        columns: u16::try_from(columns).unwrap_or(u16::MAX),
        rows,
        cursor_row,
        title: None,
        revision: 1,
    }
}

/// Runs `classify` of the server side on a screen text.
pub fn classify_text(signature: &str, text: &str) -> Classification {
    AgentDetectServer::default().classify(signature.to_string(), screen_from_text(text))
}

/// Runs `classify` on a captured fixture and returns the activity.
pub fn activity_of(signature: &str, fixture_name: &str) -> AgentActivity {
    classify_text(signature, &fixture(fixture_name)).activity
}

/// Runs `classify` on a captured fixture and returns the goal text.
pub fn goal_of(signature: &str, fixture_name: &str) -> Option<String> {
    classify_text(signature, &fixture(fixture_name)).goal
}

/// A process of the pane's tree as the kernel lists it for `identify`.
pub fn process(name: &str, argv: &[&str]) -> DetectorProcess {
    DetectorProcess {
        name: name.to_string(),
        argv: argv.iter().map(|argument| argument.to_string()).collect(),
    }
}

/// Runs `identify` of the server side on a list of processes.
pub fn identify(processes: Vec<DetectorProcess>) -> Option<String> {
    AgentDetectServer::default().identify(processes)
}

/// Runs `identify` on one process named `name` with `argv = [name]`.
pub fn identify_name(name: &str) -> Option<String> {
    identify(vec![process(name, &[name])])
}

/// Adds a terminal pane carrying an agent to the fake tree of the host.
pub fn add_agent_pane(
    host: &mut FakeHost,
    title: &str,
    signature: &str,
    activity: AgentActivity,
    goal: Option<&str>,
) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: title.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView {
            signature: signature.to_string(),
            activity,
            objective: None,
            goal: goal.map(str::to_string),
        }),
    })
}

/// The plain text of the spans of a styled text.
pub fn plain(content: &[ilium_plugin_sdk::types::Span]) -> String {
    content.iter().map(|span| span.text.as_str()).collect()
}

/// Decoration specs with slot `slot`, in submission order.
pub fn decorations(
    submissions: &[Submission],
    slot: &str,
) -> Vec<ilium_plugin_sdk::types::SetDecorationSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SetDecoration(spec) if spec.slot == slot => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// Node ids of `clear-decoration` intents with slot `slot`.
pub fn cleared(submissions: &[Submission], slot: &str) -> Vec<NodeId> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::ClearDecoration(key) if key.slot == slot => Some(key.node),
            _ => None,
        })
        .collect()
}

/// The ten braille frames of the working spinner the page shows.
pub const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
