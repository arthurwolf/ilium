//! Twins of ilium/src/recap.rs: how the host resolves a pane into recap input.

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium/src/recap.rs::agent_display_names_map_to_transcript_classes
#[test]
fn port_server_agent_display_names_map_to_transcript_classes() {
    for signature in ["claude", "codex", "antigravity", "custom"] {
        let mut host = activated_server();
        open_gate(&mut host);
        let pane = host.tree().add(agent_pane("worker", signature));
        host.tree()
            .set_agent_session(pane, "session-1", rows(&["user: hello"]));
        host.cli().invoke(&["recap"], b"");
        assert_eq!(
            host.jobs().list().len(),
            1,
            "{signature} resolves a transcript"
        );
        assert!(host.transcript_reads() >= 1, "{signature}");
    }
}

/// REQ: PORT:ilium/src/recap.rs::agent_without_recorded_session_has_no_transcript
#[test]
fn port_server_agent_without_recorded_session_has_no_transcript() {
    let mut host = activated_server();
    open_gate(&mut host);
    host.tree().add(agent_pane("worker", "claude"));
    let invocation = host.cli().invoke(&["recap"], b"");
    let record = first_recap(&jsonl(&host.cli().refresh(&invocation).stdout()));
    assert_eq!(record["state"], "unavailable");
    assert!(host.jobs().list().is_empty());
}

/// REQ: PORT:ilium/src/recap.rs::unavailable_agent_never_resolves_or_reads_a_historical_transcript
#[test]
fn port_server_unavailable_agent_never_resolves_or_reads_a_historical_transcript() {
    let mut host = activated_server();
    open_gate(&mut host);
    host.tree().add(agent_pane("worker", "claude"));
    host.cli().invoke(&["recap"], b"");
    assert_eq!(host.transcript_reads(), 0);
}

/// REQ: PORT:ilium/src/recap.rs::shell_pane_screen_comes_from_its_selected_current_frame
#[test]
fn port_server_shell_pane_screen_comes_from_its_selected_current_frame() {
    let (mut host, _, _) = start_shell_run(&["current frame row"]);
    assert!(job_evidence(&mut host).contains("current frame row"));
    assert!(host.screen_reads() >= 1);
}

/// REQ: PORT:ilium/src/recap.rs::shell_pane_without_current_frame_has_no_screen_text
#[test]
fn port_server_shell_pane_without_current_frame_has_no_screen_text() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("build shell"));
    host.tree().set_screen_unavailable(pane);
    let invocation = host.cli().invoke(&["recap"], b"");
    assert!(host.jobs().list().is_empty());
    assert_eq!(
        first_recap(&jsonl(&host.cli().refresh(&invocation).stdout()))["state"],
        "unavailable"
    );
}

/// REQ: PORT:ilium/src/recap.rs::agent_display_names_map_to_transcript_classes
#[test]
fn port_server_transcript_query_targets_the_agent_pane() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(agent_pane("worker", "codex"));
    host.tree()
        .set_agent_session(pane, "session-1", rows(&["user: hello"]));
    host.cli().invoke(&["recap"], b"");
    let queries = host.transcript_queries();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].pane, pane);
}

/// REQ: PORT:ilium/src/recap.rs::agent_without_recorded_session_has_no_transcript
#[test]
fn port_server_agent_without_session_never_reads_the_screen() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(agent_pane("worker", "claude"));
    host.tree().set_screen(pane, rows(&["$ visible"]));
    host.cli().invoke(&["recap"], b"");
    assert_eq!(host.screen_reads(), 0);
}

/// REQ: PORT:ilium/src/recap.rs::shell_pane_screen_comes_from_its_selected_current_frame
#[test]
fn port_server_shell_evidence_is_fixed_when_the_run_starts() {
    let (mut host, pane, _) = start_shell_run(&["frame at start"]);
    host.tree().set_screen(pane, rows(&["frame after start"]));
    let evidence = job_evidence(&mut host);
    assert!(evidence.contains("frame at start"), "{evidence}");
    assert!(!evidence.contains("frame after start"), "{evidence}");
}

/// REQ: PORT:ilium/src/recap.rs::shell_pane_without_current_frame_has_no_screen_text
#[test]
fn port_server_unavailable_frame_counts_as_unavailable_in_the_summary() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("build shell"));
    host.tree().set_screen_unavailable(pane);
    let invocation = host.cli().invoke(&["recap"], b"");
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    assert_eq!(summary(&records)["unavailable"], 1);
}

/// REQ: PORT:ilium/src/recap.rs::unavailable_agent_never_resolves_or_reads_a_historical_transcript
#[test]
fn port_server_unavailable_agent_issues_no_transcript_query() {
    let mut host = activated_server();
    open_gate(&mut host);
    host.tree().add(agent_pane("worker", "claude"));
    host.cli().invoke(&["recap"], b"");
    assert!(host.transcript_queries().is_empty());
}
