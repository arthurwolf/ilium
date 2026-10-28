//! Compaction-lane testing hooks used only by the feature test target.
//!
//! Plugin-specific scaffolding on top of `ilium_plugin_sdk::testing::FakeHost`: agent config files live in the
//! fake `agent-homes` scope, scan and recommendation data sit in the plugin's storage mailbox, session conversion
//! calls the plugin's service `ilium.core.compaction/convert-session`, and jobs go through the host's job fake.

use ilium_plugin_sdk::host::Bucket;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Event, NodeId, PluginError, Submission};
use serde_json::json;

use crate::host_ext_shared::RECOMMENDATION_KEY;

/// Storage key holding the last scan result (`{"sessions":..,"turns":..}`).
const SCAN_RESULT_KEY: &str = "compaction.scan-result";
/// Storage key holding the chunk count after which the next job is cancelled.
const CANCEL_AFTER_CHUNKS_KEY: &str = "compaction.cancel-after-chunks";
const CONVERT_SERVICE: &str = "ilium.core.compaction/convert-session";

/// Agent config files seen by the plugin, through the host.
pub struct AgentFiles<'a> {
    pub _host: &'a mut FakeHost,
}

impl AgentFiles<'_> {
    /// Writes a file (`agent-homes:` shorthand paths).
    pub fn write(&mut self, path: &str, bytes: &[u8]) {
        self._host.fs().write(path, bytes);
    }

    /// Reads a file.
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        self._host.fs().read(path)
    }
}

pub trait CompactionFeatureHostExt {
    fn set_scan_result(&mut self, sessions: u64, turns: u64);
    fn set_recommendation(&mut self, tokens: u64, alternative: u64);
    fn agent_files(&mut self) -> AgentFiles<'_>;
    fn convert_session(&mut self, from: &str, to: &str, path: &str) -> Vec<Submission>;
    fn convert_session_failing(&mut self, from: &str, to: &str) -> Vec<Submission>;
    fn cancel_job_after_chunks(&mut self, chunks: usize);
    fn job_progress_reports(&mut self) -> Vec<f64>;
    fn run_job_for_pane(
        &mut self,
        pane: u64,
        kind: &str,
        input: Vec<u8>,
    ) -> Result<Vec<u8>, PluginError>;
}

/// Calls the conversion service and returns the submissions the plugin made meanwhile.
fn convert(host: &mut FakeHost, from: &str, to: &str, path: &str) -> Vec<Submission> {
    let before = host.submitted().len();
    // A failing conversion is a legitimate outcome here; the test inspects the submissions it left.
    let _ = host.call_service(
        CONVERT_SERVICE,
        "call",
        &json!({ "from": from, "to": to, "path": path }).to_string(),
    );
    host.settle();
    host.submitted().split_off(before)
}

impl CompactionFeatureHostExt for FakeHost {
    fn set_scan_result(&mut self, sessions: u64, turns: u64) {
        let value = json!({ "sessions": sessions, "turns": turns }).to_string();
        self.storage_mut()
            .put(Bucket::Global, SCAN_RESULT_KEY, value.as_bytes());
    }

    fn set_recommendation(&mut self, tokens: u64, alternative: u64) {
        let value = json!({ "tokens": tokens, "alternative": alternative }).to_string();
        self.storage_mut()
            .put(Bucket::Global, RECOMMENDATION_KEY, value.as_bytes());
    }

    fn agent_files(&mut self) -> AgentFiles<'_> {
        AgentFiles { _host: self }
    }

    fn convert_session(&mut self, from: &str, to: &str, path: &str) -> Vec<Submission> {
        convert(self, from, to, path)
    }

    /// Converts a session file that does not exist, so the plugin's own read fails.
    fn convert_session_failing(&mut self, from: &str, to: &str) -> Vec<Submission> {
        convert(self, from, to, "agent-homes:.missing/no-such-session.jsonl")
    }

    /// The job fake only knows a single cancel flag; the chunk count is kept for the plugin to read.
    fn cancel_job_after_chunks(&mut self, chunks: usize) {
        self.storage_mut().put(
            Bucket::Global,
            CANCEL_AFTER_CHUNKS_KEY,
            chunks.to_string().as_bytes(),
        );
        self.jobs().request_cancel();
    }

    fn job_progress_reports(&mut self) -> Vec<f64> {
        self.jobs()
            .reported_progress()
            .into_iter()
            .map(f64::from)
            .collect()
    }

    /// Focuses `pane`, then runs the job runner for `kind`.
    fn run_job_for_pane(
        &mut self,
        pane: u64,
        kind: &str,
        input: Vec<u8>,
    ) -> Result<Vec<u8>, PluginError> {
        self.deliver_one(Event::PaneFocused(Some(NodeId(pane))));
        self.settle();
        self.run_job(kind, input)
    }
}
