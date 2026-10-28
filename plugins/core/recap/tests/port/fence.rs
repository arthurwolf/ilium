//! Twins of ilium-client/src/recap/fence.rs: a record is dropped when the pane changed during the run.

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/recap/fence.rs::fence_rejects_reattached_session_and_vanished_pane
#[test]
fn port_fence_rejects_a_reattached_session_and_a_vanished_pane() {
    let (mut host, pane, invocation) = start_agent_run(&["user: fix the parser"]);
    host.tree()
        .set_agent_session(pane, "session-2", rows(&["user: fix the parser"]));
    let reattached = finish_run(&mut host, &invocation, answer(5, 5));
    assert_eq!(first_recap(&reattached)["state"], "stale");

    let (mut same, _, invocation) = start_agent_run(&["user: fix the parser"]);
    let current = finish_run(&mut same, &invocation, answer(5, 5));
    assert_eq!(first_recap(&current)["state"], "summarized");

    let (mut gone, pane, invocation) = start_shell_run(&["$ ls"]);
    gone.tree().remove(pane);
    let vanished = finish_run(&mut gone, &invocation, answer(5, 5));
    assert_eq!(first_recap(&vanished)["state"], "stale");
}

/// REQ: PORT:ilium-client/src/recap/fence.rs::fingerprint_changes_with_evidence
#[test]
fn port_fence_fingerprint_changes_with_evidence() {
    let (mut host, pane, invocation) = start_shell_run(&["a"]);
    finish_run(&mut host, &invocation, answer(5, 5));
    let again = host.cli().invoke(&["recap"], b"");
    host.settle();
    assert!(
        host.jobs().list().is_empty(),
        "identical evidence reuses the cached text"
    );
    assert_eq!(
        first_recap(&jsonl(&host.cli().refresh(&again).stdout()))["state"],
        "summarized"
    );
    host.tree().set_screen(pane, rows(&["b"]));
    host.cli().invoke(&["recap"], b"");
    assert_eq!(
        host.jobs().list().len(),
        1,
        "different evidence starts a new job"
    );
}

/// REQ: PORT:ilium-client/src/recap/fence.rs::fence_rejects_reattached_session_and_vanished_pane
#[test]
fn port_fence_reattached_session_is_counted_as_stale_and_keeps_no_short_text() {
    let (mut host, pane, invocation) = start_agent_run(&["user: fix the parser"]);
    host.tree()
        .set_agent_session(pane, "session-2", rows(&["user: fix the parser"]));
    let records = finish_run(&mut host, &invocation, answer(5, 5));
    assert_eq!(summary(&records)["stale"], 1);
    assert!(first_recap(&records)["short"].is_null());
}

/// REQ: PORT:ilium-client/src/recap/fence.rs::fingerprint_changes_with_evidence
#[test]
fn port_fence_new_screen_text_reaches_the_new_job_evidence() {
    let (mut host, pane, _) = start_shell_run(&["first marker row"]);
    host.tree().set_screen(pane, rows(&["second marker row"]));
    host.cli().invoke(&["recap"], b"");
    let mut jobs = host.jobs().list();
    let latest = jobs.pop().expect("a second job was started");
    let text = String::from_utf8_lossy(&latest.input).to_string();
    assert!(text.contains("second marker row"), "{text}");
    assert!(!text.contains("first marker row"), "{text}");
}
