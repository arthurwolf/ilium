//! Fake tree, panes and agents (reference-test-harness.md "Fake tree, panes, agents and jobs").

use ilium_plugin_sdk_core::types::{AgentActivity, AgentView, NodeClass};

/// A tree node as tests and fixtures see it; ids start at 1 (DEV-471).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TreeNode {
    /// Node id; assigned by `add`.
    pub id: u64,
    /// Parent id, `None` for a root.
    pub parent: Option<u64>,
    /// Display name.
    pub name: String,
    /// Container, pane or folder.
    pub class: NodeClass,
    /// WIT content kind such as "ilium.terminal".
    pub content_kind: Option<String>,
    /// Agent data when an agent runs in the pane.
    pub agent: Option<AgentView>,
}

/// Handle on the fake tree of a host.
#[derive(Debug)]
pub struct FakeTree {
    _private: (),
}

impl FakeTree {
    /// Adds a node, delivers `tree-changed` and returns the new id.
    pub fn add(&mut self, _node: TreeNode) -> u64 {
        unimplemented!("DEV-470")
    }

    /// Renames a node and delivers `tree-changed`.
    pub fn rename(&mut self, _id: u64, _name: &str) {
        unimplemented!("DEV-470")
    }

    /// Delivers `agent-activity-changed` with `revision` plus 1.
    pub fn set_activity(&mut self, _id: u64, _activity: AgentActivity) {
        unimplemented!("DEV-470")
    }

    /// Removes a node and its children and delivers `node-removed`.
    pub fn remove(&mut self, _id: u64) {
        unimplemented!("DEV-470")
    }

    /// Reads a node.
    pub fn get(&self, _id: u64) -> Option<TreeNode> {
        unimplemented!("DEV-470")
    }

    /// Adds a group node under the root and returns its id (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn add_group(&mut self, _name: &str) -> u64 {
        unimplemented!("DEV-470")
    }

    /// Adds a pane running an agent of `signature` (`"claude"`, `"codex"`) and returns its id (lane hook, see manual_gaps_fakes.md).
    pub fn add_agent_pane(&mut self, _name: &str, _signature: &str) -> u64 {
        unimplemented!("DEV-470")
    }

    /// The rows `terminal.read-screen` returns for a pane (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_screen(&mut self, _id: u64, _rows: Vec<String>) {
        unimplemented!("DEV-470")
    }

    /// `terminal.read-screen` answers `None` for the pane (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_screen_unavailable(&mut self, _id: u64) {
        unimplemented!("DEV-470")
    }

    /// The verified agent session of a pane and its transcript lines (lane hook, see manual_gaps_fakes.md).
    pub fn set_agent_session(&mut self, _id: u64, _session_id: &str, _transcript: Vec<String>) {
        unimplemented!("DEV-470")
    }

    /// Records a goal on an agent pane; `status` is active, paused, blocked, done or unknown (lane hook, see manual_gaps_fakes.md).
    pub fn set_agent_goal(&mut self, _id: u64, _objective: &str, _status: &str) {
        unimplemented!("DEV-470")
    }

    /// The working directory of a pane, in `"scope:path"` shorthand (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_pane_directory(&mut self, _id: u64, _directory: &str) {
        unimplemented!("DEV-470")
    }

    /// The plugin metadata bytes stored on a node (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn metadata(&self, _id: u64) -> Option<Vec<u8>> {
        unimplemented!("DEV-470")
    }

    /// Stores plugin metadata bytes on a node (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_metadata(&mut self, _id: u64, _bytes: Vec<u8>) {
        unimplemented!("DEV-470")
    }

    /// Locks or unlocks a node against structural edits (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_locked(&mut self, _id: u64, _locked: bool) {
        unimplemented!("DEV-470")
    }

    /// Marks a pane as living in a Git worktree (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_worktree(&mut self, _id: u64, _worktree: bool) {
        unimplemented!("DEV-470")
    }
}
