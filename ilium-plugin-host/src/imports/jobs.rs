//! Host implementation of `ilium:host/jobs` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostJobs` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::jobs::HostJobs;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `jobs` interface.
#[derive(Debug)]
pub struct JobsImport;

impl HostJobs for JobsImport {
    fn start(&mut self, _kind: String, _input: Vec<u8>) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::jobs::start")
    }

    fn cancel(&mut self, _job: RequestId) {
        unimplemented!("host::imports::jobs::cancel")
    }

    fn take_output(&mut self, _job: RequestId) -> Option<Vec<u8>> {
        unimplemented!("host::imports::jobs::take_output")
    }

    fn report_progress(&mut self, _percent: f32) {
        unimplemented!("host::imports::jobs::report_progress")
    }

    fn is_cancelled(&mut self) -> bool {
        unimplemented!("host::imports::jobs::is_cancelled")
    }

    fn wait_request(
        &mut self,
        _request: RequestId,
        _timeout_millis: u32,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::jobs::wait_request")
    }
}
