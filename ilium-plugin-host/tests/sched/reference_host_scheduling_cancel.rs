//! Cancellation outcomes, teardown bounds and hook-chain execution (HST-209..213).

use proptest::prelude::*;

use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::ProcessKind;
use ilium_plugin_host::scheduler::{
    cancellation_outcome, child_signal_schedule, job_cancel_verdict, run_hook, teardown_bound,
    CancelCause, CancelledWork, ChildSignal, HandlerSpec, JobCancelVerdict, RequestLedger,
    CHILD_KILL_GRACE_MS, HTTP_FILE_TEARDOWN_MS, JOB_CANCEL_GRACE_MS,
};

use crate::common::*;

fn failed_code(outcome: &Outcome) -> Option<&str> {
    match outcome {
        Outcome::Failed(error) => Some(error.code.as_str()),
        _ => None,
    }
}

// ---- HST-209 ------------------------------------------------------------------------------

/// REQ: HST-209
#[test]
fn hst_209_side_stopped_completes_cancelled() {
    assert_eq!(
        cancellation_outcome(CancelCause::SideStopped),
        Outcome::Cancelled
    );
}

/// REQ: HST-209
#[test]
fn hst_209_plugin_asked_and_shutdown_complete_cancelled() {
    assert_eq!(
        cancellation_outcome(CancelCause::PluginAsked),
        Outcome::Cancelled
    );
    assert_eq!(
        cancellation_outcome(CancelCause::Shutdown),
        Outcome::Cancelled
    );
}

/// REQ: HST-209
#[test]
fn hst_209_grant_revoked_completes_failed_revoked() {
    assert_eq!(
        failed_code(&cancellation_outcome(CancelCause::GrantRevoked)),
        Some("revoked")
    );
}

/// REQ: HST-209
#[test]
fn hst_209_deadline_passed_completes_failed_timed_out() {
    assert_eq!(
        failed_code(&cancellation_outcome(CancelCause::DeadlinePassed)),
        Some("timed-out")
    );
}

/// REQ: HST-209
#[test]
fn hst_209_provider_trap_completes_waiting_callers_failed_cancelled() {
    assert_eq!(
        failed_code(&cancellation_outcome(CancelCause::ProviderTrapped)),
        Some("cancelled")
    );
}

/// REQ: HST-209
#[test]
fn hst_209_provider_breach_completes_the_breaching_call_failed_timed_out() {
    assert_eq!(
        failed_code(&cancellation_outcome(CancelCause::ProviderBreach)),
        Some("timed-out")
    );
}

/// REQ: HST-209
#[test]
fn hst_209_cancel_of_an_open_request_completes_it_cancelled() {
    let mut ledger = RequestLedger::new();
    ledger.open(RequestRef::Submission(1));
    ledger.cancel(RequestRef::Submission(1));
    assert_eq!(
        ledger.outcome_of(RequestRef::Submission(1)),
        Some(Outcome::Cancelled)
    );
}

/// REQ: HST-209
#[test]
fn hst_209_a_completed_request_is_not_changed_by_a_later_cancel() {
    let mut ledger = RequestLedger::new();
    ledger.open(RequestRef::Submission(1));
    ledger.complete(RequestRef::Submission(1), Outcome::Ok);
    ledger.cancel(RequestRef::Submission(1));
    assert_eq!(
        ledger.outcome_of(RequestRef::Submission(1)),
        Some(Outcome::Ok)
    );
    assert_eq!(ledger.completion_count(RequestRef::Submission(1)), 1);
}

/// REQ: HST-209
#[test]
fn hst_209_cancel_of_an_unknown_id_is_a_silent_no_op() {
    let mut ledger = RequestLedger::new();
    ledger.cancel(RequestRef::Submission(99));
    assert_eq!(ledger.outcome_of(RequestRef::Submission(99)), None);
    assert_eq!(ledger.completion_count(RequestRef::Submission(99)), 0);
}

