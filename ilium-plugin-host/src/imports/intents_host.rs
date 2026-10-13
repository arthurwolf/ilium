//! Host implementation of `ilium:host/intents-host` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostIntentsHost` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::intents_host::HostIntentsHost;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::intents::Submission;

/// Host side of the `intents-host` interface.
#[derive(Debug)]
pub struct IntentsHostImport;

impl HostIntentsHost for IntentsHostImport {
    fn submit(&mut self, submission: Submission) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::intents_host::submit")
    }
}
