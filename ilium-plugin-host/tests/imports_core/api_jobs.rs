//! api-jobs.md: `start`, `cancel`, `take-output`, `report-progress`, `is-cancelled`,
//! `wait-request`, and what a job instance may call.

use ilium_plugin_api::generated::host::{HostJobs, HostSettings};
use ilium_plugin_api::generated::types::{ErrorCode, Outcome, PluginError, RequestId};
use ilium_plugin_api::Tier;
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;
use proptest::prelude::*;
use serde_json::json;

use crate::support::*;

const KIND: &str = "work";

fn manifest_with_job() -> String {
    let mut manifest = minimal_manifest();
    manifest["contributes"] = json!({ "jobs": [{ "kind": KIND, "title": "Work" }] });
    manifest.to_string()
}

fn job_fixture(runner: Box<dyn FnMut(&str, &[u8]) -> Result<Vec<u8>, PluginError>>) -> ImportsFixture {
    ImportsFixture::builder()
        .tier(Tier::T2)
        .grants(&["jobs.run"])
        .manifest_json(&manifest_with_job())
        .job_runner(runner)
        .build()
}

fn echo_fixture() -> ImportsFixture {
    job_fixture(Box::new(|_kind, input| Ok(input.to_vec())))
}

fn failing(code: ErrorCode) -> ImportsFixture {
    job_fixture(Box::new(move |_kind, _input| {
        Err(PluginError { code, message: "boom".to_owned(), retry_after_ms: None })
    }))
}

// ---- start, take-output ----------------------------------------------------------------------

/// REQ: WIT:ilium:host/jobs.start, WIT:ilium:host/jobs.take-output
#[test]
fn jobs_start_runs_the_kind_and_take_output_returns_its_result_once() {
    let mut fixture = echo_fixture();
    let job = expect_ok(fixture.jobs().start(KIND.to_owned(), b"input".to_vec()));
    assert!(is_ok(&complete(&mut fixture, job)));
    assert_eq!(fixture.jobs().take_output(job), Some(b"input".to_vec()));
    assert_eq!(fixture.jobs().take_output(job), None);
}

