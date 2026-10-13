//! The dispatcher: submissions in, outcomes out, deduplication and pending-request accounting
//! (reference-events-and-intents.md ABI-008, ABI-010, ABI-013).

use std::sync::Arc;

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::requests::{RequestRef, RequestResult};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::kernel::{KernelFacade, Origin};

/// ABI-013: pending requests per plugin.
pub const MAX_PENDING_PER_PLUGIN: usize = 64;

/// ABI-008: a call that returns more submissions than this traps (the 257th).
pub const MAX_SUBMISSIONS_PER_CALL: usize = 256;

/// Dispatcher settings (reference-events-and-intents.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchConfig {
    /// Pending requests allowed per plugin.
    pub max_pending_per_plugin: usize,
    /// Submissions allowed per guest call.
    pub max_submissions_per_call: usize,
    /// Deduplication window in milliseconds (ABI-010).
    pub dedup_window_ms: u64,
}

/// What one [`Dispatcher::pump`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PumpReport {
    /// Rounds run.
    pub rounds: u32,
    /// Events handed to guests.
    pub events_delivered: usize,
    /// Intents applied to the kernel.
    pub intents_applied: usize,
    /// Calls that trapped.
    pub traps: usize,
}

/// Routes submissions to the kernel and outcomes back to plugins (reference-host-architecture.md).
pub struct Dispatcher {
    kernel: Arc<dyn KernelFacade>,
    config: DispatchConfig,
}

impl Dispatcher {
    /// A dispatcher over a kernel facade.
    pub fn new(kernel: Arc<dyn KernelFacade>, config: DispatchConfig) -> Dispatcher {
        unimplemented!("host::dispatch::Dispatcher::new")
    }

    /// Queues one submission for an origin and returns its request reference.
    pub fn submit(&self, origin: Origin, submission: Submission) -> RequestRef {
        unimplemented!("host::dispatch::Dispatcher::submit")
    }

    /// Queues events for delivery to the plugins that subscribed.
    pub fn deliver(&self, events: Vec<Event>) {
        unimplemented!("host::dispatch::Dispatcher::deliver")
    }

    /// The outcomes waiting for a plugin.
    pub fn outcomes(&self, plugin: &PluginId) -> Vec<RequestResult> {
        unimplemented!("host::dispatch::Dispatcher::outcomes")
    }

    /// Runs delivery and application rounds until quiet or `max_rounds` is reached.
    pub fn pump(&self, max_rounds: u32) -> PumpReport {
        unimplemented!("host::dispatch::Dispatcher::pump")
    }

    /// The settings in force.
    pub fn config(&self) -> &DispatchConfig {
        &self.config
    }
}
