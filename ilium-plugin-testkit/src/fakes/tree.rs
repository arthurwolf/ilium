//! Fake tree, panes and agents (reference-test-harness.md "Fake tree, panes, agents and jobs").

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use ilium_plugin_sdk_core::generated::types::{AgentActivityEvent, Event, NodeId};
use ilium_plugin_sdk_core::types::{AgentActivity, AgentView, NodeClass};

/// Events the fakes produce for the plugin; the host drains it on deliver and settle.
pub(crate) type Outbox = Rc<RefCell<Vec<Event>>>;

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

/// Extra per-node data the lane hooks store.
#[derive(Debug, Clone, Default)]
struct NodeExtras {
    screen: Option<Vec<String>>,
    screen_unavailable: bool,
    session: Option<(String, Vec<String>)>,
    directory: Option<String>,
    metadata: Option<Vec<u8>>,
    locked: bool,
    worktree: bool,
}

/// Handle on the fake tree of a host.
#[derive(Debug, Default)]
pub struct FakeTree {
    nodes: BTreeMap<u64, TreeNode>,
    extras: BTreeMap<u64, NodeExtras>,
    revisions: BTreeMap<u64, u64>,
    last_id: u64,
    outbox: Outbox,
}

impl FakeTree {
    /// A tree that publishes events into `outbox`.
    pub(crate) fn with_outbox(outbox: Outbox) -> FakeTree {
        FakeTree {
            outbox,
            ..FakeTree::default()
        }
    }

    /// Ids of every node in pre-order (roots by ascending id, children by ascending id).
    pub(crate) fn preorder_ids(&self) -> Vec<u64> {
        fn walk(tree: &FakeTree, parent: Option<u64>, out: &mut Vec<u64>) {
            for (id, node) in &tree.nodes {
                if node.parent == parent {
                    out.push(*id);
                    walk(tree, Some(*id), out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self, None, &mut out);
        out
    }

    /// Inserts a node under its own id without queuing an event; later ids continue after the highest.
    pub(crate) fn insert_with_id(&mut self, node: TreeNode) {
        self.last_id = self.last_id.max(node.id);
        self.nodes.insert(node.id, node);
    }

    /// Inserts a node without queuing an event (fixtures load the tree before activation).
    pub(crate) fn insert_quiet(&mut self, node: TreeNode) -> u64 {
        self.last_id += 1;
        let id = self.last_id;
        self.nodes.insert(id, TreeNode { id, ..node });
        id
    }

    fn emit(&self, event: Event) {
        self.outbox.borrow_mut().push(event);
    }

    /// Adds a node, delivers `tree-changed` and returns the new id.
    pub fn add(&mut self, node: TreeNode) -> u64 {
        let id = self.insert_quiet(node);
        self.emit(Event::TreeChanged(vec![NodeId(id)]));
        id
    }

    /// Renames a node and delivers `tree-changed`.
    pub fn rename(&mut self, id: u64, name: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.name = name.to_string();
            self.emit(Event::TreeChanged(vec![NodeId(id)]));
        }
    }

    /// Delivers `agent-activity-changed` with `revision` plus 1.
    pub fn set_activity(&mut self, id: u64, activity: AgentActivity) {
        let Some(node) = self.nodes.get_mut(&id) else {
            return;
        };
        let previous = node
            .agent
            .as_ref()
            .map_or(AgentActivity::None, |agent| agent.activity);
        match node.agent.as_mut() {
            Some(agent) => agent.activity = activity,
            None => {
                node.agent = Some(AgentView {
                    signature: String::new(),
                    activity,
                    objective: None,
                    goal: None,
                });
            }
        }
        let revision = self.revisions.entry(id).or_insert(0);
        *revision += 1;
        let revision = *revision;
        self.emit(Event::AgentActivityChanged(AgentActivityEvent {
            node: NodeId(id),
            previous,
            current: activity,
            revision,
        }));
    }

    /// Removes a node and its children and delivers `node-removed`.
    pub fn remove(&mut self, id: u64) {
        if !self.nodes.contains_key(&id) {
            return;
        }
        let mut doomed = vec![id];
        let mut index = 0;
        while index < doomed.len() {
            let parent = doomed[index];
            doomed.extend(
                self.nodes
                    .values()
                    .filter(|node| node.parent == Some(parent))
                    .map(|node| node.id),
            );
            index += 1;
        }
        for removed in doomed {
            self.nodes.remove(&removed);
            self.extras.remove(&removed);
            self.emit(Event::NodeRemoved(NodeId(removed)));
        }
    }

    /// Reads a node.
    pub fn get(&self, id: u64) -> Option<TreeNode> {
        self.nodes.get(&id).cloned()
    }

    /// Moves a node under another parent and delivers `tree-changed` (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn move_node(&mut self, id: u64, new_parent: u64) {
        if id == new_parent || !self.nodes.contains_key(&new_parent) {
            return;
        }
        if let Some(node) = self.nodes.get_mut(&id) {
            node.parent = Some(new_parent);
            self.emit(Event::TreeChanged(vec![NodeId(id), NodeId(new_parent)]));
        }
    }

    /// Adds a group node under the root and returns its id (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn add_group(&mut self, name: &str) -> u64 {
        self.add(TreeNode {
            id: 0,
            parent: None,
            name: name.to_string(),
            class: NodeClass::Container,
            content_kind: None,
            agent: None,
        })
    }

