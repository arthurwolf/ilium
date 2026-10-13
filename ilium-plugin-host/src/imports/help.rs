//! Host implementation of `ilium:host/help` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostHelp` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::help::HostHelp;

/// Host side of the `help` interface.
#[derive(Debug)]
pub struct HelpImport;

impl HostHelp for HelpImport {
    fn topic(&mut self, id: String) -> Option<String> {
        unimplemented!("host::imports::help::topic")
    }
}
