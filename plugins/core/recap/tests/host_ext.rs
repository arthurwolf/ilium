//! Recap-lane testing hooks on `FakeHost` (job gate counters, onboarding state, scripted answers).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. Skeleton bodies.

#![allow(dead_code)]

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

/// One `read_screen` the plugin made (API_GAPS.md: F-112 reads the visible part, 40 rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenRequest {
    /// Pane whose screen was read.
    pub pane: u64,
    /// Rows requested.
    pub rows: u16,
    /// Whether only the visible part was requested.
    pub visible_only: bool,
}

/// Recap-lane hooks. Implemented for `FakeHost` with skeleton bodies.
pub trait RecapHostExt {
    /// Answers the next client request of `kind` with `reply_json`; `None` when none is pending.
    fn answer_client_request(&mut self, _kind: &str, _reply_json: &str) -> Option<String> {
        unimplemented!("recap.answer-client-request")
    }
    /// Leaves every client request unanswered so timeouts apply.
    fn leave_client_requests_unanswered(&mut self) {
        unimplemented!("recap.leave-client-requests-unanswered")
    }
    /// Marks onboarding as finished (or not) on this side.
    fn set_onboarding_finished(&mut self, _finished: bool) {
        unimplemented!("recap.set-onboarding-finished")
    }
    /// Cancels the job currently running.
    fn cancel_current_job(&mut self) {
        unimplemented!("recap.cancel-current-job")
    }
    /// Scripts the text of the next inference answer.
    fn set_inference_answer(&mut self, _answer: String) {
        unimplemented!("recap.set-inference-answer")
    }
    /// Wait budgets (ms) of the inference calls made.
    fn inference_wait_ms(&self) -> Vec<u64> {
        unimplemented!("recap.inference-wait-ms")
    }
    /// Progress values the job reported.
    fn reported_progress(&self) -> Vec<f64> {
        unimplemented!("recap.reported-progress")
    }
    /// Highest number of jobs running at once.
    fn running_job_limit(&self) -> usize {
        unimplemented!("recap.running-job-limit")
    }
    /// How often the plugin read a pane screen.
    fn screen_reads(&self) -> usize {
        unimplemented!("recap.screen-reads")
    }
    /// Screen read requests, in order.
    fn screen_requests(&self) -> Vec<ScreenRequest> {
        unimplemented!("recap.screen-requests")
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
