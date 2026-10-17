//! Port twins of ilium-server/src/agent_delivery.rs: the lifecycle text typed into the composer
//! (manual core-plugin-progress.md, src/docs/automation.md "progress monitor result messages").

use crate::common::*;

const PROBE: &str = "/opt/probe.sh";
const JOB: &str = "render-17";
const DONE_NEXT: &str = "Next: verify this result, then continue with the next step of your current request.";
const UNKNOWN_NEXT: &str = "Next: do not assume success or failure; check the task directly, then continue your current request.";

fn settle_with(host: &mut ilium_plugin_sdk::testing::FakeHost, output: &str) {
    expect_probe(host, PROBE, output);
    tick(host, 1);
}

fn monitor_failure_text() -> String {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    for _ in 0..3 {
        expect_exit(&mut host, PROBE, failing_exit(1));
        tick(&mut host, 1);
    }
    let texts = typed_texts(&host, pane);
    assert_eq!(texts.len(), 1, "{texts:?}");
    texts[0].clone()
}

/// REQ: PORT:ilium-server/src/agent_delivery.rs::every_result_message_ends_with_its_next_action
#[test]
fn port_901_every_result_message_ends_with_its_next_action() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    settle_with(&mut host, &done(JOB, "Render finished"));
    let done_text = typed_texts(&host, pane).remove(0);
    assert!(done_text.ends_with(DONE_NEXT), "{done_text}");
    assert!(done_text.lines().next().unwrap().ends_with("completed successfully."), "{done_text}");

    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    settle_with(&mut host, &failed(JOB, 37, "Stopped", "renderer crashed"));
    let error_text = typed_texts(&host, pane).remove(0);
    assert!(error_text.ends_with("retry, and continue your current request."), "{error_text}");
    assert!(error_text.contains("retained evidence once"), "{error_text}");

    let unknown_text = monitor_failure_text();
    assert!(unknown_text.ends_with(UNKNOWN_NEXT), "{unknown_text}");

    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    progress(&mut host, pane, &["clear", "--all"]);
    host.settle();
    let stopped = typed_texts(&host, pane);
    assert_eq!(stopped.len(), 1, "{stopped:?}");
    assert!(stopped[0].ends_with(UNKNOWN_NEXT), "{}", stopped[0]);
}

/// REQ: PORT:ilium-server/src/agent_delivery.rs::terminal_messages_distinguish_task_error_from_unknown_monitor_outcome
#[test]
fn port_902_task_error_is_distinguished_from_unknown_monitor_outcome() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    settle_with(&mut host, &failed(JOB, 37, "Stopped", "renderer crashed"));
    let task_error = typed_texts(&host, pane).remove(0);
    assert!(task_error.contains("reports that render-17 failed"), "{task_error}");
    assert!(task_error.contains("renderer crashed"), "{task_error}");
    assert!(!task_error.contains("task outcome is unknown"), "{task_error}");

    let monitor_error = monitor_failure_text();
    assert!(monitor_error.contains("The task outcome is unknown."), "{monitor_error}");
    assert!(monitor_error.contains("Monitor error: "), "{monitor_error}");
    assert!(!monitor_error.contains("reports that render-17 failed"), "{monitor_error}");
}

/// REQ: PORT:ilium-server/src/agent_delivery.rs::task_controlled_delivery_text_drops_terminal_controls_and_is_bounded
#[test]
fn port_903_task_controlled_delivery_text_drops_terminal_controls_and_is_bounded() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    // A probe whose output is invalid (escape byte, far over the field limits) makes the monitor
    // fail; whatever of it reaches the composer must be control-free and at most 8 KiB.
    let hostile = format!("ok\u{1b}[31m{}", "x".repeat(16 * 1024));
    for _ in 0..3 {
        expect_probe(&mut host, PROBE, &hostile);
        tick(&mut host, 1);
    }
    let texts = typed_texts(&host, pane);
    assert_eq!(texts.len(), 1, "{texts:?}");
    assert!(!texts[0].contains('\u{1b}'), "{}", texts[0]);
    assert!(texts[0].len() <= 8 * 1024, "{}", texts[0].len());
    assert!(texts[0].contains("The task outcome is unknown."), "{}", texts[0]);
}