    /// Adds a pane running an agent of `signature` (`"claude"`, `"codex"`) and returns its id (lane hook, see manual_gaps_fakes.md).
    pub fn add_agent_pane(&mut self, name: &str, signature: &str) -> u64 {
        self.add(TreeNode {
            id: 0,
            parent: None,
            name: name.to_string(),
            class: NodeClass::Pane,
            content_kind: Some("ilium.terminal".to_string()),
            agent: Some(AgentView {
                signature: signature.to_string(),
                activity: AgentActivity::Idle,
                objective: None,
                goal: None,
            }),
        })
    }

    /// The rows `terminal.read-screen` returns for a pane (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_screen(&mut self, id: u64, rows: Vec<String>) {
        let extras = self.extras.entry(id).or_default();
        extras.screen = Some(rows);
        extras.screen_unavailable = false;
    }

    /// `terminal.read-screen` answers `None` for the pane (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_screen_unavailable(&mut self, id: u64) {
        let extras = self.extras.entry(id).or_default();
        extras.screen = None;
        extras.screen_unavailable = true;
    }

    /// The verified agent session of a pane and its transcript lines (lane hook, see manual_gaps_fakes.md).
    pub fn set_agent_session(&mut self, id: u64, session_id: &str, transcript: Vec<String>) {
        self.extras.entry(id).or_default().session = Some((session_id.to_string(), transcript));
    }

    /// Records a goal on an agent pane; `status` is active, paused, blocked, done or unknown (lane hook, see manual_gaps_fakes.md).
    pub fn set_agent_goal(&mut self, id: u64, objective: &str, status: &str) {
        if let Some(agent) = self.nodes.get_mut(&id).and_then(|node| node.agent.as_mut()) {
            agent.objective = Some(objective.to_string());
            agent.goal = Some(status.to_string());
        }
    }

    /// The working directory of a pane, in `"scope:path"` shorthand (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_pane_directory(&mut self, id: u64, directory: &str) {
        self.extras.entry(id).or_default().directory = Some(directory.to_string());
    }

    /// The plugin metadata bytes stored on a node (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn metadata(&self, id: u64) -> Option<Vec<u8>> {
        self.extras.get(&id).and_then(|extras| extras.metadata.clone())
    }

    /// Stores plugin metadata bytes on a node (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_metadata(&mut self, id: u64, bytes: Vec<u8>) {
        self.extras.entry(id).or_default().metadata = Some(bytes);
    }

    /// Locks or unlocks a node against structural edits (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_locked(&mut self, id: u64, locked: bool) {
        self.extras.entry(id).or_default().locked = locked;
    }

    /// Marks a pane as living in a Git worktree (lane hook, see manual_gaps_fakes.md; DEV-470).
    pub fn set_worktree(&mut self, id: u64, worktree: bool) {
        self.extras.entry(id).or_default().worktree = worktree;
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

    /// REQ: DEV-471
    #[test]
    fn dev_471_first_two_nodes_get_ids_one_and_two() {
        let mut host = FakeHost::new();
        let first_id = host.tree().add(pane_node("shell", None));
        let second_id = host.tree().add(pane_node("editor", None));
        assert_eq!(first_id, 1);
        assert_eq!(second_id, 2);
    }

    /// REQ: DEV-471
    #[test]
    fn dev_471_ids_ignore_supplied_value_and_are_not_reused_after_remove() {
        let mut host = FakeHost::new();
        let first_id = host.tree().add(TreeNode {
            id: 99,
            ..pane_node("a", None)
        });
        host.tree().remove(first_id);
        let next_id = host.tree().add(pane_node("b", None));
        assert_eq!(first_id, 1);
        assert_eq!(next_id, 2);
        assert_eq!(host.tree().get(next_id).map(|node| node.id), Some(2));
    }

    /// REQ: DEV-470
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

    /// REQ: DEV-470
    #[test]
    fn dev_470_rename_changes_the_name_that_get_reports() {
        let mut host = FakeHost::new();
        let pane_id = host.tree().add(pane_node("shell", None));
        host.tree().rename(pane_id, "build");
        assert_eq!(
            host.tree().get(pane_id).map(|node| node.name),
            Some("build".to_string())
        );
    }

    /// REQ: DEV-470
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

    /// REQ: DEV-470
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

    /// REQ: DEV-470
    #[test]
    fn dev_470_get_on_unknown_or_removed_id_returns_none() {
        let mut host = FakeHost::new();
        assert!(host.tree().get(42).is_none());
        let pane_id = host.tree().add(pane_node("shell", None));
        host.tree().remove(pane_id);
        assert!(host.tree().get(pane_id).is_none());
    }

    /// REQ: DEV-470
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
        assert_eq!(
            host.tree().get(pane_id).map(|node| node.parent),
            Some(Some(target_id))
        );
    }
}
