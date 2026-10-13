//! Host implementation of `ilium:host/inference` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostInference` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::inference::{Completion, CompletionRequest, HostInference};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `inference` interface.
#[derive(Debug)]
pub struct InferenceImport;

impl HostInference for InferenceImport {
    fn complete(&mut self, request: CompletionRequest) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::inference::complete")
    }

    fn take_completion(&mut self, request: RequestId) -> Option<Completion> {
        unimplemented!("host::imports::inference::take_completion")
    }

    fn models(&mut self) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::inference::models")
    }

    fn take_models(&mut self, request: RequestId) -> Option<Vec<String>> {
        unimplemented!("host::imports::inference::take_models")
    }

    fn provider_reply(
        &mut self,
        call: u64,
        outcome: Result<Completion, PluginError>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::inference::provider_reply")
    }
}
