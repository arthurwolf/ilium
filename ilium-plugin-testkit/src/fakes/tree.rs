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
}
