//! Shared helpers of the ports suite: server host, panes with agents, event builders, readers.

use std::path::PathBuf;

use ilium_plugin_core_notifications::NotificationsServer;
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentActivityEvent, AgentView, Event, Intent, KernelSettingsEvent,
    MarkAttentionSpec, NodeClass, NodeId, NotifySpec, PluginId, Submission, TopicPublishedEvent,
};
use serde_json::{json, Value};

/// Topic of the Progress plugin that reports the state of a monitored task
/// (reference-core-extension-points.md: `ilium.core.progress/changed`).
pub const PROGRESS_TOPIC: &str = "ilium.core.progress/changed";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<NotificationsServer>();
    host
}

/// Writes one setting of the `sound` tab and tells the plugin which key changed.
pub fn set_setting(host: &mut FakeHost, key: &str, value: Value) {
    host.settings().set(key, value).expect("setting accepted");
    host.deliver_one(Event::KernelSettingsChanged(KernelSettingsEvent {
        tab: "sound".to_string(),
        keys: vec![key.to_string()],
    }));
    host.settle();
}

/// Adds a pane to the fake tree; `activity` makes it an agent pane.
pub fn add_pane(host: &mut FakeHost, name: &str, activity: Option<AgentActivity>) -> NodeId {
    let id = host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: activity.map(|activity| AgentView {
            signature: "claude".to_string(),
            activity,
            objective: None,
            goal: None,
        }),
    });
    NodeId(id)
}

/// Moves the agent of `node` from `previous` to `current` and delivers the event.
pub fn transition(
    host: &mut FakeHost,
    node: NodeId,
    previous: AgentActivity,
    current: AgentActivity,
    revision: u64,
) -> Vec<Submission> {
    host.tree().set_activity(node.0, current);
    let submissions = host.deliver_one(Event::AgentActivityChanged(AgentActivityEvent {
        node,
        previous,
        current,
        revision,
    }));
    host.settle();
    submissions
}

/// A pane with an agent that is working, and the submissions of its finishing.
pub fn finish_turn(host: &mut FakeHost, node: NodeId, revision: u64) -> Vec<Submission> {
    transition(
        host,
        node,
        AgentActivity::Working,
        AgentActivity::Done,
        revision,
    )
}

/// The progress topic event of a monitored task.
///
/// Assumed payload (API_GAPS.md): `{ monitor, pane, job_id, status, percent }` with `status` one of
/// `running`, `done`, `error`, `lost`.
pub fn task_event(pane: NodeId, job_id: &str, status: &str) -> Event {
    let percent = if status == "running" { 50.0 } else { 100.0 };
    Event::TopicPublished(TopicPublishedEvent {
        topic: PROGRESS_TOPIC.to_string(),
        from: PluginId("ilium.core.progress".to_string()),
        payload_json: json!({
            "monitor": 1, "pane": pane.0, "job_id": job_id, "status": status, "percent": percent
        })
        .to_string(),
    })
}

/// Delivers one task outcome and returns the submissions.
pub fn task_outcome(
    host: &mut FakeHost,
    pane: NodeId,
    job_id: &str,
    status: &str,
) -> Vec<Submission> {
    let submissions = host.deliver_one(task_event(pane, job_id, status));
    host.settle();
    submissions
}

/// The desktop notices among the submissions.
pub fn notices(submissions: &[Submission]) -> Vec<NotifySpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::Notify(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// The sound ids among the submissions.
pub fn sounds(submissions: &[Submission]) -> Vec<String> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::PlaySound(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// The attention marks among the submissions.
pub fn marks(submissions: &[Submission]) -> Vec<MarkAttentionSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::MarkAttention(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// A pane like `add_pane`, with an agent `signature` and an optional goal.
pub fn add_pane_full(
    host: &mut FakeHost,
    name: &str,
    signature: &str,
    activity: Option<AgentActivity>,
    objective: Option<&str>,
    goal: Option<&str>,
) -> NodeId {
    let id = host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: activity.map(|activity| AgentView {
            signature: signature.to_string(),
            activity,
            objective: objective.map(str::to_string),
            goal: goal.map(str::to_string),
        }),
    });
    NodeId(id)
}

/// An agent pane with a goal.
pub fn add_pane_with_goal(
    host: &mut FakeHost,
    name: &str,
    activity: Option<AgentActivity>,
    goal: Option<&str>,
) -> NodeId {
    add_pane_full(host, name, "claude", activity, None, goal)
}

/// An agent pane of the given signature.
pub fn add_pane_signature(
    host: &mut FakeHost,
    name: &str,
    signature: &str,
    activity: AgentActivity,
) -> NodeId {
    add_pane_full(host, name, signature, Some(activity), None, None)
}

/// A transition event after which the agent holds `goal` (the tree view is not touched for the goal).
pub fn transition_with_goal(
    host: &mut FakeHost,
    node: NodeId,
    previous: AgentActivity,
    current: AgentActivity,
    revision: u64,
    _goal: Option<&str>,
) -> Vec<Submission> {
    transition(host, node, previous, current, revision)
}
