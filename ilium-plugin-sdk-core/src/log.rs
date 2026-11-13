//! Logging helper over `host::log` (rust-sdk.md "Calling the host").

use crate::generated::host::Level;

/// Sends one log line to the host; lines over the per-second budget are dropped by the host.
pub fn log(_level: Level, _message: &str) {
    unimplemented!("rust-sdk.log")
}
