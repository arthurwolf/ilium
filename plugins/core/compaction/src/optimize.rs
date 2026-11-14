//! The `scan` job: scans one agent's transcripts through the host `fs` import and answers the optimizer report.
//!
//! Input is `{"agent": "claude" | "codex"}`, output is `{"text": <plain-text report>}`. The scan reports its
//! byte progress to the host and stops between read chunks when the host asks it to cancel.

use ilium_plugin_sdk::host::{HostClock, HostJobs};
use ilium_plugin_sdk::types::{ErrorCode, PluginError};
use ilium_plugin_sdk::{host, HostApi};
use serde::{Deserialize, Serialize};

use crate::analysis::AgentKind;
use crate::scan::source::HostTranscriptFs;
use crate::scan::{run_scan, ScanProgressCounters, ScanSettingsInput};

/// The scan job request.
#[derive(Debug, Deserialize)]
struct ScanRequest {
    agent: String,
}

/// The scan job answer.
#[derive(Debug, Serialize)]
struct ScanAnswer {
    text: String,
}

/// The agent a pane signature names; unknown signatures scan Claude Code.
pub fn agent_for_signature(signature: &str) -> AgentKind {
    if signature == "codex" {
        AgentKind::Codex
    } else {
        AgentKind::ClaudeCode
    }
}

/// The wire name of an agent in the request.
pub fn wire_name(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::ClaudeCode => "claude",
        AgentKind::Codex => "codex",
    }
}

/// Runs the `scan` job.
pub fn run_scan_job(input: &[u8]) -> Result<Vec<u8>, PluginError> {
    let request: ScanRequest = serde_json::from_slice(input)
        .map_err(|error| PluginError::invalid_argument(format!("bad scan request: {error}")))?;
    let agent = agent_for_signature(&request.agent);
    let now_unix_ms = {
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        i64::try_from(HostClock::now_unix_millis(api)).unwrap_or(i64::MAX)
    };
    let counters = ScanProgressCounters::default();
    let stop = || {
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        let cancelled = HostJobs::is_cancelled(api);
        let total = counters.bytes_total();
        if total > 0 {
            HostJobs::report_progress(api, 100.0 * counters.bytes_done() as f32 / total as f32);
        }
        cancelled
    };
    let report = run_scan(
        agent,
        &HostTranscriptFs,
        &ScanSettingsInput::default(),
        now_unix_ms,
        &counters,
        &stop,
    )
    .map_err(|_| PluginError::new(ErrorCode::Cancelled, "The scan was cancelled."))?;
    serde_json::to_vec(&ScanAnswer {
        text: report.to_plain_text(),
    })
    .map_err(|error| PluginError::new(ErrorCode::Internal, error.to_string()))
}
