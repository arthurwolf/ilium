//! Recap-lane testing hooks on `FakeHost` (onboarding state, job limits, read counters).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. The fake host cannot see which reads the
//! plugin made, so the plugin reports them through its service `ilium.core.recap/harness` (methods
//! `set-onboarding-finished`, `limits`, `reads`) and the hooks parse the answers.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

const HARNESS_SERVICE: &str = "ilium.core.recap/harness";

/// Calls one method of the plugin's harness service; a failure stops the test with the method named.
pub fn harness_call(host: &mut FakeHost, method: &str, payload: Value) -> Value {
    let text = host
        .call_service(HARNESS_SERVICE, method, &payload.to_string())
        .unwrap_or_else(|error| panic!("{HARNESS_SERVICE}.{method} failed: {error:?}"));
    if text.trim().is_empty() {
        return Value::Null;
    }
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{HARNESS_SERVICE}.{method} replied invalid JSON: {error}"))
}


/// One `agent.transcript` read the plugin made (primitives agent.wit `transcript-query`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptQuery {
    /// Pane the transcript belongs to.
    pub pane: u64,
    /// `last-turns` of the query.
    pub last_turns: u32,
    /// `max-bytes` of the query.
    pub max_bytes: u32,
}

/// Recap-lane hooks used by both the feature and the port targets. Implemented for `FakeHost`.
pub trait RecapHostExt {
    /// Marks onboarding as finished (or not) on this side.
    fn set_onboarding_finished(&mut self, finished: bool);
    /// Highest number of jobs running at once.
    fn running_job_limit(&mut self) -> usize;
    /// How often the plugin read a pane screen.
    fn screen_reads(&mut self) -> usize;
    /// Transcript queries, in order.
    fn transcript_queries(&mut self) -> Vec<TranscriptQuery>;
    /// How often the plugin read a transcript.
    fn transcript_reads(&mut self) -> usize;
}

fn count(host: &mut FakeHost, method: &str, field: &str) -> usize {
    let reply = harness_call(host, method, json!({}));
    usize::try_from(
        reply[field]
            .as_u64()
            .unwrap_or_else(|| panic!("{HARNESS_SERVICE}.{method} answers {field} as a number")),
    )
    .expect("a read count fits usize")
}

impl RecapHostExt for FakeHost {
    fn set_onboarding_finished(&mut self, finished: bool) {
        harness_call(self, "set-onboarding-finished", json!({ "finished": finished }));
    }

    fn running_job_limit(&mut self) -> usize {
        count(self, "limits", "running_jobs")
    }

    fn screen_reads(&mut self) -> usize {
        count(self, "reads", "screen")
    }

    fn transcript_queries(&mut self) -> Vec<TranscriptQuery> {
        let reply = harness_call(self, "reads", json!({}));
        reply["transcript_queries"]
            .as_array()
            .map(|queries| {
                queries
                    .iter()
                    .map(|query| TranscriptQuery {
                        pane: query["pane"].as_u64().unwrap_or_default(),
                        last_turns: u32::try_from(query["last_turns"].as_u64().unwrap_or_default())
                            .unwrap_or(u32::MAX),
                        max_bytes: u32::try_from(query["max_bytes"].as_u64().unwrap_or_default())
                            .unwrap_or(u32::MAX),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn transcript_reads(&mut self) -> usize {
        count(self, "reads", "transcript")
    }
}
