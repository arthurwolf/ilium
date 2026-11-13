//! The tree and pane host calls of the native bridge, answered from the fake tree
//! (reference-test-harness.md "Fake tree, panes, agents and jobs").
//!
//! A call without its grant reads as an empty tree, the way a real host hides the interface.

use ilium_plugin_sdk_core::generated::host::panes::HostPanes;
use ilium_plugin_sdk_core::generated::host::tree::HostTree;
use ilium_plugin_sdk_core::generated::types::tree::{
    NodeClass, NodeQuery, NodeView, PaneInfo, PrimitiveKind,
};
use ilium_plugin_sdk_core::generated::types::{NodeId, PluginError};

use super::bridge::NativeBridge;

/// Size the fake reports for every pane: the fakes do not model a terminal grid.
const FAKE_COLUMNS: u16 = 80;
const FAKE_ROWS: u16 = 24;
/// Most ids `find` returns (api-tree.md).
const FIND_LIMIT: usize = 4096;

impl NativeBridge {
    fn reads_tree(&mut self) -> bool {
        self.permit("tree.read").is_ok()
    }
}

impl HostTree for NativeBridge {
    fn get(&mut self, node: NodeId) -> Option<NodeView> {
        if !self.reads_tree() {
            return None;
        }
        self.host().tree.view(node.0)
    }

    fn children(&mut self, node: NodeId) -> Vec<NodeId> {
        if !self.reads_tree() {
            return Vec::new();
        }
        self.host()
            .tree
            .child_ids(Some(node.0))
            .into_iter()
            .map(NodeId)
            .collect()
    }

    fn roots(&mut self) -> Vec<NodeId> {
        if !self.reads_tree() {
            return Vec::new();
        }
        self.host()
            .tree
            .child_ids(None)
            .into_iter()
            .map(NodeId)
            .collect()
    }

    fn find(&mut self, query: NodeQuery) -> Vec<NodeId> {
        if !self.reads_tree() {
            return Vec::new();
        }
        let tree = &self.host().tree;
        let all = tree.preorder_ids();
        // Strictly below the queried node, as the kernel-backed import answers; the step bound
        // guards against a parent cycle in a hand-built tree.
        let under = |id: u64| {
            let Some(root) = query.under else {
                return true;
            };
            let mut parent = tree.get(id).and_then(|node| node.parent);
            for _ in 0..all.len() {
                match parent {
                    Some(found) if found == root.0 => return true,
                    Some(found) => parent = tree.get(found).and_then(|node| node.parent),
                    None => return false,
                }
            }
            false
        };
        all.iter()
            .filter_map(|id| tree.view(*id))
            .filter(|view| {
                query
                    .content_kind
                    .as_ref()
                    .is_none_or(|kind| view.content_kind.as_ref() == Some(kind))
            })
            .filter(|view| {
                query.signature.as_ref().is_none_or(|signature| {
                    view.agent
                        .as_ref()
                        .is_some_and(|agent| &agent.signature == signature)
                })
            })
            .filter(|view| under(view.id.0))
            .map(|view| view.id)
            .take(FIND_LIMIT)
            .collect()
    }

    fn metadata(&mut self, node: NodeId) -> Option<Vec<u8>> {
        self.permit("tree.metadata").ok()?;
        self.host().tree.metadata(node.0)
    }

    fn set_metadata(&mut self, node: NodeId, bytes: Vec<u8>) -> Result<(), PluginError> {
        self.permit("tree.metadata")?;
        self.host().tree.set_metadata(node.0, bytes);
        Ok(())
    }
}

impl HostPanes for NativeBridge {
    fn info(&mut self, pane: NodeId) -> Option<PaneInfo> {
        if !self.reads_tree() {
            return None;
        }
        let view = self.host().tree.view(pane.0)?;
        if view.class != NodeClass::Pane {
            return None;
        }
        let content_kind = view.content_kind.unwrap_or_default();
        let primitive = match content_kind.as_str() {
            "ilium.terminal" => PrimitiveKind::Terminal,
            "ilium.editor" => PrimitiveKind::Editor,
            _ => PrimitiveKind::Basic,
        };
        Some(PaneInfo {
            node: view.id,
            content_kind,
            primitive,
            columns: FAKE_COLUMNS,
            rows: FAKE_ROWS,
            focused: false,
            title: view.title,
            is_restored: false,
        })
    }

    /// The fakes have no focus; a test that needs one drives the plugin with `pane-focused`.
    fn focused(&mut self) -> Option<NodeId> {
        None
    }
}
