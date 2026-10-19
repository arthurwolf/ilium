//! Shared helpers: the server host, panes with agents, event builders and submission readers.

use std::path::PathBuf;

use ilium_plugin_core_notifications::NotificationsServer;
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentActivity, AgentActivityEvent, AgentView, Event, KernelSettingsEvent, NodeClass, NodeId,
    Submission,
};
use serde_json::Value;

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/notifications).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
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
