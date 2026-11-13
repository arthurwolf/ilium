//! Host implementation of `ilium:host/location` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostLocation` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::location::HostLocation;

/// Host side of the `location` interface.
#[derive(Debug)]
pub struct LocationImport;

impl HostLocation for LocationImport {
    fn current(&mut self) -> Option<(f64, f64)> {
        unimplemented!("host::imports::location::current")
    }
}
