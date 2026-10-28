//! Fake jobs (reference-test-harness.md "Fake tree, panes, agents and jobs").

use ilium_plugin_sdk_core::generated::types::{
    Event, JobProgressEvent, Outcome, RequestId, RequestRef, RequestResult,
};
use ilium_plugin_sdk_core::types::PluginError;

use super::tree::Outbox;

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
#[derive(Debug, Default)]
pub struct FakeJobs {
    jobs: Vec<JobRecord>,
    cancelled: Vec<u64>,
    progress: Vec<f32>,
    cancel_requested: bool,
    outbox: Outbox,
}

impl FakeJobs {
    /// Jobs that publish events into `outbox`.
    pub(crate) fn with_outbox(outbox: Outbox) -> FakeJobs {
        FakeJobs {
            outbox,
            ..FakeJobs::default()
        }
    }

    /// Records a job the plugin started.
    pub(crate) fn record(&mut self, request: u64, kind: &str, input: &[u8]) {
        self.jobs.push(JobRecord {
            request,
            kind: kind.to_string(),
            input: input.to_vec(),
        });
    }

    /// Records that the plugin cancelled a job.
    pub(crate) fn record_cancel(&mut self, request: u64) {
        self.cancelled.push(request);
    }

    /// Records progress the plugin reported.
    pub(crate) fn record_progress(&mut self, percent: f32) {
        self.progress.push(percent);
    }

    /// True once after `request_cancel`.
    pub(crate) fn take_cancel_request(&mut self) -> bool {
        std::mem::take(&mut self.cancel_requested)
    }

    /// Jobs the plugin started.
    pub fn list(&self) -> Vec<JobRecord> {
        self.jobs.clone()
    }

    /// Delivers one `request-completed` event for the job.
    pub fn finish(&mut self, request: u64, result: Result<Vec<u8>, PluginError>) {
        let outcome = match result {
            Ok(_) => Outcome::Ok,
            Err(error) => Outcome::Failed(error),
        };
        self.outbox
            .borrow_mut()
            .push(Event::RequestCompleted(RequestResult {
                request: RequestRef::Call(RequestId(request)),
                outcome,
            }));
    }

    /// Delivers `job-progress`.
    pub fn progress(&mut self, request: u64, percent: f32, _message: &str) {
        self.outbox
            .borrow_mut()
            .push(Event::JobProgress(JobProgressEvent {
                job: RequestId(request),
                percent,
            }));
    }

    /// Request numbers of the jobs the plugin cancelled, in order (lane hook, see manual_gaps_fakes.md; DEV-472).
    pub fn cancelled(&self) -> Vec<u64> {
        self.cancelled.clone()
    }

    /// Percent values the plugin reported through `jobs.progress`, in order (lane hook, see manual_gaps_fakes.md).
    pub fn reported_progress(&self) -> Vec<f32> {
        self.progress.clone()
    }

    /// Makes the next job run see a cancel request (lane hook, see manual_gaps_fakes.md; DEV-472).
    pub fn request_cancel(&mut self) {
        self.cancel_requested = true;
    }
}

#[cfg(test)]
mod spec_tests {
    use crate::FakeHost;
    use ilium_plugin_sdk_core::types::PluginError;

    /// REQ: DEV-472
    #[test]
    fn dev_472_fresh_host_lists_no_jobs() {
        let mut host = FakeHost::new();
        assert!(host.jobs().list().is_empty());
    }

    /// REQ: DEV-472
    #[test]
    fn dev_472_finish_with_ok_output_does_not_add_a_job_record() {
        let mut host = FakeHost::new();
        host.jobs().finish(1, Ok(b"hits".to_vec()));
        assert!(host.jobs().list().is_empty());
    }

    /// REQ: DEV-472
    #[test]
    fn dev_472_finish_with_error_leaves_job_list_unchanged() {
        let mut host = FakeHost::new();
        host.jobs()
            .finish(3, Err(PluginError::timed_out("deadline reached")));
        assert!(host.jobs().list().is_empty());
    }

    /// REQ: DEV-472
    #[test]
    fn dev_472_progress_with_empty_message_records_no_job() {
        let mut host = FakeHost::new();
        host.jobs().progress(9, 10.0, "");
        assert!(host.jobs().list().is_empty());
    }

    /// REQ: DEV-472
    #[test]
    fn dev_472_fresh_host_reports_no_cancelled_jobs() {
        let mut host = FakeHost::new();
        assert!(host.jobs().cancelled().is_empty());
    }

    /// REQ: DEV-472
    #[test]
    fn dev_472_fresh_host_reports_no_plugin_progress() {
        let mut host = FakeHost::new();
        assert!(host.jobs().reported_progress().is_empty());
    }

    /// REQ: DEV-472
    #[test]
    fn dev_472_host_cancel_request_is_not_recorded_as_a_plugin_cancel() {
        let mut host = FakeHost::new();
        host.jobs().request_cancel();
        assert!(host.jobs().cancelled().is_empty());
    }

    /// REQ: DEV-472
    #[test]
    fn dev_472_invalid_argument_completion_leaves_job_list_empty() {
        let mut host = FakeHost::new();
        host.jobs()
            .finish(4, Err(PluginError::invalid_argument("unknown job kind")));
        assert!(host.jobs().list().is_empty());
    }
}
