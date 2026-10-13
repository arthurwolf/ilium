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
}
