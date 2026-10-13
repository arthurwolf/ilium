//! Host implementation of `ilium:host/timers` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostTimers` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::timers::HostTimers;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{EpochMs, TimerId};

/// Host side of the `timers` interface.
#[derive(Debug)]
pub struct TimersImport;

impl HostTimers for TimersImport {
    fn set(
        &mut self,
        after_millis: u32,
        repeat_millis: Option<u32>,
    ) -> Result<TimerId, PluginError> {
        unimplemented!("host::imports::timers::set")
    }

    fn cancel(&mut self, timer: TimerId) {
        unimplemented!("host::imports::timers::cancel")
    }

    fn at(&mut self, epoch: EpochMs, name: String) -> Result<TimerId, PluginError> {
        unimplemented!("host::imports::timers::at")
    }

    fn cancel_durable(&mut self, name: String) {
        unimplemented!("host::imports::timers::cancel_durable")
    }
}
