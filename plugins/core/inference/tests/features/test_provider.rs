//! Sample 4 of the page: the Test provider button. A `button` row runs the command
//! `test-provider`; the plugin sends one short request and, when `request-completed` arrives,
//! writes latency and model into the `last_test` info row.

use ilium_plugin_sdk::types::{ErrorCode, Event, Outcome, RequestId, RequestRef, RequestResult};
use ilium_plugin_sdk::testing::FakeHost;
use proptest::prelude::*;

use crate::common::{completion, error, row_command, server_host, TEST_COMMAND};

fn started_host() -> (FakeHost, RequestRef) {
    let mut host = server_host();
    host.deliver_one(row_command(TEST_COMMAND));
    let pending = host.pending_requests();
    assert_eq!(pending.len(), 1, "one request per run");
    (host, pending[0].clone())
}

fn finish_ok(host: &mut FakeHost, request: RequestRef, text: &str, model: &str, elapsed_ms: u64) {
    let RequestRef::Call(call) = request.clone() else { panic!("the test run is one slow inference call") };
    host.inference().complete_inference(call, completion(text, model));
    host.advance_clock(elapsed_ms);
    host.complete(request, Outcome::Ok);
    host.settle();
}

/// REQ: F-084
#[test]
fn test_084_the_button_command_starts_exactly_one_request() {
    let (host, _) = started_host();
    assert_eq!(host.pending_requests().len(), 1);
}

/// REQ: F-084
#[test]
fn test_084_the_request_is_one_user_message_with_the_fixed_prompt() {
    let (mut host, _) = started_host();
    let requests = host.inference().requests();
    let messages = &requests[0].1.messages;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[0].content, "Reply with the word ok.");
}

/// REQ: F-084
#[test]
fn test_084_the_request_allows_16_output_tokens() {
    let (mut host, _) = started_host();
    assert_eq!(host.inference().requests()[0].1.max_output_tokens, Some(16));
}

/// REQ: F-084
#[test]
fn test_084_the_request_has_no_json_schema() {
    let (mut host, _) = started_host();
    assert_eq!(host.inference().requests()[0].1.json_schema, None);
}

/// REQ: F-084, F-085
#[test]
fn test_085_the_request_names_the_provider_test_purpose_for_the_admission_gate() {
    let (mut host, _) = started_host();
    assert_eq!(host.inference().requests()[0].1.purpose, "provider-test");
}

/// REQ: F-084
#[test]
fn test_084_before_any_run_the_info_row_reads_not_run_yet() {
    let mut host = server_host();
    assert_eq!(host.settings().info("last_test"), Some("Not run yet".to_string()));
}

/// REQ: F-084
#[test]
fn test_084_starting_a_run_does_not_change_the_info_row() {
    let (mut host, _) = started_host();
    assert_eq!(host.settings().info("last_test"), Some("Not run yet".to_string()));
}

/// REQ: F-084
#[test]
fn test_084_a_completed_request_writes_latency_and_model() {
    let (mut host, request) = started_host();
    finish_ok(&mut host, request, "ok", "gpt-5.6-terra", 812);
    assert_eq!(host.settings().info("last_test"), Some("812 ms, gpt-5.6-terra".to_string()));
}

/// REQ: F-084
#[test]
fn test_084_zero_elapsed_milliseconds_are_written_as_zero() {
    let (mut host, request) = started_host();
    finish_ok(&mut host, request, "ok", "m", 0);
    assert_eq!(host.settings().info("last_test"), Some("0 ms, m".to_string()));
}

/// REQ: F-084
#[test]
fn test_084_a_failed_outcome_writes_failed() {
    let (mut host, request) = started_host();
    host.complete(request, Outcome::Failed(error(ErrorCode::Network)));
    host.settle();
    assert_eq!(host.settings().info("last_test"), Some("failed".to_string()));
}

/// REQ: F-084
#[test]
fn test_084_a_refused_outcome_writes_failed() {
    let (mut host, request) = started_host();
    host.complete(request, Outcome::Refused(error(ErrorCode::NotPermitted)));
    host.settle();
    assert_eq!(host.settings().info("last_test"), Some("failed".to_string()));
}

/// REQ: F-084
#[test]
fn test_084_a_cancelled_outcome_writes_failed() {
    let (mut host, request) = started_host();
    host.complete(request, Outcome::Cancelled);
    host.settle();
    assert_eq!(host.settings().info("last_test"), Some("failed".to_string()));
}

/// REQ: F-084
#[test]
fn test_084_a_result_for_another_request_leaves_the_row_alone() {
    let (mut host, _) = started_host();
    host.deliver_one(Event::RequestCompleted(RequestResult {
        request: RequestRef::Call(RequestId(987_654)),
        outcome: Outcome::Ok,
    }));
    assert_eq!(host.settings().info("last_test"), Some("Not run yet".to_string()));
}

/// REQ: F-084
#[test]
fn test_084_a_command_of_another_name_starts_no_request() {
    let mut host = server_host();
    host.deliver_one(row_command("ilium.core.inference/other"));
    assert!(host.pending_requests().is_empty());
}

/// REQ: F-084
#[test]
fn test_084_the_second_run_replaces_the_first_so_the_first_result_is_ignored() {
    let (mut host, first) = started_host();
    host.deliver_one(row_command(TEST_COMMAND));
    let second = host.pending_requests().into_iter().find(|request| *request != first).expect("second request");
    finish_ok(&mut host, first, "ok", "old-model", 5);
    assert_eq!(host.settings().info("last_test"), Some("Not run yet".to_string()));
    finish_ok(&mut host, second, "ok", "new-model", 7);
    assert_eq!(host.settings().info("last_test").expect("row").ends_with(", new-model"), true);
}

/// REQ: F-084
#[test]
fn test_084_a_result_after_the_run_finished_changes_nothing() {
    let (mut host, request) = started_host();
    finish_ok(&mut host, request.clone(), "ok", "gpt-5.6-terra", 100);
    host.deliver_one(Event::RequestCompleted(RequestResult { request, outcome: Outcome::Failed(error(ErrorCode::Network)) }));
    assert_eq!(host.settings().info("last_test"), Some("100 ms, gpt-5.6-terra".to_string()));
}

/// REQ: F-084
#[test]
fn test_084_a_new_run_after_a_finished_one_updates_the_row_again() {
    let (mut host, request) = started_host();
    finish_ok(&mut host, request, "ok", "gpt-5.6-terra", 100);
    host.deliver_one(row_command(TEST_COMMAND));
    let next = host.pending_requests().pop().expect("pending");
    finish_ok(&mut host, next, "ok", "gpt-5.6-terra", 250);
    assert_eq!(host.settings().info("last_test"), Some("250 ms, gpt-5.6-terra".to_string()));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // REQ: F-084
    #[test]
    fn test_084_the_row_text_is_elapsed_milliseconds_then_the_model(elapsed in 0u64..1_000_000, model in "[a-z0-9.-]{1,20}") {
        let (mut host, request) = started_host();
        finish_ok(&mut host, request, "ok", &model, elapsed);
        prop_assert_eq!(host.settings().info("last_test"), Some(format!("{elapsed} ms, {model}")));
    }
}
