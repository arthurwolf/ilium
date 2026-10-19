//! Host implementation of `ilium:host/clock` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostClock` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::clock::HostClock;

/// Host side of the `clock` interface.
#[derive(Debug)]
pub struct ClockImport;

impl HostClock for ClockImport {
    fn now_unix_millis(&mut self) -> u64 {
        unimplemented!("host::imports::clock::now_unix_millis")
    }

    fn monotonic_millis(&mut self) -> u64 {
        unimplemented!("host::imports::clock::monotonic_millis")
    }
}
