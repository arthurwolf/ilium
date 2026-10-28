//! Host implementation of `ilium:host/jobs`.
//!
//! No `contributes.jobs` registry is wired into the import yet, so every kind is undeclared and
//! `start` refuses it; cancellation and progress state is kept so the guest-side calls stay total.

use ilium_plugin_api::generated::host::jobs::HostJobs;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::RequestId;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

/// Default `job_io_mib` limit for a job's input and output: 16 MiB.
const JOB_IO_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// Requests that were cancelled; a cancelled job never yields output.
static CANCELLED_REQUESTS: Mutex<Option<HashSet<u64>>> = Mutex::new(None);

/// Last reported progress of the running job, as `f32` bits (0 to 100).
static REPORTED_PROGRESS_BITS: AtomicU32 = AtomicU32::new(0);

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

/// Host side of the `jobs` interface.
#[derive(Debug)]
pub struct JobsImport;

impl HostJobs for JobsImport {
    fn start(&mut self, kind: String, input: Vec<u8>) -> Result<RequestId, PluginError> {
        if kind.is_empty() {
            return Err(error(ErrorCode::InvalidArgument, "job kind is empty"));
        }
        if input.len() > JOB_IO_LIMIT_BYTES {
            return Err(error(
                ErrorCode::InvalidArgument,
                "job input exceeds the job_io_mib limit",
            ));
        }
        Err(error(
            ErrorCode::InvalidArgument,
            "job kind is not declared in contributes.jobs",
        ))
    }

    fn cancel(&mut self, job: RequestId) {
        let mut guard = CANCELLED_REQUESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        guard.get_or_insert_with(HashSet::new).insert(job.0);
    }

    fn take_output(&mut self, _job: RequestId) -> Option<Vec<u8>> {
        // No job ever runs, so no output exists; cancelled jobs never have output either.
        None
    }

    fn report_progress(&mut self, percent: f32) {
        let clamped = if percent.is_nan() {
            0.0
        } else {
            percent.clamp(0.0, 100.0)
        };
        REPORTED_PROGRESS_BITS.store(clamped.to_bits(), Ordering::Relaxed);
    }

    fn is_cancelled(&mut self) -> bool {
        // The calling job's cancel flag is not tracked per job yet; a plain call is not cancelled.
        false
    }

    fn wait_request(
        &mut self,
        request: RequestId,
        _timeout_millis: u32,
    ) -> Result<(), PluginError> {
        let guard = CANCELLED_REQUESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if guard.as_ref().is_some_and(|set| set.contains(&request.0)) {
            return Err(error(ErrorCode::Cancelled, "request was cancelled"));
        }
        // No request can complete, so the wait always runs into its timeout.
        Err(error(ErrorCode::TimedOut, "request did not finish in time"))
    }
}

#[cfg(test)]
mod tests {
    use super::JobsImport;
    use ilium_plugin_api::generated::host::jobs::HostJobs;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::RequestId;

    /// Default `job_io_mib` limit for a job's input and for its output: 16 MiB.
    const DEFAULT_JOB_IO_BYTES: usize = 16 * 1024 * 1024;

    /// A job kind that no `contributes.jobs` entry declares in these unit tests.
    const UNDECLARED_KIND: &str = "undeclared-test-kind";

    /// Asserts that a start call was refused with the expected error code.
    fn assert_start_refused(result: Result<RequestId, PluginError>, expected: ErrorCode) {
        match result {
            Ok(request) => panic!(
                "start must be refused with {expected:?}, but it returned request {}",
                request.0
            ),
            Err(error) => assert_eq!(
                error.code, expected,
                "wrong error code, message: {}",
                error.message
            ),
        }
    }

    /// REQ: API-002
    #[test]
    fn start_with_undeclared_kind_is_refused_as_invalid_argument() {
        let mut jobs = JobsImport;
        let result = jobs.start(UNDECLARED_KIND.to_string(), b"input".to_vec());
        assert_start_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-002
    #[test]
    fn start_with_empty_kind_is_refused_as_invalid_argument() {
        let mut jobs = JobsImport;
        let result = jobs.start(String::new(), Vec::new());
        assert_start_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-002
    #[test]
    fn start_with_input_over_job_io_limit_is_refused_as_invalid_argument() {
        let mut jobs = JobsImport;
        let oversized_input = vec![0u8; DEFAULT_JOB_IO_BYTES + 1];
        let result = jobs.start(UNDECLARED_KIND.to_string(), oversized_input);
        assert_start_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: ABI-012
    #[test]
    fn take_output_of_unknown_request_returns_none() {
        let mut jobs = JobsImport;
        assert_eq!(jobs.take_output(RequestId(1)), None);
    }

    /// REQ: ABI-012
    #[test]
    fn take_output_of_request_zero_returns_none() {
        let mut jobs = JobsImport;
        assert_eq!(jobs.take_output(RequestId(0)), None);
    }

    /// REQ: ABI-012
    #[test]
    fn take_output_of_largest_request_id_returns_none() {
        let mut jobs = JobsImport;
        assert_eq!(jobs.take_output(RequestId(u64::MAX)), None);
    }

    /// REQ: API-074
    #[test]
    fn cancel_of_unknown_request_is_a_silent_no_op() {
        let mut jobs = JobsImport;
        jobs.cancel(RequestId(u64::MAX));
        assert_eq!(jobs.take_output(RequestId(u64::MAX)), None);
    }

    /// REQ: RQO-006
    #[test]
    fn cancelled_job_has_no_output_to_take() {
        let mut jobs = JobsImport;
        jobs.cancel(RequestId(7));
        assert_eq!(jobs.take_output(RequestId(7)), None);
    }

    /// REQ: HST-210
    #[test]
    fn is_cancelled_is_false_when_no_cancel_was_requested() {
        let mut jobs = JobsImport;
        assert!(!jobs.is_cancelled());
    }

    /// REQ: WID-023
    #[test]
    fn report_progress_accepts_nan_and_out_of_range_values_without_panicking() {
        let mut jobs = JobsImport;
        jobs.report_progress(f32::NAN);
        jobs.report_progress(-5.0);
        jobs.report_progress(0.0);
        jobs.report_progress(100.0);
        jobs.report_progress(150.0);
    }

    /// REQ: HST-499
    #[test]
    fn wait_request_with_zero_timeout_on_unfinished_request_times_out() {
        let mut jobs = JobsImport;
        match jobs.wait_request(RequestId(42), 0) {
            Ok(()) => panic!("an unfinished request must not complete within a zero timeout"),
            Err(error) => assert_eq!(
                error.code,
                ErrorCode::TimedOut,
                "message: {}",
                error.message
            ),
        }
    }
}