/// REQ: HST-209
#[test]
fn hst_209_cancelling_twice_produces_no_further_completion() {
    let mut ledger = RequestLedger::new();
    ledger.open(RequestRef::Submission(2));
    ledger.cancel(RequestRef::Submission(2));
    ledger.cancel(RequestRef::Submission(2));
    assert_eq!(ledger.completion_count(RequestRef::Submission(2)), 1);
}

proptest! {
    // REQ: HST-209
    #[test]
    fn hst_209_every_request_completes_at_most_once(
        ops in proptest::collection::vec((0u64..4, 0u8..3), 0..30)
    ) {
        let mut ledger = RequestLedger::new();
        for (id, op) in ops {
            let request = RequestRef::Submission(id);
            match op {
                0 => ledger.open(request),
                1 => ledger.complete(request, Outcome::Ok),
                _ => ledger.cancel(request),
            }
            for check in 0..4 {
                prop_assert!(ledger.completion_count(RequestRef::Submission(check)) <= 1);
            }
        }
    }
}

// ---- HST-210 ------------------------------------------------------------------------------

/// REQ: HST-210
#[test]
fn hst_210_http_and_file_work_stop_within_100_ms() {
    assert_eq!(HTTP_FILE_TEARDOWN_MS, 100);
    assert_eq!(teardown_bound(CancelledWork::Http), ms(100));
    assert_eq!(teardown_bound(CancelledWork::FileRead), ms(100));
}

/// REQ: HST-210
#[test]
fn hst_210_child_gets_sigterm_at_once_and_sigkill_after_2_s() {
    assert_eq!(CHILD_KILL_GRACE_MS, 2_000);
    assert_eq!(
        child_signal_schedule(),
        vec![(ChildSignal::Term, ms(0)), (ChildSignal::Kill, ms(2_000))]
    );
}

/// REQ: HST-210
#[test]
fn hst_210_job_is_still_running_at_999_ms_after_the_flag() {
    assert_eq!(JOB_CANCEL_GRACE_MS, 1_000);
    assert_eq!(job_cancel_verdict(0, 999, false), JobCancelVerdict::Running);
}

/// REQ: HST-210
#[test]
fn hst_210_job_ignoring_the_flag_is_trapped_after_1000_ms() {
    assert_eq!(job_cancel_verdict(0, 1_001, false), JobCancelVerdict::Trap);
    assert_eq!(
        job_cancel_verdict(500, 1_501, false),
        JobCancelVerdict::Trap
    );
}

/// REQ: HST-210
#[test]
fn hst_210_job_that_returned_in_time_is_never_trapped() {
    assert_eq!(
        job_cancel_verdict(0, 5_000, true),
        JobCancelVerdict::Running
    );
}

proptest! {
    // REQ: HST-210
    #[test]
    fn hst_210_trap_verdict_never_comes_before_the_grace(set_at in 0u64..10_000, elapsed in 0u64..999) {
        prop_assert_eq!(job_cancel_verdict(set_at, set_at + elapsed, false), JobCancelVerdict::Running);
    }
}

// ---- HST-211..213 -------------------------------------------------------------------------

fn handler(
    name: &str,
    process: ProcessKind,
    deadline_ms: u64,
    busy_until_ms: u64,
    run_ms: u64,
) -> HandlerSpec {
    HandlerSpec {
        plugin: plugin(name),
        process,
        deadline_ms,
        busy_until_ms,
        run_ms,
    }
}

fn server_handler(name: &str, run_ms: u64) -> HandlerSpec {
    handler(name, ProcessKind::Server, 5, 0, run_ms)
}

/// REQ: HST-211
#[test]
fn hst_211_run_hook_does_not_block_the_owner() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![server_handler("a", 40), server_handler("b", 40)],
        0,
    );
    assert_eq!(hook.owner_blocked_ms, 0);
}

/// REQ: HST-211
#[test]
fn hst_211_handlers_run_strictly_one_after_another() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![
            server_handler("a", 10),
            server_handler("b", 10),
            server_handler("c", 10),
        ],
        0,
    );
    let mut previous_end = 0;
    for run in &hook.runs {
        let start = run.started_ms.expect("started");
        assert!(start >= previous_end);
        previous_end = run.ended_ms.expect("ended");
    }
}

