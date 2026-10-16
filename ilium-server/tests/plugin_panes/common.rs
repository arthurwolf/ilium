//! Shared helpers: a `ServerKernel` test world (gap constructors, see API_GAPS.md) and outcome readers.
#![allow(dead_code)]

use ilium_plugin_api::generated::types::{ErrorCode, Intent, NodeId, Outcome, PaneLifecycleState, PluginId};
use ilium_plugin_api::kernel::{KernelFacade, Origin};
use ilium_plugin_api::Side;
use ilium_server::plugin_host::ServerKernel;

pub const RUNNING_PANE: NodeId = NodeId(10);
pub const RESTORED_PANE: NodeId = NodeId(11);
pub const EXITED_PANE: NodeId = NodeId(12);
pub const NEIGHBOUR_PANE: NodeId = NodeId(13);
pub const PLUGIN: &str = "test.plugin";

pub fn origin() -> Origin {
    Origin::Plugin { plugin: PluginId(PLUGIN.to_owned()), side: Side::Server, instance: 1 }
}

/// A kernel whose tree holds RUNNING_PANE (running), RESTORED_PANE (restored, no process),
/// EXITED_PANE (exited) and NEIGHBOUR_PANE (a split neighbour of RUNNING_PANE, minimum 10x3).
pub fn world(grants: &[&str]) -> ServerKernel {
    ServerKernel::for_test_world(PLUGIN, grants)
}

pub fn submit(kernel: &ServerKernel, intent: Intent) -> Outcome {
    kernel.apply_intent(&origin(), intent)
}

pub fn refusal(outcome: &Outcome) -> Option<ErrorCode> {
    match outcome {
        Outcome::Refused(error) => Some(error.code),
        _ => None,
    }
}

pub fn failure(outcome: &Outcome) -> Option<ErrorCode> {
    match outcome {
        Outcome::Failed(error) => Some(error.code),
        _ => None,
    }
}

pub fn message(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Refused(error) | Outcome::Failed(error) => error.message.clone(),
        _ => String::new(),
    }
}

pub fn states(kernel: &ServerKernel, pane: NodeId) -> Vec<PaneLifecycleState> {
    kernel.lifecycle_events(pane)
}
