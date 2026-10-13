//! Host implementation of `ilium:host/topics` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostTopics` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::topics::HostTopics;
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `topics` interface.
#[derive(Debug)]
pub struct TopicsImport;

impl HostTopics for TopicsImport {
    fn publish(&mut self, topic: String, payload_json: String) -> Result<(), PluginError> {
        unimplemented!("host::imports::topics::publish")
    }
}
