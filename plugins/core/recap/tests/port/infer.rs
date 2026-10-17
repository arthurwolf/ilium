//! Twins of ilium-client/src/recap/infer.rs: the bounded, cancellable model call.

use ilium_plugin_sdk::types::{ErrorCode, PluginError};

use crate::common::*;
use crate::support::*;

fn failure_of(records: &[serde_json::Value]) -> String {
    first_recap(records)["failure"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// REQ: PORT:ilium-client/src/recap/infer.rs::returns_provider_text_on_success
#[test]
fn port_infer_returns_provider_text_on_success() {
    let (mut host, _, invocation) = start_shell_run(&["$ cargo test"]);
    let records = finish_run(&mut host, &invocation, answer(4, 6));
    let record = first_recap(&records);
    assert_eq!(record["state"], "summarized");
    assert_eq!(record["short"]["text"], words(4));
    assert_eq!(record["long"]["text"], words(6));
}

/// REQ: PORT:ilium-client/src/recap/infer.rs::reports_provider_failure_without_panicking
#[test]
fn port_infer_reports_provider_failure_without_panicking() {
    let (mut host, _, invocation) = start_shell_run(&["$ cargo test"]);
    let request = host.jobs().list()[0].request;
    host.jobs().finish(
        request,
        Err(PluginError {
            code: ErrorCode::Network,
            message: "no key".to_string(),
            retry_after_ms: None,
        }),
    );
    host.settle();
    let output = host.cli().refresh(&invocation);
    let records = jsonl(&output.stdout());
    assert_eq!(first_recap(&records)["state"], "failed");
    assert!(
        failure_of(&records).contains("model provider failed"),
        "{records:?}"
    );
    assert!(failure_of(&records).contains("no key"));
    assert_eq!(output.exit_code(), Some(0));
}

/// REQ: PORT:ilium-client/src/recap/infer.rs::times_out_slow_calls
#[test]
fn port_infer_times_out_slow_calls() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("slow shell"));
    host.tree().set_screen(pane, rows(&["$ sleep 100"]));
    let invocation = host.cli().invoke(&["recap", "--timeout-seconds", "1"], b"");
    assert_eq!(host.jobs().list().len(), 1);
    host.advance_clock(1_001);
    host.settle();
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    assert_eq!(first_recap(&records)["state"], "failed");
    assert!(failure_of(&records).contains("timed out"), "{records:?}");
}

/// REQ: PORT:ilium-client/src/recap/infer.rs::cancellation_abandons_waiting_call
#[test]
fn port_infer_cancellation_abandons_a_waiting_call() {
    let (mut host, _, invocation) = start_shell_run(&["$ sleep 100"]);
    host.cli().cancel(&invocation);
    host.settle();
    let output = host.cli().refresh(&invocation);
    let records = jsonl(&output.stdout());
    assert_eq!(summary(&records)["cancelled"], true);
    assert!(failure_of(&records).contains("cancelled"), "{records:?}");
}

fn many_shell_panes(host: &mut ilium_plugin_sdk::testing::FakeHost, count: usize) {
    for index in 0..count {
        let pane = host.tree().add(shell_pane(&format!("shell {index}")));
        host.tree()
            .set_screen(pane, rows(&[&format!("$ job {index}")]));
    }
}

/// REQ: PORT:ilium-client/src/recap/infer.rs::concurrency_gate_bounds_parallel_calls
#[test]
fn port_infer_concurrency_gate_bounds_parallel_calls() {
    let mut host = activated_server();
    open_gate(&mut host);
    many_shell_panes(&mut host, 6);
    host.cli().invoke(&["recap", "--max-concurrency", "2"], b"");
    assert_eq!(host.running_job_limit(), 2);
    assert!(
        host.jobs().list().len() <= 6,
        "every pane queues at most one job"
    );
}

/// REQ: PORT:ilium-client/src/recap/infer.rs::outstanding_provider_calls_never_exceed_gate_after_timeouts_and_cancels
#[test]
fn port_infer_outstanding_calls_never_exceed_the_gate_after_timeouts_and_cancels() {
    let mut host = activated_server();
    open_gate(&mut host);
    many_shell_panes(&mut host, 4);
    let first = host.cli().invoke(
        &["recap", "--max-concurrency", "2", "--timeout-seconds", "1"],
        b"",
    );
    host.advance_clock(1_001);
    host.settle();
    assert!(host.running_job_limit() <= 2);
    let second = host.cli().invoke(&["recap", "--max-concurrency", "2"], b"");
    host.cli().cancel(&second);
    host.settle();
    assert!(
        host.running_job_limit() <= 2,
        "the gate holds after timeouts and cancels"
    );
    let _ = host.cli().refresh(&first);
}