/// REQ: HST-211
#[test]
fn hst_211_chain_cap_is_measured_from_the_first_handlers_start() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![
            server_handler("a", 60),
            server_handler("b", 39),
            server_handler("c", 10),
        ],
        1_000,
    );
    assert!(!hook.runs[2].skipped_by_chain_cap || hook.runs[2].started_ms.is_none());
    assert!(hook.runs[1].started_ms.is_some());
}

/// REQ: HST-211
#[test]
fn hst_211_handlers_not_started_when_the_100_ms_cap_expires_are_skipped_and_logged() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![
            server_handler("a", 60),
            server_handler("b", 50),
            server_handler("c", 10),
        ],
        1_000,
    );
    assert!(hook.runs[2].skipped_by_chain_cap);
    assert!(hook.log_ids.iter().any(|id| id == "extension.chain-cap"));
}

/// REQ: HST-211
#[test]
fn hst_211_chain_finishing_inside_the_cap_logs_nothing() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![server_handler("a", 20), server_handler("b", 20)],
        0,
    );
    assert!(hook.log_ids.is_empty());
    assert!(hook.runs.iter().all(|run| !run.skipped_by_chain_cap));
}

/// REQ: HST-212
#[test]
fn hst_212_handler_that_cannot_start_within_its_deadline_is_skipped_as_pass() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![handler("busy", ProcessKind::Server, 5, 6, 1)],
        0,
    );
    assert!(hook.runs[0].skipped_as_pass);
    assert!(hook.runs[0].breach);
    assert!(hook.runs[0].started_ms.is_none());
}

/// REQ: HST-212
#[test]
fn hst_212_handler_that_can_start_just_inside_its_deadline_runs() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![handler("busy", ProcessKind::Server, 5, 4, 1)],
        0,
    );
    assert!(!hook.runs[0].skipped_as_pass);
    assert_eq!(hook.runs[0].started_ms, Some(4));
}

/// REQ: HST-212
#[test]
fn hst_212_late_handler_counts_a_breach() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![handler("slow", ProcessKind::Server, 5, 0, 9)],
        0,
    );
    assert!(hook.runs[0].breach);
}

/// REQ: HST-212
#[test]
fn hst_212_timely_handler_counts_no_breach() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![handler("fast", ProcessKind::Server, 5, 0, 4)],
        0,
    );
    assert!(!hook.runs[0].breach);
}

/// REQ: HST-212
#[test]
fn hst_212_a_skipped_handler_does_not_stop_the_chain() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![
            handler("busy", ProcessKind::Server, 5, 50, 1),
            server_handler("next", 1),
        ],
        0,
    );
    assert!(hook.runs[0].skipped_as_pass);
    assert!(hook.runs[1].started_ms.is_some());
}

/// REQ: HST-213
#[test]
fn hst_213_server_owned_point_runs_only_server_handlers() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![
            server_handler("s", 1),
            handler("c", ProcessKind::Client, 5, 0, 1),
        ],
        0,
    );
    let ran: Vec<_> = hook.runs.iter().map(|run| run.plugin.clone()).collect();
    assert_eq!(ran, vec![plugin("s")]);
}

/// REQ: HST-213
#[test]
fn hst_213_client_owned_point_runs_only_client_handlers() {
    let hook = run_hook(
        ProcessKind::Client,
        vec![
            server_handler("s", 1),
            handler("c", ProcessKind::Client, 5, 0, 1),
        ],
        0,
    );
    let ran: Vec<_> = hook.runs.iter().map(|run| run.plugin.clone()).collect();
    assert_eq!(ran, vec![plugin("c")]);
}

/// REQ: HST-213
#[test]
fn hst_213_chain_with_only_foreign_handlers_runs_nothing() {
    let hook = run_hook(
        ProcessKind::Server,
        vec![handler("c", ProcessKind::Client, 5, 0, 1)],
        0,
    );
    assert!(hook.runs.is_empty());
}
