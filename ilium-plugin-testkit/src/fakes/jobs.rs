//! Fake jobs (reference-test-harness.md "Fake tree, panes, agents and jobs").

use ilium_plugin_sdk_core::types::PluginError;

/// A job the plugin started (DEV-472).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    /// Request number of the job.
    pub request: u64,
    /// Job kind.
    pub kind: String,
    /// Input bytes.
    pub input: Vec<u8>,
}

/// Handle on the fake job service of a host.
#[derive(Debug)]
pub struct FakeJobs {
    _private: (),
}

impl FakeJobs {
    /// Jobs the plugin started.
    pub fn list(&self) -> Vec<JobRecord> {
        unimplemented!("DEV-472")
    }

    /// Delivers one `request-completed` event for the job.
    pub fn finish(&mut self, _request: u64, _result: Result<Vec<u8>, PluginError>) {
        unimplemented!("DEV-472")
    }

    /// Delivers `job-progress`.
    pub fn progress(&mut self, _request: u64, _percent: f32, _message: &str) {
        unimplemented!("DEV-472")
    }

    /// Request numbers of the jobs the plugin cancelled, in order (lane hook, see manual_gaps_fakes.md; DEV-472).
    pub fn cancelled(&self) -> Vec<u64> {
        unimplemented!("DEV-472")
    }

    /// Percent values the plugin reported through `jobs.progress`, in order (lane hook, see manual_gaps_fakes.md).
    pub fn reported_progress(&self) -> Vec<f32> {
        unimplemented!("DEV-472")
    }

    /// Makes the next job run see a cancel request (lane hook, see manual_gaps_fakes.md; DEV-472).
    pub fn request_cancel(&mut self) {
        unimplemented!("DEV-472")
    }
}
