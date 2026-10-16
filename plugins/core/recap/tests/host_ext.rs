//! Recap-lane testing hooks on `FakeHost` (job gate counters, onboarding state, scripted answers).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. Skeleton bodies.

#![allow(dead_code)]

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::Value;

/// Recap-lane hooks. Implemented for `FakeHost` with skeleton bodies.
pub trait RecapHostExt {
    /// Answers the next client request of `kind` with `reply_json`; `None` when none is pending.
    fn answer_client_request(&mut self, _kind: &str, _reply_json: &str) -> Option<String> { unimplemented!("recap.answer-client-request") }
    /// Leaves every client request unanswered so timeouts apply.
    fn leave_client_requests_unanswered(&mut self) { unimplemented!("recap.leave-client-requests-unanswered") }
    /// Marks onboarding as finished (or not) on this side.
    fn set_onboarding_finished(&mut self, _finished: bool) { unimplemented!("recap.set-onboarding-finished") }
    /// Cancels the job currently running.
    fn cancel_current_job(&mut self) { unimplemented!("recap.cancel-current-job") }
    /// Scripts the text of the next inference answer.
    fn set_inference_answer(&mut self, _answer: String) { unimplemented!("recap.set-inference-answer") }
    /// Wait budgets (ms) of the inference calls made.
    fn inference_wait_ms(&self) -> Vec<u64> { unimplemented!("recap.inference-wait-ms") }
    /// Progress values the job reported.
    fn reported_progress(&self) -> Vec<f64> { unimplemented!("recap.reported-progress") }
    /// Highest number of jobs running at once.
    fn running_job_limit(&self) -> usize { unimplemented!("recap.running-job-limit") }
    /// How often the plugin read a pane screen.
    fn screen_reads(&self) -> usize { unimplemented!("recap.screen-reads") }
    /// Screen read requests, as JSON.
    fn screen_requests(&self) -> Vec<Value> { unimplemented!("recap.screen-requests") }
    /// Transcript queries, as JSON.
    fn transcript_queries(&self) -> Vec<Value> { unimplemented!("recap.transcript-queries") }
    /// How often the plugin read a transcript.
    fn transcript_reads(&self) -> usize { unimplemented!("recap.transcript-reads") }
}

impl RecapHostExt for FakeHost {}
