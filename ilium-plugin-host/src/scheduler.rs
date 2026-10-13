//! Per-plugin queues, fairness and epoch interruption (reference-runtime-model.md,
//! reference-host-architecture.md HST-180..216).

use std::time::Instant;

use ilium_plugin_api::ids::PluginId;

/// Queue statistics of one plugin side (reference-host-architecture.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueStats {
    /// Events waiting.
    pub queued_events: usize,
    /// Events dropped for a full queue since activation.
    pub dropped_events: u64,
    /// Whether a call is running.
    pub running: bool,
}

/// The scheduler of all plugin queues (reference-host-architecture.md).
#[derive(Debug)]
pub struct Scheduler {
    epoch_tick_ms: u64,
}

impl Scheduler {
    /// A scheduler with the given epoch tick (reference-host-runtime.md).
    pub fn new(epoch_tick_ms: u64) -> Scheduler {
        Scheduler { epoch_tick_ms }
    }

    /// Statistics of a plugin's queue, if it has one.
    pub fn stats(&self, plugin: &PluginId) -> Option<QueueStats> {
        unimplemented!("host::scheduler::Scheduler::stats")
    }

    /// Advances time: runs due calls, applies deadlines, rotates fairly between plugins.
    pub fn tick(&self, now: Instant) {
        unimplemented!("host::scheduler::Scheduler::tick")
    }
}
