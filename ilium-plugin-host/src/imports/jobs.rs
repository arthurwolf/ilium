//! Host implementation of `ilium:host/jobs` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostJobs` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::jobs::HostJobs;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;

/// Host side of the `jobs` interface.
#[derive(Debug)]
pub struct JobsImport;

impl HostJobs for JobsImport {
    fn start(&mut self, _kind: String, _input: Vec<u8>) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::jobs::start")
    }

    fn cancel(&mut self, _job: RequestId) {
        unimplemented!("host::imports::jobs::cancel")
    }

    fn take_output(&mut self, _job: RequestId) -> Option<Vec<u8>> {
        unimplemented!("host::imports::jobs::take_output")
    }

    fn report_progress(&mut self, _percent: f32) {
        unimplemented!("host::imports::jobs::report_progress")
    }

    fn is_cancelled(&mut self) -> bool {
        unimplemented!("host::imports::jobs::is_cancelled")
    }

    fn wait_request(
        &mut self,
        _request: RequestId,
        _timeout_millis: u32,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::jobs::wait_request")
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
            Err(error) => assert_eq!(error.code, ErrorCode::TimedOut, "message: {}", error.message),
        }
    }
}
