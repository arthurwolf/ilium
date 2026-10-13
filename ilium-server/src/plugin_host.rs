//! The server's plugin host: boot order and the `KernelFacade` over the real tree
//! (reference-host-architecture.md). Skeleton: nothing calls it yet, so no existing code path
//! changes behaviour.

// Not wired into the server yet; the implementation phase calls it from `run()`.
#![allow(dead_code, unused_variables)]

use ilium_plugin_api::generated::types::{Intent, NodeId};
use ilium_plugin_api::kernel::{
    Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
};
use ilium_plugin_api::ProcessKind;
use ilium_plugin_host::lifecycle::{ShutdownReport, StartMode, StartReport};

/// Read and write access to the real tree and panes for plugins.
#[derive(Debug)]
pub struct ServerKernel;

impl TreeRead for ServerKernel {
    fn nodes(&self) -> Vec<TreeNode> {
        unimplemented!("server::plugin_host::ServerKernel::nodes")
    }

    fn node(&self, id: NodeId) -> Option<TreeNode> {
        unimplemented!("server::plugin_host::ServerKernel::node")
    }

    fn epoch(&self) -> u64 {
        unimplemented!("server::plugin_host::ServerKernel::epoch")
    }
}

impl PaneRead for ServerKernel {
    fn screen_text(&self, pane: NodeId) -> Option<String> {
        unimplemented!("server::plugin_host::ServerKernel::screen_text")
    }

    fn cells(&self, pane: NodeId, rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
        unimplemented!("server::plugin_host::ServerKernel::cells")
    }

    fn history(&self, pane: NodeId, lines: u32) -> Option<Vec<String>> {
        unimplemented!("server::plugin_host::ServerKernel::history")
    }
}

impl KernelFacade for ServerKernel {
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult {
        unimplemented!("server::plugin_host::ServerKernel::apply_intent")
    }

    fn tree(&self) -> &dyn TreeRead {
        self
    }

    fn panes(&self) -> &dyn PaneRead {
        self
    }

    fn process_kind(&self) -> ProcessKind {
        ProcessKind::Server
    }
}

/// Owns the plugin host inside the server process.
#[derive(Debug)]
pub struct ServerPluginHost {
    start_mode: StartMode,
}

impl ServerPluginHost {
    /// A host that will start in the given mode.
    pub fn new(start_mode: StartMode) -> ServerPluginHost {
        ServerPluginHost { start_mode }
    }

    /// Boot order: engine, registries, core plugins, then the rest (reference-host-architecture.md).
    pub fn start(&self) -> StartReport {
        unimplemented!("server::plugin_host::ServerPluginHost::start")
    }

    /// Reverse order within the shutdown budget.
    pub fn shutdown(&self) -> ShutdownReport {
        unimplemented!("server::plugin_host::ServerPluginHost::shutdown")
    }
}
