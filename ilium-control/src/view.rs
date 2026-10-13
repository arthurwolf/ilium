//! Tree nodes as JSON values.

use ilium_core::{
    AgentTurn, ContainerKind, Node, NodeId, NodeKind, PaneContentKind, PaneStatus, Tree, ROOT_ID,
};
use serde_json::{json, Map, Value};

/// How much of a subtree a view includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOptions {
    /// Levels of children below the starting node; `None` is unlimited.
    pub depth: Option<usize>,
    /// Add the full per-pane detail object to every pane in the view.
    pub pane_details: bool,
    /// Add `children` ids only (no nested objects) once `depth` is reached.
    pub child_ids_at_limit: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            depth: None,
            pane_details: true,
            child_ids_at_limit: true,
        }
    }
}

/// Stable lowercase name of an agent turn, shared by every output format.
pub const fn turn_name(turn: AgentTurn, completion_unread: bool) -> &'static str {
    match turn {
        AgentTurn::Working => "working",
        AgentTurn::WaitingApproval => "waiting_approval",
        AgentTurn::WaitingSubagents => "waiting_subagents",
        AgentTurn::Settling => "settling",
        AgentTurn::Idle if completion_unread => "done",
        AgentTurn::Idle => "idle",
    }
}

/// `project`, `group`, `split_view`, `folder`, or `pane`.
pub fn node_kind_name(node: &Node) -> &'static str {
    match &node.kind {
        NodeKind::Container(container) => match container.kind {
            ContainerKind::Project { .. } => "project",
            ContainerKind::Group => "group",
            ContainerKind::SplitView { .. } => "split_view",
        },
        NodeKind::Folder { .. } => "folder",
        NodeKind::Pane { .. } => "pane",
    }
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn path_value(path: &std::path::Path) -> Value {
    Value::String(path.to_string_lossy().into_owned())
}

/// The status of a pane as a small tagged object.
fn status_view(status: &PaneStatus) -> Value {
    match status {
        PaneStatus::PlainShell => json!({"kind": "shell"}),
        PaneStatus::Editor { dirty } => json!({"kind": "editor", "dirty": dirty}),
        PaneStatus::Board => json!({"kind": "board"}),
        PaneStatus::Agent(agent) => json!({
            "kind": "agent",
            "agent": to_value(&agent.class),
            "state": turn_name(agent.turn, agent.completion_unread),
            "completion_unread": agent.completion_unread,
            "goal": agent.goal.as_ref().map(to_value),
        }),
        PaneStatus::AgentUnavailable(recovery) => json!({
            "kind": "agent_unavailable",
            "recovery": to_value(recovery),
        }),
    }
}

/// Everything known about one pane. `None` when `id` is not a pane.
pub fn pane_detail(tree: &Tree, id: NodeId) -> Option<Value> {
    let node = tree.get(id)?;
    let NodeKind::Pane {
        content,
        status,
        title_source,
        scheduled_input,
        prompt_queue,
        last_prompt,
        progress_monitors,
        launch_cwd,
        workspace,
        board_storage,
        ..
    } = &node.kind
    else {
        return None;
    };
    let content_name = match content {
        PaneContentKind::Terminal => "terminal",
        PaneContentKind::Editor => "editor",
        PaneContentKind::Board => "board",
    };
    let mut detail = Map::new();
    detail.insert("content".into(), json!(content_name));
    detail.insert("status".into(), status_view(status));
    detail.insert("title_source".into(), to_value(title_source));
    detail.insert(
        "cwd".into(),
        tree.pane_cwd(id).map(path_value).unwrap_or(Value::Null),
    );
    detail.insert(
        "launch_cwd".into(),
        launch_cwd.as_deref().map(path_value).unwrap_or(Value::Null),
    );
    detail.insert(
        "project_id".into(),
        tree.project_ancestor(id)
            .map_or(Value::Null, |id| json!(id.0)),
    );
    detail.insert(
        "project_path".into(),
        tree.project_path_for(id)
            .map(path_value)
            .unwrap_or(Value::Null),
    );
    detail.insert("last_prompt".into(), json!(last_prompt));
    detail.insert("scheduled_input".into(), to_value(scheduled_input));
    detail.insert("prompt_queue".into(), to_value(prompt_queue));
    detail.insert("progress_monitors".into(), to_value(progress_monitors));
    detail.insert(
        "workspace".into(),
        workspace.as_deref().map_or(Value::Null, to_value),
    );
    if board_storage.is_some() {
        detail.insert("board_storage".into(), to_value(board_storage));
    }
    Some(Value::Object(detail))
}

/// One node, with its subtree down to `options.depth`.
pub fn node_view(tree: &Tree, id: NodeId, options: ViewOptions) -> Option<Value> {
    node_view_at(tree, id, options, 0)
}

