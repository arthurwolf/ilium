//! Shared helpers of the port twins: hosts for both sides, screens, process lists, readers.

use std::path::PathBuf;

use ilium_plugin_core_agent_detect::{AgentDetectClient, AgentDetectServer};
use ilium_plugin_sdk::generated::exports::detector::{
    Classification, DetectorProcess, DetectorScreen,
};
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{AgentActivity, AgentView, Intent, NodeClass, NodeId, Submission};
use ilium_plugin_sdk::Detector;

/// Path of the plugin manifest.
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
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
    AgentDetectServer.classify(signature.to_string(), screen_from_text(text))
}

/// The activity `classify` reports for `text`.
pub fn activity(signature: &str, text: &str) -> AgentActivity {
    classify_text(signature, text).activity
}

/// The goal `classify` reports for `text`.
pub fn goal(signature: &str, text: &str) -> Option<String> {
    classify_text(signature, text).goal
}

/// True for the activities that mean "the agent is not at rest": the WIT `agent-activity` has six cases only, so the
/// manual does not say how waiting-background, background-task-still-running and settling map; they must not be
/// reported as idle, done or none.
pub fn is_busy(activity: &AgentActivity) -> bool {
    !matches!(
        activity,
        AgentActivity::Idle | AgentActivity::Done | AgentActivity::None
    )
}

/// A process of the pane's tree as the kernel lists it for `identify`.
pub fn process(name: &str, argv: &[&str]) -> DetectorProcess {
    DetectorProcess {
        name: name.to_string(),
        argv: argv.iter().map(|argument| argument.to_string()).collect(),
    }
}

/// A process whose name is the lowercase file name of `argv[0]`, like the kernel reports it.
pub fn launched(argv: &[&str]) -> DetectorProcess {
    let name = argv[0].rsplit('/').next().unwrap_or(argv[0]).to_string();
    process(&name, argv)
}

/// Runs `identify` of the server side on a list of processes.
pub fn identify(processes: Vec<DetectorProcess>) -> Option<String> {
    AgentDetectServer.identify(processes)
}

/// Runs `identify` on the single process `argv`.
pub fn identify_argv(argv: &[&str]) -> Option<String> {
    identify(vec![launched(argv)])
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

/// The braille spinner frames of a working agent (shared with the features suite).
pub const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
