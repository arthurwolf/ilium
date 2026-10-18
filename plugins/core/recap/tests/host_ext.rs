//! Recap-lane testing hooks on `FakeHost` (onboarding state, job limits, read counters).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. Skeleton bodies.

use ilium_plugin_sdk::testing::FakeHost;

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

/// Recap-lane hooks used by both the feature and the port targets. Implemented for `FakeHost` with
/// skeleton bodies.
pub trait RecapHostExt {
    /// Marks onboarding as finished (or not) on this side.
    fn set_onboarding_finished(&mut self, _finished: bool) {
        unimplemented!("recap.set-onboarding-finished")
    }
    /// Highest number of jobs running at once.
    fn running_job_limit(&self) -> usize {
        unimplemented!("recap.running-job-limit")
    }
    /// How often the plugin read a pane screen.
    fn screen_reads(&self) -> usize {
        unimplemented!("recap.screen-reads")
    }
    /// Transcript queries, in order.
    fn transcript_queries(&self) -> Vec<TranscriptQuery> {
        unimplemented!("recap.transcript-queries")
    }
    /// How often the plugin read a transcript.
    fn transcript_reads(&self) -> usize {
        unimplemented!("recap.transcript-reads")
    }
}

impl RecapHostExt for FakeHost {}
