//! Typing the outcome into the composer and its delivery states (progress-B13, progress-B14).

use ilium_plugin_sdk::types::Outcome;

use crate::common::*;

const PROBE: &str = "/opt/probe.sh";
const JOB: &str = "export-412";

fn settle_with(host: &mut ilium_plugin_sdk::testing::FakeHost, output: &str) {
    expect_probe(host, PROBE, output);
    tick(host, 1);
}

/// REQ: progress-B14
#[test]
fn progress_b14_a_done_outcome_is_typed_once_into_the_composer_of_the_pane() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    settle_with(&mut host, &done(JOB, "Export finished"));
    assert_eq!(
        typed_texts(&host, pane),
        [format!("Ilium progress monitor {id} reports that {JOB} completed successfully. Final progress: 100%. Status: Export finished.")]
    );
}

/// REQ: progress-B14
#[test]
fn progress_b14_an_error_outcome_names_the_failure_and_the_error() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    settle_with(&mut host, &failed(JOB, 37, "Stopped", "disk full"));
    assert_eq!(
        typed_texts(&host, pane),
        [format!("Ilium progress monitor {id} reports that {JOB} failed. Final progress: 37%. Status: Stopped. Error: disk full.")]
    );
}

/// REQ: progress-B14
#[test]
fn progress_b14_a_failed_monitor_says_the_outcome_is_unknown() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    for _ in 0..3 {
        expect_exit(&mut host, PROBE, failing_exit(1));
        tick(&mut host, 1);
    }
    let texts = typed_texts(&host, pane);
    assert_eq!(texts.len(), 1);
    assert!(texts[0].starts_with(&format!("Ilium progress monitor {id} could no longer observe {JOB}. The task outcome is unknown. Last progress: 10%. Monitor error: ")), "{}", texts[0]);
}

/// REQ: progress-B14
#[test]
fn progress_b14_a_cleared_monitor_whose_task_never_settled_reports_it_stopped() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    progress(&mut host, pane, &["clear", "--all"]);
    host.settle();
    for text in typed_texts(&host, pane) {
        assert!(text.starts_with(&format!("Ilium progress monitor {id} stopped before {JOB} reached a terminal task status. The task outcome is unknown.")), "{text}");
    }
}

/// REQ: progress-B14
#[test]
fn progress_b14_a_running_wait_that_collected_the_outcome_means_nothing_is_typed() {
    let (mut host, pane) = host_with_pane();
    expect_probe(&mut host, PROBE, &running(JOB, 10, "Working"));
    let open = start_in_pane(&mut host, pane, &["progress", "set", "--command", PROBE, "--wait"]);
    settle_with(&mut host, &done(JOB, "Finished"));
    assert!(open.read().stdout.contains("progress_wait"));
    assert!(typed_texts(&host, pane).is_empty());
}

/// REQ: progress-B14
#[test]
fn progress_b14_a_cancelled_wait_falls_back_to_typing_the_message() {
    let (mut host, pane) = host_with_pane();
    expect_probe(&mut host, PROBE, &running(JOB, 10, "Working"));
    let open = start_in_pane(&mut host, pane, &["progress", "set", "--command", PROBE, "--wait"]);
    open.cancel(&mut host);
    settle_with(&mut host, &done(JOB, "Finished"));
    assert_eq!(typed_texts(&host, pane).len(), 1);
}

/// REQ: progress-B14
#[test]
fn progress_b14_the_message_goes_to_the_registering_pane_only() {
    let (mut host, pane) = host_with_pane();
    let other = add_shell(&mut host, "other");
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    settle_with(&mut host, &done(JOB, "Finished"));
    assert!(typed_texts(&host, other).is_empty());
    assert_eq!(typed_texts(&host, pane).len(), 1);
}

/// REQ: progress-B13, progress-B14
#[test]
fn progress_b13_a_delivered_outcome_is_never_sent_again() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    settle_with(&mut host, &done(JOB, "Finished"));
    for request in host.pending_requests() {
        host.complete(request, Outcome::Ok);
    }
    host.settle();
    host.process().allow_unexpected();
    tick(&mut host, 120);
    assert_eq!(typed_texts(&host, pane).len(), 1);
}

/// REQ: progress-B13
#[test]
fn progress_b13_an_undelivered_outcome_is_sent_again_every_twenty_seconds() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    settle_with(&mut host, &done(JOB, "Finished"));
    let first = typed_texts(&host, pane).len();
    assert_eq!(first, 1);
    host.process().allow_unexpected();
    host.advance_clock(19_999);
    host.settle();
    assert_eq!(typed_texts(&host, pane).len(), first, "not yet at 19.999 s");
    host.advance_clock(1);
    host.settle();
    assert_eq!(typed_texts(&host, pane).len(), first + 1, "re-sent at 20 s");
}

/// REQ: progress-B13
#[test]
fn progress_b13_the_wait_record_for_a_delivered_monitor_still_names_its_outcome() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    settle_with(&mut host, &done(JOB, "Finished"));
    let run = progress(&mut host, pane, &["wait", &id.to_string()]);
    assert_eq!(record_of(&run.stdout, "progress_wait")["outcome"], "done");
}

/// REQ: progress-B13
#[test]
fn doc_progress_b13_the_seven_delivery_states_are_named_on_the_page() {
    let row = table_row(&page_text(), "progress-B13").join(" ");
    assert!(
        row.contains("`not-queued`, `queued`, `attempted`, `delivered-to-pty`, `uncertain`, `not-deliverable`, `collected-by-waiter`. Only `queued` is retried after a restart. Undelivered outcomes are re-sent every 20 s"),
        "{row}"
    );
}
