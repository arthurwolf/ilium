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
        _after_millis: u32,
        _repeat_millis: Option<u32>,
    ) -> Result<TimerId, PluginError> {
        unimplemented!("host::imports::timers::set")
    }

    fn cancel(&mut self, _timer: TimerId) {
        unimplemented!("host::imports::timers::cancel")
    }

    fn at(&mut self, _epoch: EpochMs, _name: String) -> Result<TimerId, PluginError> {
        unimplemented!("host::imports::timers::at")
    }

    fn cancel_durable(&mut self, _name: String) {
        unimplemented!("host::imports::timers::cancel_durable")
    }
}
