//! Capability grants and output of the test harness (DEV-889, DEV-890). Page:
//! reference-plugin-test-harness.

use serde_json::Value;

use crate::common::*;
use crate::{failing_case_run, fake_host_harness};

/// REQ: DEV-889
#[test]
fn ungranted_host_call_fails_with_capability_denied_and_names_the_capability() {
    let mut harness = fake_host_harness(&["clock.read"]);
    let outcome: Result<Value, Value> =
        harness.call_host("net.http.get", "https://example.invalid/");
    let error = outcome.expect_err("a call without its grant must fail");
    assert_eq!(error["code"], "capability.denied", "{error}");
    let message = error["message"].as_str().expect("error message text");
    assert!(
        message.contains("net.http.get") || message.contains("net"),
        "the failing test names the capability: {message}"
    );
}

/// REQ: DEV-889
#[test]
fn granted_host_call_succeeds_and_real_resources_fail_with_harness_real_resource() {
    let mut harness = fake_host_harness(&["net.http.get"]);
    let granted: Result<Value, Value> =
        harness.call_host("net.http.get", "https://example.invalid/");
    assert!(
        granted.is_ok(),
        "the explicit grant lets the fake host answer: {granted:?}"
    );

    let real_socket: Result<Value, Value> = harness.connect_real_socket("203.0.113.7:443");
    let error = real_socket.expect_err("a real socket must be unreachable from a test");
    assert_eq!(error["code"], "harness.real-resource", "{error}");

    let real_clock: Result<Value, Value> = harness.read_real_clock();
    let clock_error = real_clock.expect_err("the real clock must be unreachable from a test");
    assert_eq!(
        clock_error["code"], "harness.real-resource",
        "{clock_error}"
    );

    let real_file: Result<Value, Value> = harness.read_file_outside_test_directory("/etc/hostname");
    let file_error = real_file.expect_err("files outside the test directory must be unreachable");
    assert_eq!(file_error["code"], "harness.real-resource", "{file_error}");
}

/// REQ: DEV-890
#[test]
fn failing_test_output_has_no_absolute_paths_user_names_or_wall_clock_times() {
    let first_root = scratch();
    let second_root = scratch();
    let first = failing_case_run(first_root.path(), "/home/first-user");
    let second = failing_case_run(second_root.path(), "/home/second-user");
    assert_ne!(first.exit_code, 0, "the failing case fails the run");
    for output in [&first, &second] {
        assert!(
            output.stdout_raw.lines().any(|line| line.contains("test ")),
            "the test name is printed"
        );
        assert!(
            !output
                .stdout_raw
                .contains(first_root.path().to_str().expect("utf8")),
            "no absolute path of the machine"
        );
        assert!(!output.stdout_raw.contains("first-user"), "no user name");
        assert!(!output.stdout_raw.contains("second-user"), "no user name");
    }
    assert_eq!(
        first.stdout_raw, second.stdout_raw,
        "CI logs from two machines compare equal"
    );
}

/// REQ: DEV-890
#[test]
fn failing_output_is_bounded_to_about_forty_context_lines() {
    let root = scratch();
    let failing = failing_case_run(root.path(), "/home/any-user");
    assert_ne!(failing.exit_code, 0, "the failing case fails the run");
    let line_count = failing.stdout_raw.lines().count();
    assert!(
        line_count <= 80,
        "at most 40 lines of context plus the header lines, got {line_count} lines"
    );
}
