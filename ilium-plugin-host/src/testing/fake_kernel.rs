//! An in-memory kernel behind `KernelFacade` (test plan section 1.3 `testing`).

use std::collections::BTreeMap;
use std::sync::Mutex;

use ilium_plugin_api::generated::types::Intent;
use ilium_plugin_api::generated::types::NodeId;
use ilium_plugin_api::kernel::{
    Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
};
use ilium_plugin_api::ProcessKind;

/// The screen of one fake pane.
#[derive(Debug, Clone, Default)]
pub struct FakePaneScreen {
    /// Visible text, one entry per row.
    pub rows: Vec<String>,
    /// Lines of history, oldest first.
    pub history: Vec<String>,
}

/// An in-memory tree and pane screens; every applied intent is recorded.
#[derive(Debug)]
pub struct FakeKernel {
    process_kind: ProcessKind,
    state: Mutex<FakeKernelState>,
}

#[derive(Debug, Default)]
struct FakeKernelState {
    nodes: Vec<TreeNode>,
    epoch: u64,
    screens: BTreeMap<u64, FakePaneScreen>,
    applied: Vec<(Origin, Intent)>,
}

impl FakeKernel {
    /// An empty kernel running as the given process.
    pub fn new(process_kind: ProcessKind) -> FakeKernel {
        FakeKernel {
            process_kind,
            state: Mutex::new(FakeKernelState::default()),
        }
    }

    /// Replaces the tree and bumps the epoch.
    pub fn set_tree(&self, nodes: Vec<TreeNode>) {
        unimplemented!("host::testing::FakeKernel::set_tree")
    }

    /// Sets the screen of a pane.
    pub fn set_screen(&self, pane: NodeId, screen: FakePaneScreen) {
        unimplemented!("host::testing::FakeKernel::set_screen")
    }

    /// Every intent applied so far, with its origin, in order.
    pub fn applied_intents(&self) -> Vec<(Origin, Intent)> {
        unimplemented!("host::testing::FakeKernel::applied_intents")
    }
}

impl TreeRead for FakeKernel {
    fn nodes(&self) -> Vec<TreeNode> {
        unimplemented!("host::testing::FakeKernel::nodes")
    }

    fn node(&self, id: NodeId) -> Option<TreeNode> {
        unimplemented!("host::testing::FakeKernel::node")
    }

    fn epoch(&self) -> u64 {
        unimplemented!("host::testing::FakeKernel::epoch")
    }
}

impl PaneRead for FakeKernel {
    fn screen_text(&self, pane: NodeId) -> Option<String> {
        unimplemented!("host::testing::FakeKernel::screen_text")
    }

    fn cells(&self, pane: NodeId, rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        unimplemented!("host::testing::FakeKernel::cells")
    }

    fn history(&self, pane: NodeId, lines: u32) -> Option<Vec<String>> {
        unimplemented!("host::testing::FakeKernel::history")
    }
}

impl KernelFacade for FakeKernel {
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
        unimplemented!("host::testing::FakeKernel::apply_intent")
    }

    fn tree(&self) -> &dyn TreeRead {
        self
    }

    fn panes(&self) -> &dyn PaneRead {
        self
    }

    fn process_kind(&self) -> ProcessKind {
        self.process_kind
    }
}
