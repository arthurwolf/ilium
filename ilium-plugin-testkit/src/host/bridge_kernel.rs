//! The tree and pane host calls of the native bridge, answered from the fake tree
//! (reference-test-harness.md "Fake tree, panes, agents and jobs").
//!
//! A call without its grant reads as an empty tree, the way a real host hides the interface.

use ilium_plugin_sdk_core::generated::host::kernel_settings::HostKernelSettings;
use ilium_plugin_sdk_core::generated::host::panes::HostPanes;
use ilium_plugin_sdk_core::generated::host::tree::HostTree;
use ilium_plugin_sdk_core::generated::types::tree::{
    NodeClass, NodeQuery, NodeView, PaneInfo, PrimitiveKind,
};
use ilium_plugin_sdk_core::generated::types::{ErrorCode, NodeId, PluginError};

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
        let mut view = self.host().tree.view(node.0)?;
        // The agent of a pane is filled in only for a plugin that holds `agent.read`.
        if self.permit("agent.read").is_err() {
            view.agent = None;
        }
        Some(view)
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

/// The kernel settings tabs, answered from `FakeHost::kernel_settings` (the tab a test set). A
/// read needs `settings.read-kernel:<tab>` and a write `settings.write-kernel:<tab>`, as in the
/// kernel-backed import; a tab the test never set reads as not found.
impl HostKernelSettings for NativeBridge {
    fn read(&mut self, tab: String) -> Result<String, PluginError> {
        self.permit(&format!("settings.read-kernel:{tab}"))?;
        match self.host().kernel_settings.tab(&tab) {
            Some(values) => Ok(values.to_string()),
            None => Err(PluginError::new(
                ErrorCode::NotFound,
                format!("the {tab} tab has no values in this fake host"),
            )),
        }
    }

    fn write(&mut self, tab: String, key: String, value: String) -> Result<(), PluginError> {
        self.permit(&format!("settings.write-kernel:{tab}"))?;
        let parsed: serde_json::Value = serde_json::from_str(&value).map_err(|error| {
            PluginError::new(
                ErrorCode::InvalidArgument,
                format!("the value is not JSON: {error}"),
            )
        })?;
        self.host().kernel_settings.set(&tab, &key, parsed)
    }
}

use ilium_plugin_sdk_core::generated as m;
use ilium_plugin_sdk_core::generated::host::vector::HostVector;
use ilium_plugin_sdk_core::generated::primitives::terminal::HostTerminal;
use ilium_plugin_sdk_core::generated::types::Outcome;

// ---- terminal screens and the vector index ---------------------------------------------------

/// A call the fakes do not model.
fn unmodelled() -> PluginError {
    PluginError::new(
        ErrorCode::Unsupported,
        "the fake host does not model this call",
    )
}

impl HostTerminal for NativeBridge {
    fn read_screen(
        &mut self,
        a0: m::types::ids::NodeId,
        a1: m::primitives::terminal::ScreenPart,
        a2: u32,
    ) -> ::core::option::Option<m::primitives::terminal::Screen> {
        self.permit("terminal.read-screen").ok()?;
        let pane = a0.0;
        let host = self.host();
        let rows = match a1 {
            m::primitives::terminal::ScreenPart::Visible => host.tree.screen_rows(pane)?,
            // The fake keeps no text for the scrollback: only its length is modelled.
            m::primitives::terminal::ScreenPart::Scrollback => Vec::new(),
        };
        let limit = usize::try_from(a2.min(10_000)).unwrap_or(10_000);
        let rows: Vec<String> = rows.into_iter().take(limit).collect();
        let columns = rows
            .iter()
            .map(|row| u16::try_from(row.chars().count()).unwrap_or(u16::MAX))
            .max()
            .unwrap_or(0);
        Some(m::primitives::terminal::Screen {
            columns,
            rows,
            cursor_row: 0,
            cursor_column: 0,
            title: None,
            revision: 0,
        })
    }
    fn get_selection(
        &mut self,
        a0: m::types::ids::NodeId,
    ) -> ::core::option::Option<m::primitives::terminal::Selection> {
        ::core::option::Option::None
    }
    fn add_output_filter(
        &mut self,
        a0: m::types::ids::NodeId,
        a1: m::primitives::terminal::OutputFilter,
    ) -> ::core::result::Result<(), m::types::errors::PluginError> {
        ::core::result::Result::Err(unmodelled())
    }
    fn remove_output_filter(&mut self, a0: m::types::ids::NodeId, a1: ::std::string::String) -> () {
        ()
    }
    fn claim_pointer(
        &mut self,
        a0: m::types::ids::NodeId,
    ) -> ::core::result::Result<(), m::types::errors::PluginError> {
        ::core::result::Result::Err(unmodelled())
    }
    fn claim_pointer_gesture(
        &mut self,
        a0: m::types::ids::NodeId,
        a1: m::types::ui::PointerModifiers,
    ) -> ::core::result::Result<(), m::types::errors::PluginError> {
        ::core::result::Result::Err(unmodelled())
    }
    fn release_pointer(&mut self, a0: m::types::ids::NodeId) -> () {
        ()
    }
    fn set_highlights(
        &mut self,
        a0: m::types::ids::NodeId,
        a1: ::std::string::String,
        a2: ::std::vec::Vec<m::primitives::terminal::CellRegion>,
    ) -> ::core::result::Result<(), m::types::errors::PluginError> {
        ::core::result::Result::Err(unmodelled())
    }
    fn clear_highlights(&mut self, a0: m::types::ids::NodeId, a1: ::std::string::String) -> () {
        ()
    }
    fn read_cells(
        &mut self,
        a0: m::types::ids::NodeId,
        a1: m::primitives::terminal::ScreenPart,
        a2: u32,
        a3: u32,
    ) -> ::core::option::Option<m::primitives::terminal::CellRows> {
        ::core::option::Option::None
    }
    fn modes(
        &mut self,
        a0: m::types::ids::NodeId,
    ) -> ::core::option::Option<m::primitives::terminal::TerminalMode> {
        ::core::option::Option::None
    }
    fn get_foreground_process(
        &mut self,
        a0: m::types::ids::NodeId,
    ) -> ::core::option::Option<m::primitives::terminal::ForegroundProcess> {
        ::core::option::Option::None
    }
    fn freeze_snapshot(
        &mut self,
        a0: m::types::ids::NodeId,
    ) -> ::core::result::Result<u64, m::types::errors::PluginError> {
        ::core::result::Result::Err(unmodelled())
    }
    fn read_snapshot_cells(
        &mut self,
        a0: u64,
        a1: u32,
        a2: u32,
    ) -> ::core::option::Option<m::primitives::terminal::CellRows> {
        ::core::option::Option::None
    }
    fn drop_snapshot(&mut self, a0: u64) -> () {
        ()
    }
    fn history_text(
        &mut self,
        a0: m::types::ids::NodeId,
        a1: u32,
    ) -> ::core::result::Result<m::types::ids::RequestId, m::types::errors::PluginError> {
        ::core::result::Result::Err(unmodelled())
    }
    fn take_history(
        &mut self,
        a0: m::types::ids::RequestId,
    ) -> ::core::option::Option<::std::string::String> {
        ::core::option::Option::None
    }
    fn search_history(
        &mut self,
        a0: m::types::ids::NodeId,
        a1: ::std::string::String,
        a2: u16,
    ) -> ::core::result::Result<m::types::ids::RequestId, m::types::errors::PluginError> {
        ::core::result::Result::Err(unmodelled())
    }
    fn take_matches(
        &mut self,
        a0: m::types::ids::RequestId,
    ) -> ::core::option::Option<::std::vec::Vec<m::primitives::terminal::HistoryMatch>> {
        ::core::option::Option::None
    }
    fn lock_input(
        &mut self,
        a0: m::types::ids::NodeId,
        a1: u16,
    ) -> ::core::result::Result<(), m::types::errors::PluginError> {
        ::core::result::Result::Err(unmodelled())
    }
    fn unlock_input(&mut self, a0: m::types::ids::NodeId) -> () {
        ()
    }
}