/// REQ: WIT:ilium:host/jobs.take-output
#[test]
fn jobs_take_output_for_an_unknown_job_is_none() {
    let mut fixture = echo_fixture();
    assert_eq!(fixture.jobs().take_output(RequestId(808_080)), None);
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_without_the_jobs_run_grant_not_permitted() {
    let mut fixture = ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&manifest_with_job())
        .build();
    assert_eq!(expect_code(fixture.jobs().start(KIND.to_owned(), Vec::new())), ErrorCode::NotPermitted);
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_undeclared_kind_invalid_argument() {
    let mut fixture = echo_fixture();
    assert_eq!(expect_code(fixture.jobs().start("undeclared".to_owned(), Vec::new())), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_input_of_exactly_16_mib_accepted_and_one_more_invalid_argument() {
    let mut fixture = echo_fixture();
    assert!(fixture.jobs().start(KIND.to_owned(), vec![0u8; 16 * MIB]).is_ok());
    assert_eq!(
        expect_code(fixture.jobs().start(KIND.to_owned(), vec![0u8; 16 * MIB + 1])),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_empty_input_is_accepted() {
    let mut fixture = echo_fixture();
    let job = expect_ok(fixture.jobs().start(KIND.to_owned(), Vec::new()));
    assert!(is_ok(&complete(&mut fixture, job)));
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_a_full_queue_is_busy() {
    let mut fixture = echo_fixture();
    fixture.hold_async_completions(true);
    let mut refused = None;
    for _ in 0..1000 {
        match fixture.jobs().start(KIND.to_owned(), Vec::new()) {
            Ok(_) => {}
            Err(error) => {
                refused = Some(error.code);
                break;
            }
        }
    }
    assert_eq!(refused, Some(ErrorCode::Busy));
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_run_error_fails_the_request_with_that_code() {
    let mut fixture = failing(ErrorCode::InvalidArgument);
    let job = expect_ok(fixture.jobs().start(KIND.to_owned(), Vec::new()));
    match complete(&mut fixture, job) {
        Outcome::Failed(error) => {
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert_eq!(error.message, "boom");
        }
        other => panic!("expected failed, got {other:?}"),
    }
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_a_trapping_job_fails_internal() {
    let mut fixture = echo_fixture();
    fixture.make_next_job_trap();
    let job = expect_ok(fixture.jobs().start(KIND.to_owned(), Vec::new()));
    assert_eq!(outcome_code(&complete(&mut fixture, job)), Some(ErrorCode::Internal));
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_a_job_over_its_time_budget_fails_timed_out() {
    let mut fixture = echo_fixture();
    fixture.make_next_job_run_forever();
    let job = expect_ok(fixture.jobs().start(KIND.to_owned(), Vec::new()));
    fixture.advance(std::time::Duration::from_secs(3600));
    assert_eq!(outcome_code(&complete(&mut fixture, job)), Some(ErrorCode::TimedOut));
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_a_job_over_its_memory_limit_fails_quota_exceeded() {
    let mut fixture = echo_fixture();
    fixture.make_next_job_exceed_memory();
    let job = expect_ok(fixture.jobs().start(KIND.to_owned(), Vec::new()));
    assert_eq!(outcome_code(&complete(&mut fixture, job)), Some(ErrorCode::QuotaExceeded));
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_output_over_the_limit_fails_invalid_argument() {
    let mut fixture = job_fixture(Box::new(|_kind, _input| Ok(vec![0u8; 16 * MIB + 1])));
    let job = expect_ok(fixture.jobs().start(KIND.to_owned(), Vec::new()));
    assert_eq!(outcome_code(&complete(&mut fixture, job)), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_failed_jobs_never_count_toward_the_restart_rule() {
    let mut fixture = failing(ErrorCode::Internal);
    for _ in 0..10 {
        let job = expect_ok(fixture.jobs().start(KIND.to_owned(), Vec::new()));
        complete(&mut fixture, job);
    }
    assert!(!fixture.plugin_is_stopped());
}

proptest! {
    // REQ: WIT:ilium:host/jobs.start
    #[test]
    fn jobs_start_output_equals_what_the_runner_returned(input in proptest::collection::vec(any::<u8>(), 0..512)) {
        let mut fixture = echo_fixture();
        let job = expect_ok(fixture.jobs().start(KIND.to_owned(), input.clone()));
        prop_assert!(is_ok(&complete(&mut fixture, job)));
        prop_assert_eq!(fixture.jobs().take_output(job), Some(input));
    }
}

// ---- cancel, is-cancelled, report-progress ---------------------------------------------------

/// REQ: WIT:ilium:host/jobs.cancel
#[test]
fn jobs_cancel_a_waiting_job_completes_cancelled() {
    let mut fixture = echo_fixture();
    fixture.hold_async_completions(true);
    let job = expect_ok(fixture.jobs().start(KIND.to_owned(), Vec::new()));
    fixture.jobs().cancel(job);
    fixture.hold_async_completions(false);
    assert!(matches!(complete(&mut fixture, job), Outcome::Cancelled));
}

/// REQ: WIT:ilium:host/jobs.cancel
#[test]
fn jobs_cancel_of_a_finished_job_changes_nothing() {
    let mut fixture = echo_fixture();
    let job = expect_ok(fixture.jobs().start(KIND.to_owned(), b"x".to_vec()));
    assert!(is_ok(&complete(&mut fixture, job)));
    fixture.jobs().cancel(job);
    assert_eq!(fixture.jobs().take_output(job), Some(b"x".to_vec()));
}

/// REQ: WIT:ilium:host/jobs.cancel
#[test]
fn jobs_cancel_of_an_unknown_job_is_harmless() {
    let mut fixture = echo_fixture();
    fixture.jobs().cancel(RequestId(1_111_111));
    assert!(fixture.jobs().start(KIND.to_owned(), Vec::new()).is_ok());
}

/// REQ: WIT:ilium:host/jobs.is-cancelled
#[test]
fn jobs_is_cancelled_is_false_for_a_running_job_and_true_after_cancel() {
    let mut fixture = echo_fixture();
    let mut instance = fixture.job_instance(KIND, b"");
    assert!(!instance.jobs().is_cancelled());
    instance.cancel_from_host();
    assert!(instance.jobs().is_cancelled());
}

/// REQ: WIT:ilium:host/jobs.report-progress
#[test]
fn jobs_report_progress_values_reach_the_host_clamped_to_0_100() {
    let mut fixture = echo_fixture();
    let mut instance = fixture.job_instance(KIND, b"");
    for percent in [-5.0f32, 0.0, 42.5, 100.0, 250.0] {
        instance.jobs().report_progress(percent);
    }
    assert_eq!(instance.reported_progress(), vec![0.0, 0.0, 42.5, 100.0, 100.0]);
}

/// REQ: WIT:ilium:host/jobs.report-progress
#[test]
fn jobs_report_progress_not_a_number_is_ignored() {
    let mut fixture = echo_fixture();
    let mut instance = fixture.job_instance(KIND, b"");
    instance.jobs().report_progress(f32::NAN);
    assert!(instance.reported_progress().iter().all(|value| value.is_finite()));
}

// ---- wait-request ----------------------------------------------------------------------------

/// REQ: WIT:ilium:host/jobs.wait-request
#[test]
fn jobs_wait_request_returns_once_the_awaited_request_completes() {
    let mut fixture = echo_fixture();
    let awaited = fixture.start_background_request();
    let mut instance = fixture.job_instance(KIND, b"");
    fixture.complete_background_request(awaited);
    expect_ok(instance.jobs().wait_request(awaited, 1000));
}

/// REQ: WIT:ilium:host/jobs.wait-request
#[test]
fn jobs_wait_request_unknown_request_not_found() {
    let mut fixture = echo_fixture();
    let mut instance = fixture.job_instance(KIND, b"");
    assert_eq!(expect_code(instance.jobs().wait_request(RequestId(999_001), 10)), ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/jobs.wait-request
#[test]
fn jobs_wait_request_timeout_elapsing_timed_out() {
    let mut fixture = echo_fixture();
    let awaited = fixture.start_background_request();
    let mut instance = fixture.job_instance(KIND, b"");
    assert_eq!(expect_code(instance.jobs().wait_request(awaited, 50)), ErrorCode::TimedOut);
}

/// REQ: WIT:ilium:host/jobs.wait-request
#[test]
fn jobs_wait_request_outside_a_job_run_unsupported() {
    let mut fixture = echo_fixture();
    assert_eq!(expect_code(fixture.jobs().wait_request(RequestId(1), 10)), ErrorCode::Unsupported);
}

// ---- what a job instance may call ------------------------------------------------------------

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_start_from_inside_a_job_instance_unsupported() {
    let mut fixture = echo_fixture();
    let mut instance = fixture.job_instance(KIND, b"");
    assert_eq!(expect_code(instance.jobs().start(KIND.to_owned(), Vec::new())), ErrorCode::Unsupported);
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_job_instance_calls_to_settings_are_unsupported() {
    let mut fixture = echo_fixture();
    let mut instance = fixture.job_instance(KIND, b"");
    assert_eq!(expect_code(instance.settings().set_json("{}".to_owned())), ErrorCode::Unsupported);
}

/// REQ: WIT:ilium:host/jobs.start
#[test]
fn jobs_job_instance_may_call_clock_and_log() {
    use ilium_plugin_api::generated::host::{HostClock, HostLog, Level};
    let mut fixture = echo_fixture();
    let mut instance = fixture.job_instance(KIND, b"");
    assert_eq!(instance.clock().now_unix_millis(), TEST_CLOCK_START_MILLIS);
    instance.log().log(Level::Info, "from the job".to_owned());
    assert!(fixture.logs().iter().any(|line| line.message == "from the job"));
}
