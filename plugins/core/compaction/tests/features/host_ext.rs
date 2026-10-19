//! Compaction-lane testing hooks used only by the feature test target.
//!
//! Bodies are skeletons: `unimplemented!()` until the plugin exists. Shared harness behaviour stays
//! in `ilium_plugin_sdk::testing::FakeHost`.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{PluginError, Submission};

/// Agent config files seen by the plugin, through the host.
pub struct AgentFiles<'a>(pub &'a mut FakeHost);

impl AgentFiles<'_> {
    /// Writes a file.
    pub fn write(&mut self, _path: &str, _bytes: &[u8]) {
        unimplemented!("compaction.agent-files-write")
    }

    /// Reads a file.
    pub fn read(&self, _path: &str) -> Option<Vec<u8>> {
        unimplemented!("compaction.agent-files-read")
    }
}

pub trait CompactionFeatureHostExt {
    fn set_scan_result(&mut self, _sessions: u64, _turns: u64) {
        unimplemented!("compaction.set-scan-result")
    }
    fn set_recommendation(&mut self, _tokens: u64, _alternative: u64) {
        unimplemented!("compaction.set-recommendation")
    }
    fn agent_files(&mut self) -> AgentFiles<'_> {
        unimplemented!("compaction.agent-files")
    }
    fn convert_session(&mut self, _from: &str, _to: &str, _path: &str) -> Vec<Submission> {
        unimplemented!("compaction.convert-session")
    }
    fn convert_session_failing(&mut self, _from: &str, _to: &str) -> Vec<Submission> {
        unimplemented!("compaction.convert-session-failing")
    }
    fn cancel_job_after_chunks(&mut self, _chunks: usize) {
        unimplemented!("compaction.cancel-job-after-chunks")
    }
    fn job_progress_reports(&self) -> Vec<f64> {
        unimplemented!("compaction.job-progress-reports")
    }
    fn run_job_for_pane(
        &mut self,
        _pane: u64,
        _kind: &str,
        _input: Vec<u8>,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("compaction.run-job-for-pane")
    }
}

impl CompactionFeatureHostExt for FakeHost {}
