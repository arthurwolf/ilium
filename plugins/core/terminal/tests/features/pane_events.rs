//! Pane processes and their signals: the page sample "Interrupt and watch exits".

use ilium_plugin_sdk::types::{Event, NodeId, PaneExitedEvent};

use crate::common::server_host;

fn exited(node: u64, status: Option<i32>, signal: Option<&str>) -> Event {
    Event::PaneExited(PaneExitedEvent {
        node: NodeId(node),
        status,
        signal: signal.map(str::to_string),
    })
}

/// REQ: F-014
#[test]
fn feature_014_a_pane_exit_with_a_status_is_logged_at_info() {
    let mut host = server_host();
    host.deliver(vec![exited(7, Some(0), None)]);
    let lines = host.logs();
    let line = lines
        .iter()
        .find(|line| line.message.contains("pane 7 exited"))
        .expect("exit line");
    assert_eq!(line.level, "info");
    assert_eq!(line.message, "pane 7 exited status=Some(0) signal=None");
}

/// REQ: F-014
#[test]
fn feature_014_a_pane_exit_by_signal_logs_the_signal_name() {
    let mut host = server_host();
    host.deliver(vec![exited(9, None, Some("SIGTERM"))]);
    let lines = host.logs();
    assert!(
        lines
            .iter()
            .any(|line| line.message == "pane 9 exited status=None signal=Some(\"SIGTERM\")"),
        "{lines:?}"
    );
}

/// REQ: F-014
#[test]
fn feature_014_a_nonzero_exit_status_is_logged_as_given() {
    let mut host = server_host();
    host.deliver(vec![exited(3, Some(130), None)]);
    assert!(host
        .logs()
        .iter()
        .any(|line| line.message == "pane 3 exited status=Some(130) signal=None"));
}

/// REQ: F-014
#[test]
fn feature_014_each_exit_in_a_batch_is_logged_once_in_order() {
    let mut host = server_host();
    host.deliver(vec![exited(1, Some(0), None), exited(2, Some(1), None)]);
    let messages: Vec<String> = host
        .logs()
        .into_iter()
        .map(|line| line.message)
        .filter(|m| m.contains("exited"))
        .collect();
    assert_eq!(
        messages,
        [
            "pane 1 exited status=Some(0) signal=None",
            "pane 2 exited status=Some(1) signal=None"
        ]
    );
}

/// REQ: F-014
#[test]
fn feature_014_a_pane_exit_alone_submits_no_signal() {
    let mut host = server_host();
    assert!(host.deliver(vec![exited(7, Some(0), None)]).is_empty());
}
