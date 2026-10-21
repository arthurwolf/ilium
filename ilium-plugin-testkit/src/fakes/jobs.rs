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

#[cfg(test)]
mod spec_tests {
    use crate::FakeHost;
    use ilium_plugin_sdk_core::types::PluginError;

    /// REQ: DEV-472 (`jobs().list()` lists the jobs the plugin started; a host where the plugin started none lists nothing)
    #[test]
    fn dev_472_fresh_host_lists_no_jobs() {
        let mut host = FakeHost::new();
        assert!(host.jobs().list().is_empty());
    }

    /// REQ: DEV-472 (`finish` completes a job with `request-completed`; it does not record a new job start)
    #[test]
    fn dev_472_finish_with_ok_output_does_not_add_a_job_record() {
        let mut host = FakeHost::new();
        host.jobs().finish(1, Ok(b"hits".to_vec()));
        assert!(host.jobs().list().is_empty());
    }

    /// REQ: DEV-472 (`finish` with `Err(plugin-error)` is accepted as a failed completion and leaves the job list unchanged)
    #[test]
    fn dev_472_finish_with_error_leaves_job_list_unchanged() {
        let mut host = FakeHost::new();
        host.jobs().finish(3, Err(PluginError::timed_out("deadline reached")));
        assert!(host.jobs().list().is_empty());
    }

    /// REQ: DEV-472 (`progress` delivers `job-progress` for a request; an unknown request number with an empty message is still delivered, and records no job)
    #[test]
    fn dev_472_progress_with_empty_message_records_no_job() {
        let mut host = FakeHost::new();
        host.jobs().progress(9, 10.0, "");
        assert!(host.jobs().list().is_empty());
    }

    /// REQ: DEV-472 (lane hook: `cancelled()` lists only cancels the plugin issued, so a fresh host reports none)
    #[test]
    fn dev_472_fresh_host_reports_no_cancelled_jobs() {
        let mut host = FakeHost::new();
        assert!(host.jobs().cancelled().is_empty());
    }

    /// REQ: DEV-472 (lane hook: `reported_progress()` lists the percent values the plugin reported through `jobs.progress`, so a fresh host reports none)
    #[test]
    fn dev_472_fresh_host_reports_no_plugin_progress() {
        let mut host = FakeHost::new();
        assert!(host.jobs().reported_progress().is_empty());
    }

    /// REQ: DEV-472 (`request_cancel` makes the next job see a cancel request; it is a host-side signal, so no plugin cancel is recorded)
    #[test]
    fn dev_472_host_cancel_request_is_not_recorded_as_a_plugin_cancel() {
        let mut host = FakeHost::new();
        host.jobs().request_cancel();
        assert!(host.jobs().cancelled().is_empty());
    }

    /// REQ: DEV-472 (the job-side `jobs.start` returns `invalid-argument` for an unknown kind; `finish` may carry that error and the list stays empty)
    #[test]
    fn dev_472_invalid_argument_completion_leaves_job_list_empty() {
        let mut host = FakeHost::new();
        host.jobs()
            .finish(4, Err(PluginError::invalid_argument("unknown job kind")));
        assert!(host.jobs().list().is_empty());
    }
}
