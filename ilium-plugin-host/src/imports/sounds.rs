//! Host implementation of `ilium:host/sounds` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostSounds` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::sounds::HostSounds;

/// Host side of the `sounds` interface.
#[derive(Debug)]
pub struct SoundsImport;

impl HostSounds for SoundsImport {
    fn catalog(&mut self) -> Vec<String> {
        unimplemented!("host::imports::sounds::catalog")
    }
}
