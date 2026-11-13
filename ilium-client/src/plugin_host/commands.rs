//! Kernel entry points that hand a user action to a plugin command (foundation contract
//! section 15.3).
//!
//! A ported feature keeps its key binding, toolbar button and menu entry in the kernel until
//! the plugins that own those surfaces are ported; what they do is run the plugin's command.
//! The kernel passes what only it knows (the selection, the tree as it is drawn) as the
//! command's arguments and decides nothing else.

use ilium_core::{NodeKind, PaneContentKind, PaneStatus, ROOT_ID};
use ilium_plugin_api::generated::types::CommandOrigin;

use crate::app::App;

/// The Layout plugin's "New split view" command.
pub(crate) const NEW_SPLIT_COMMAND: &str = "ilium.core.layout/new-split";

impl App {
    /// Runs a plugin command. Tells the user when no plugin host is running; a command that
    /// no enabled plugin contributes is reported by the host as a toast.
    pub(crate) fn invoke_plugin_command(
        &mut self,
        command: &str,
        arguments_json: String,
        origin: CommandOrigin,
    ) {
        let screen = self.popup_screen();
        let posted = self
            .wasm_plugins
            .as_ref()
            .is_some_and(|plugins| plugins.invoke_command(command, arguments_json, origin, screen));
        if !posted {
            let owner = command.split_once('/').map_or(command, |(owner, _)| owner);
            self.status_message = Some(format!(
                "{owner} is not running, so this action is unavailable"
            ));
        }
    }

    /// "New split view" from the key binding, the tree toolbar or the context menu.
    pub(crate) fn action_new_split_view(&mut self, origin: CommandOrigin) {
        let arguments = self.new_split_arguments();
        self.invoke_plugin_command(NEW_SPLIT_COMMAND, arguments, origin);
    }

    /// The arguments of `new-split`: the group the selection points at and the tree as plain
    /// facts. Which panes may join, how they are labelled and how many fit is the plugin's
    /// business.
    fn new_split_arguments(&self) -> String {
        let nodes: Vec<serde_json::Value> = self
            .tree
            .all_ids()
            .filter_map(|id| {
                let node = self.tree.get(id)?;
                let (kind, content, agent) = match &node.kind {
                    NodeKind::Pane {
                        content, status, ..
                    } => (
                        "pane",
                        Some(match content {
                            PaneContentKind::Terminal => "terminal",
                            PaneContentKind::Editor => "editor",
                            PaneContentKind::Board => "board",
                        }),
                        match status {
                            PaneStatus::Agent(_) => Some("agent"),
                            PaneStatus::AgentUnavailable(_) => Some("agent unavailable"),
                            _ => None,
                        },
                    ),
                    NodeKind::Folder { .. } => ("folder", None, None),
                    NodeKind::Container(_) if node.is_split_view() => ("split", None, None),
                    NodeKind::Container(_) if node.is_worktree() => ("worktree", None, None),
                    NodeKind::Container(_) if node.is_project() => ("project", None, None),
                    NodeKind::Container(_) => ("group", None, None),
                };
                Some(serde_json::json!({
                    "id": id.0,
                    "parent": node.parent.map(|parent| parent.0),
                    "kind": kind,
                    "content": content,
                    "agent": agent,
                    "title": node.name,
                }))
            })
            .collect();
        let panes: Vec<u64> = self
            .tree
            .pane_ids_in_tree_order()
            .into_iter()
            .map(|id| id.0)
            .collect();
        serde_json::json!({
            "root": ROOT_ID.0,
            "parent": self.split_parent_group().0,
            "nodes": nodes,
            "panes": panes,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::SplitOrientation;

    #[test]
    fn new_split_arguments_carry_the_selected_group_and_the_tree_as_facts() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let free = app
            .tree
            .add_pane(group, "free", PaneContentKind::Editor)
            .unwrap();
        let held = app
            .tree
            .add_pane(group, "held", PaneContentKind::Terminal)
            .unwrap();
        let split = app
            .tree
            .create_split_view(group, "Vertical split", SplitOrientation::Vertical, &[held])
            .unwrap();
        app.tree_state.select(vec![group]);

        let arguments: serde_json::Value =
            serde_json::from_str(&app.new_split_arguments()).unwrap();

        assert_eq!(arguments["parent"], group.0);
        assert_eq!(arguments["root"], ROOT_ID.0);
        let panes = arguments["panes"].as_array().unwrap();
        assert_eq!(panes.len(), 2);
        assert!(panes.contains(&serde_json::json!(free.0)));
        assert!(panes.contains(&serde_json::json!(held.0)));
        let node = |id: u64| {
            arguments["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["id"] == id)
                .unwrap()
                .clone()
        };
        assert_eq!(node(split.0)["kind"], "split");
        assert_eq!(node(group.0)["kind"], "group");
        assert_eq!(node(free.0)["content"], "editor");
        assert_eq!(node(held.0)["parent"], split.0);
    }

    #[test]
    fn a_plugin_command_without_a_plugin_host_tells_the_user() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.action_new_split_view(CommandOrigin::Keybinding);
        assert_eq!(
            app.status_message.as_deref(),
            Some("ilium.core.layout is not running, so this action is unavailable")
        );
    }
}
