//! Host implementation of `ilium:host/log` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostLog` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::log::{HostLog, Level};

/// Host side of the `log` interface.
#[derive(Debug)]
pub struct LogImport;

impl HostLog for LogImport {
    fn log(&mut self, level: Level, message: String) {
        unimplemented!("host::imports::log::log")
    }
}