fn node_view_at(tree: &Tree, id: NodeId, options: ViewOptions, level: usize) -> Option<Value> {
    let node = tree.get(id)?;
    let mut view = Map::new();
    view.insert("id".into(), json!(id.0));
    view.insert("parent".into(), json!(node.parent.map(|parent| parent.0)));
    view.insert("kind".into(), json!(node_kind_name(node)));
    view.insert("name".into(), json!(node.name));
    view.insert("short_name".into(), json!(node.short_name));
    view.insert("is_name_fixed".into(), json!(node.is_name_fixed));
    view.insert("is_bookmarked".into(), json!(node.is_bookmarked));
    view.insert("activity_revision".into(), json!(node.activity_revision));
    view.insert(
        "has_unread_activity".into(),
        json!(node.has_activity_since_focus()),
    );
    let mut children: &[NodeId] = &[];
    match &node.kind {
        NodeKind::Container(container) => {
            view.insert("expanded".into(), json!(container.expanded));
            view.insert("locked_closed".into(), json!(container.locked_closed));
            match &container.kind {
                ContainerKind::Project { path } => {
                    view.insert("path".into(), path_value(path));
                }
                ContainerKind::SplitView { orientation } => {
                    view.insert("orientation".into(), to_value(orientation));
                }
                ContainerKind::Group => {}
            }
            children = &container.children;
        }
        NodeKind::Folder {
            path,
            expanded,
            locked_closed,
        } => {
            view.insert("path".into(), path_value(path));
            view.insert("expanded".into(), json!(expanded));
            view.insert("locked_closed".into(), json!(locked_closed));
        }
        NodeKind::Pane { .. } => {
            if options.pane_details {
                if let Some(detail) = pane_detail(tree, id) {
                    view.insert("pane".into(), detail);
                }
            }
        }
    }
    if matches!(node.kind, NodeKind::Container(_)) {
        view.insert("child_count".into(), json!(children.len()));
        let at_limit = options.depth.is_some_and(|depth| level >= depth);
        if at_limit {
            if options.child_ids_at_limit {
                view.insert(
                    "child_ids".into(),
                    Value::Array(children.iter().map(|id| json!(id.0)).collect()),
                );
            }
        } else {
            let nested: Vec<Value> = children
                .iter()
                .filter_map(|child| node_view_at(tree, *child, options, level + 1))
                .collect();
            view.insert("children".into(), Value::Array(nested));
        }
    }
    Some(Value::Object(view))
}

/// The whole session, rooted at the invisible session root.
pub fn tree_view(tree: &Tree, session_name: &str, options: ViewOptions) -> Value {
    let roots = tree
        .children_of(ROOT_ID)
        .map(|children| children.to_vec())
        .unwrap_or_default();
    let projects: Vec<Value> = roots
        .iter()
        .filter_map(|id| node_view_at(tree, *id, options, 0))
        .collect();
    json!({
        "session": session_name,
        "pane_count": tree.panes().count(),
        "project_count": tree.project_ids().len(),
        "nodes": projects,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::PaneContentKind;
    use std::path::PathBuf;

    fn sample() -> (Tree, NodeId, NodeId, NodeId) {
        let mut tree = Tree::new();
        let project = tree.add_project(PathBuf::from("/tmp/alpha")).unwrap();
        let group = tree.add_group(project, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell".to_owned(), PaneContentKind::Terminal)
            .unwrap();
        (tree, project, group, pane)
    }

    #[test]
    fn tree_view_nests_projects_groups_and_panes() {
        let (tree, project, group, pane) = sample();
        let view = tree_view(&tree, "default", ViewOptions::default());
        assert_eq!(view["pane_count"], 1);
        let project_view = &view["nodes"][0];
        assert_eq!(project_view["id"], project.0);
        assert_eq!(project_view["kind"], "project");
        assert_eq!(project_view["path"], "/tmp/alpha");
        let group_view = &project_view["children"][0];
        assert_eq!(group_view["id"], group.0);
        assert_eq!(group_view["children"][0]["id"], pane.0);
        assert_eq!(group_view["children"][0]["pane"]["status"]["kind"], "shell");
    }

    #[test]
    fn depth_limit_replaces_children_with_ids() {
        let (tree, project, group, _) = sample();
        let options = ViewOptions {
            depth: Some(0),
            ..ViewOptions::default()
        };
        let view = node_view(&tree, project, options).unwrap();
        assert!(view.get("children").is_none());
        assert_eq!(view["child_ids"][0], group.0);
    }

    #[test]
    fn pane_detail_is_none_for_a_group() {
        let (tree, _, group, pane) = sample();
        assert!(pane_detail(&tree, group).is_none());
        let detail = pane_detail(&tree, pane).unwrap();
        assert_eq!(detail["content"], "terminal");
        assert_eq!(detail["project_path"], "/tmp/alpha");
    }
}
