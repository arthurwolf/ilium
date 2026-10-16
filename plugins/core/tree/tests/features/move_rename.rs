//! Page sample "Move and rename without overriding the user": a refused rename is logged and not retried.

use ilium_plugin_sdk::types::{
    ErrorCode, Event, Outcome, PluginError, RequestRef, RequestResult,
};

use crate::common::server_host;

fn refused(code: ErrorCode) -> Outcome {
    Outcome::Refused(PluginError { code, message: "refused".to_string(), retry_after_ms: None })
}

fn completed(correlation: u64, outcome: Outcome) -> Event {
    Event::RequestCompleted(RequestResult { request: RequestRef::Submission(correlation), outcome })
}

/// REQ: F-004
#[test]
fn feature_004_a_conflict_on_a_rename_logs_that_the_manual_name_is_kept() {
    let mut host = server_host();
    host.deliver_one(completed(7, refused(ErrorCode::Conflict)));
    host.settle();
    let logs = host.logs();
    assert!(
        logs.iter().any(|line| line.level == "info" && line.message == "manual name kept; not retrying"),
        "logs: {logs:?}"
    );
}

/// REQ: F-004
#[test]
fn feature_004_a_conflict_is_not_retried() {
    let mut host = server_host();
    let submissions = host.deliver_one(completed(7, refused(ErrorCode::Conflict)));
    host.settle();
    assert!(submissions.is_empty());
    assert!(host.pending_requests().is_empty());
}

/// REQ: F-004
#[test]
fn feature_004_other_refusal_codes_do_not_log_the_manual_name_line() {
    let mut host = server_host();
    for code in [ErrorCode::NotFound, ErrorCode::NotPermitted, ErrorCode::InvalidArgument, ErrorCode::Busy] {
        host.deliver_one(completed(8, refused(code)));
    }
    host.settle();
    assert!(host.logs().iter().all(|line| line.message != "manual name kept; not retrying"));
}

/// REQ: F-004
#[test]
fn feature_004_an_ok_outcome_logs_nothing_about_a_kept_name() {
    let mut host = server_host();
    host.deliver_one(completed(9, Outcome::Ok));
    host.settle();
    assert!(host.logs().iter().all(|line| line.message != "manual name kept; not retrying"));
}

/// REQ: F-004
#[test]
fn feature_004_a_failed_outcome_with_conflict_code_is_not_a_refusal() {
    let mut host = server_host();
    let failed = Outcome::Failed(PluginError { code: ErrorCode::Conflict, message: "x".to_string(), retry_after_ms: None });
    host.deliver_one(completed(10, failed));
    host.settle();
    assert!(host.logs().iter().all(|line| line.message != "manual name kept; not retrying"));
}

/// REQ: F-004
#[test]
fn feature_004_each_conflict_logs_once() {
    let mut host = server_host();
    host.deliver(vec![completed(1, refused(ErrorCode::Conflict)), completed(2, refused(ErrorCode::Conflict))]);
    host.settle();
    let count = host.logs().iter().filter(|line| line.message == "manual name kept; not retrying").count();
    assert_eq!(count, 2);
}
