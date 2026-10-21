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

    /// Moves a node under another parent and delivers `tree-changed` (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn move_node(&mut self, _id: u64, _new_parent: u64) {
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

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::FakeHost;

    fn pane_node(name: &str, parent: Option<u64>) -> TreeNode {
        TreeNode {
            id: 0,
            parent,
            name: name.to_string(),
            class: NodeClass::Pane,
            content_kind: Some("ilium.terminal".to_string()),
            agent: None,
        }
    }

    fn agent_pane_node(name: &str) -> TreeNode {
        TreeNode {
            agent: Some(AgentView {
                signature: "claude".to_string(),
                activity: AgentActivity::Idle,
                objective: None,
                goal: None,
            }),
            ..pane_node(name, None)
        }
    }

    /// REQ: DEV-471 (ids are assigned in increasing order from 1)
    #[test]
    fn dev_471_first_two_nodes_get_ids_one_and_two() {
        let mut host = FakeHost::new();
        let first_id = host.tree().add(pane_node("shell", None));
        let second_id = host.tree().add(pane_node("editor", None));
        assert_eq!(first_id, 1);
        assert_eq!(second_id, 2);
    }

    /// REQ: DEV-471 (a caller-supplied id is replaced; ids never reuse a removed id)
    #[test]
    fn dev_471_ids_ignore_supplied_value_and_are_not_reused_after_remove() {
        let mut host = FakeHost::new();
        let first_id = host.tree().add(TreeNode { id: 99, ..pane_node("a", None) });
        host.tree().remove(first_id);
        let next_id = host.tree().add(pane_node("b", None));
        assert_eq!(first_id, 1);
        assert_eq!(next_id, 2);
        assert_eq!(host.tree().get(next_id).map(|node| node.id), Some(2));
    }

    /// REQ: DEV-470 (add stores the node so that get returns it under the new id)
    #[test]
    fn dev_470_add_then_get_returns_the_node_with_its_parent_and_class() {
        let mut host = FakeHost::new();
        let group_id = host.tree().add(TreeNode {
            class: NodeClass::Container,
            content_kind: None,
            ..pane_node("workspace", None)
        });
        let pane_id = host.tree().add(pane_node("shell", Some(group_id)));
        let stored = host.tree().get(pane_id).expect("added node is readable");
        assert_eq!(stored.name, "shell");
        assert_eq!(stored.parent, Some(group_id));
        assert_eq!(stored.class, NodeClass::Pane);
        assert_eq!(stored.content_kind.as_deref(), Some("ilium.terminal"));
    }

    /// REQ: DEV-470 (rename changes the name that get reports)
    #[test]
    fn dev_470_rename_changes_the_name_that_get_reports() {
        let mut host = FakeHost::new();
        let pane_id = host.tree().add(pane_node("shell", None));
        host.tree().rename(pane_id, "build");
        assert_eq!(host.tree().get(pane_id).map(|node| node.name), Some("build".to_string()));
    }

    /// REQ: DEV-470 (set_activity updates the activity of an agent node)
    #[test]
    fn dev_470_set_activity_updates_the_agent_activity_of_the_node() {
        let mut host = FakeHost::new();
        let pane_id = host.tree().add(agent_pane_node("claude-pane"));
        host.tree().set_activity(pane_id, AgentActivity::Working);
        let activity = host
            .tree()
            .get(pane_id)
            .and_then(|node| node.agent)
            .map(|agent| agent.activity);
        assert_eq!(activity, Some(AgentActivity::Working));
    }

    /// REQ: DEV-470 (remove deletes the node and its children and leaves siblings in place)
    #[test]
    fn dev_470_remove_deletes_the_node_and_its_children_only() {
        let mut host = FakeHost::new();
        let group_id = host.tree().add(TreeNode {
            class: NodeClass::Container,
            content_kind: None,
            ..pane_node("group", None)
        });
        let child_id = host.tree().add(pane_node("child", Some(group_id)));
        let sibling_id = host.tree().add(pane_node("sibling", None));
        host.tree().remove(group_id);
        assert!(host.tree().get(group_id).is_none());
        assert!(host.tree().get(child_id).is_none());
        assert!(host.tree().get(sibling_id).is_some());
    }

    /// REQ: DEV-470 (get on an id that was never added, or was removed, returns None)
    #[test]
    fn dev_470_get_on_unknown_or_removed_id_returns_none() {
        let mut host = FakeHost::new();
        assert!(host.tree().get(42).is_none());
        let pane_id = host.tree().add(pane_node("shell", None));
        host.tree().remove(pane_id);
        assert!(host.tree().get(pane_id).is_none());
    }

    /// REQ: DEV-470 (lane hook move_node reparents a node under another parent)
    #[test]
    fn dev_470_move_node_reparents_the_node_under_the_new_parent() {
        let mut host = FakeHost::new();
        let target_id = host.tree().add(TreeNode {
            class: NodeClass::Container,
            content_kind: None,
            ..pane_node("target", None)
        });
        let pane_id = host.tree().add(pane_node("shell", None));
        host.tree().move_node(pane_id, target_id);
        assert_eq!(host.tree().get(pane_id).map(|node| node.parent), Some(Some(target_id)));
    }
}