impl HostVector for NativeBridge {
    fn embed(
        &mut self,
        a0: ::std::vec::Vec<::std::string::String>,
        a1: ::std::string::String,
    ) -> ::core::result::Result<m::types::ids::RequestId, m::types::errors::PluginError> {
        self.permit("inference.embed")?;
        if a0.len() > 256 || a0.iter().any(|text| text.len() > 8 * 1024) {
            return ::core::result::Result::Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "embed takes at most 256 texts of at most 8 KiB each",
            ));
        }
        let vectors = self.host().vector.embed(&a0);
        let id = self.next_id();
        self.host().bridge.vectors.insert(id, vectors);
        self.complete(id, Outcome::Ok);
        ::core::result::Result::Ok(m::types::ids::RequestId(id))
    }
    fn take_vectors(
        &mut self,
        a0: m::types::ids::RequestId,
    ) -> ::core::option::Option<::std::vec::Vec<::std::vec::Vec<f32>>> {
        self.host().bridge.vectors.remove(&a0.0)
    }
    fn create_index(
        &mut self,
        a0: ::std::string::String,
        a1: u16,
    ) -> ::core::result::Result<m::types::ids::IndexId, m::types::errors::PluginError> {
        self.permit("inference.embed")?;
        let index = self.host().vector.create_index(a1);
        ::core::result::Result::Ok(m::types::ids::IndexId(index))
    }
    fn upsert(
        &mut self,
        a0: m::types::ids::IndexId,
        a1: ::std::vec::Vec<(::std::string::String, ::std::vec::Vec<f32>)>,
    ) -> ::core::result::Result<(), m::types::errors::PluginError> {
        self.permit("inference.embed")?;
        if self.host().vector.upsert(a0.0, a1) {
            ::core::result::Result::Ok(())
        } else {
            ::core::result::Result::Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "unknown index or a vector of the wrong dimension",
            ))
        }
    }
    fn remove_entries(
        &mut self,
        a0: m::types::ids::IndexId,
        a1: ::std::vec::Vec<::std::string::String>,
    ) -> ::core::result::Result<(), m::types::errors::PluginError> {
        self.permit("inference.embed")?;
        if self.host().vector.remove_entries(a0.0, &a1) {
            ::core::result::Result::Ok(())
        } else {
            ::core::result::Result::Err(PluginError::new(ErrorCode::NotFound, "unknown index"))
        }
    }
    fn top_k(
        &mut self,
        a0: m::types::ids::IndexId,
        a1: ::std::vec::Vec<f32>,
        a2: u16,
    ) -> ::core::result::Result<
        ::std::vec::Vec<(::std::string::String, f32)>,
        m::types::errors::PluginError,
    > {
        self.permit("inference.embed")?;
        self.host()
            .vector
            .top_k(a0.0, a2)
            .ok_or_else(|| PluginError::new(ErrorCode::NotFound, "unknown index"))
    }
    fn drop_index(&mut self, a0: m::types::ids::IndexId) -> () {
        self.host().vector.drop_index(a0.0);
    }
}
