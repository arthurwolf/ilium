//! Twins of the inline tests in ilium-client/src/remote_compaction_flow.rs.
//! Gaps: `monitor_*` trigger-monitor probes and the `flow_*` dialog harness on FakeHost
//! (see ../API_GAPS_port.md).

use serde_json::json;

use crate::common::*;

const SESSION: &str = "11111111-1111-4111-8111-111111111111";
const FAILURE_LIMIT: u32 = 3;

fn started() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = server_host();
    host.flow_start("claude", SESSION);
    host
}

fn resume_request() -> serde_json::Value {
    json!({"kind": "ReplacePaneWithCommand", "command_line": format!("claude --resume {SESSION}")})
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::monitor_triggers_at_the_threshold_only
#[test]
fn flow_monitor_triggers_at_the_threshold_only() {
    let mut host = server_host();
    assert!(!host.monitor_should_trigger(9, 64.9, 65.0, 600, 0));
    assert!(host.monitor_should_trigger(9, 65.0, 65.0, 600, 0));
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::monitor_waits_out_the_cooldown_after_an_attempt
#[test]
fn flow_monitor_waits_out_the_cooldown_after_an_attempt() {
    let mut host = server_host();
    host.monitor_record_attempt(9, 0);
    assert!(!host.monitor_should_trigger(9, 80.0, 65.0, 600, 599));
    assert!(host.monitor_should_trigger(9, 80.0, 65.0, 600, 600));
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::monitor_stops_after_repeated_failures_until_the_context_drops
#[test]
fn flow_monitor_stops_after_repeated_failures_until_the_context_drops() {
    let mut host = server_host();
    for _ in 0..FAILURE_LIMIT {
        host.monitor_record_failure(9);
    }
    assert!(!host.monitor_should_trigger(9, 90.0, 65.0, 0, 0));
    assert!(
        !host.monitor_should_trigger(9, 61.0, 65.0, 0, 0),
        "inside the hysteresis margin"
    );
    assert!(!host.monitor_should_trigger(9, 90.0, 65.0, 0, 0));
    assert!(
        !host.monitor_should_trigger(9, 10.0, 65.0, 0, 0),
        "a real drop re-arms the pane"
    );
    assert!(host.monitor_should_trigger(9, 90.0, 65.0, 0, 0));
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::a_success_clears_the_failure_streak
#[test]
fn flow_a_success_clears_the_failure_streak() {
    let mut host = server_host();
    for _ in 0..FAILURE_LIMIT {
        host.monitor_record_failure(9);
    }
    host.monitor_record_success(9);
    assert!(host.monitor_should_trigger(9, 90.0, 65.0, 0, 0));
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::only_claude_and_codex_map_to_a_transcript_format
#[test]
fn flow_only_claude_and_codex_map_to_a_transcript_format() {
    let host = server_host();
    assert_eq!(
        host.transcript_format_for("claude").as_deref(),
        Some("claude")
    );
    assert_eq!(
        host.transcript_format_for("codex").as_deref(),
        Some("codex")
    );
    assert_eq!(host.transcript_format_for("antigravity"), None);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::disabled_feature_never_starts_and_never_replaces_compact
#[test]
fn flow_disabled_feature_never_starts_and_never_replaces_compact() {
    let mut host = server_host();
    let pane = host.tree().add_agent_pane("agent", "claude");
    assert!(
        !host.flow_try_start(pane, false),
        "disabled: the command does not start a dialog"
    );
    assert!(!host.flow_replaces_native_compact(pane));
    assert!(!host.flow_state()["open"].as_bool().unwrap());
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::reaching_the_pause_freezes_the_pane_and_stops_the_agent
#[test]
fn flow_reaching_the_pause_freezes_the_pane_and_stops_the_agent() {
    let mut host = started();
    host.flow_event(json!({"kind": "PauseReached", "result": "ok"}));
    let state = host.flow_state();
    assert_eq!(state["frozen"], true);
    assert_eq!(
        host.flow_requests(),
        vec![json!({"kind": "TerminatePaneProcess"})]
    );
    assert_eq!(state["phase"], "StoppingAgent");
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::a_missed_pause_fails_without_freezing
#[test]
fn flow_a_missed_pause_fails_without_freezing() {
    let mut host = started();
    host.flow_event(json!({"kind": "PauseReached", "result": "late"}));
    let state = host.flow_state();
    assert_eq!(state["frozen"], false);
    assert_eq!(state["failed"], true);
    assert!(host.flow_requests().is_empty());
    host.flow_key("Esc");
    assert_eq!(host.flow_state()["open"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::a_stopped_agent_requests_the_compaction_job
#[test]
fn flow_a_stopped_agent_requests_the_compaction_job() {
    let mut host = started();
    host.flow_event(json!({"kind": "PauseReached", "result": "ok"}));
    host.flow_event(json!({"kind": "ProcessTerminated", "result": "ok"}));
    let job = host.flow_pending_job().expect("compaction job requested");
    assert_eq!(job["request"]["session_id"], SESSION);
    assert_eq!(job["request"]["agent"], "claude");
    assert_eq!(host.flow_state()["agent_stopped"], true);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::a_failed_stop_does_not_start_the_worker
#[test]
fn flow_a_failed_stop_does_not_start_the_worker() {
    let mut host = started();
    host.flow_event(json!({"kind": "PauseReached", "result": "ok"}));
    host.flow_event(json!({"kind": "ProcessTerminated", "result": "busy"}));
    assert!(host.flow_pending_job().is_none());
    assert_eq!(host.flow_state()["failed"], true);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::success_resumes_the_same_session_and_closes_the_dialog
#[test]
fn flow_success_resumes_the_same_session_and_closes_the_dialog() {
    let mut host = started();
    host.flow_event(json!({"kind": "Finished", "result": "ok"}));
    assert_eq!(host.flow_requests(), vec![resume_request()]);
    assert_eq!(host.flow_state()["open"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::failure_after_the_stop_offers_to_resume_the_original_session
#[test]
fn flow_failure_after_the_stop_offers_to_resume_the_original_session() {
    let mut host = started();
    host.flow_set_agent_stopped(true);
    host.flow_event(json!({"kind": "Finished", "result": "model unavailable"}));
    assert_eq!(host.flow_state()["failed"], true);
    assert!(host.flow_requests().is_empty());
    host.flow_key("Enter");
    assert_eq!(host.flow_requests(), vec![resume_request()]);
    assert_eq!(host.flow_state()["open"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::escape_while_waiting_cancels_without_touching_the_agent
#[test]
fn flow_escape_while_waiting_cancels_without_touching_the_agent() {
    let mut host = started();
    host.flow_key("Esc");
    assert!(host.flow_take_cancel());
    assert_eq!(host.flow_state()["open"], false);
    assert!(host.flow_requests().is_empty());
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::escape_while_compacting_asks_the_worker_to_cancel_and_keeps_the_dialog
#[test]
fn flow_escape_while_compacting_asks_the_worker_to_cancel_and_keeps_the_dialog() {
    let mut host = started();
    host.flow_set_phase("Compacting");
    host.flow_key("Esc");
    assert!(host.flow_take_cancel());
    assert_eq!(host.flow_state()["open"], true);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::x_dismisses_the_privacy_banner_for_good
#[test]
fn flow_x_dismisses_the_privacy_banner_for_good() {
    let mut host = started();
    assert_eq!(host.flow_state()["banner_shown"], true);
    host.flow_key("x");
    assert_eq!(host.flow_state()["banner_shown"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::interrupt_request_sends_escape_to_the_pane
#[test]
fn flow_interrupt_request_sends_escape_to_the_pane() {
    let mut host = started();
    host.flow_event(json!({"kind": "InterruptRequested"}));
    assert_eq!(
        host.flow_requests(),
        vec![json!({"kind": "KeyInput", "bytes": [27]})]
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::a_failed_stop_does_not_start_the_worker
#[test]
fn flow_failed_stop_offers_the_resume_on_enter() {
    let mut host = started();
    host.flow_event(json!({"kind": "PauseReached", "result": "ok"}));
    host.flow_event(json!({"kind": "ProcessTerminated", "result": "busy"}));
    host.flow_key("Enter");
    assert_eq!(host.flow_requests().last(), Some(&resume_request()));
    assert_eq!(host.flow_state()["open"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::a_missed_pause_fails_without_freezing
#[test]
fn flow_missed_pause_leaves_the_agent_running() {
    let mut host = started();
    host.flow_event(json!({"kind": "PauseReached", "result": "late"}));
    assert_eq!(host.flow_state()["agent_stopped"], false);
    assert!(!host
        .flow_requests()
        .contains(&json!({"kind": "TerminatePaneProcess"})));
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::a_stopped_agent_requests_the_compaction_job
#[test]
fn flow_compaction_job_waits_for_the_stop() {
    let mut host = started();
    host.flow_event(json!({"kind": "PauseReached", "result": "ok"}));
    assert!(host.flow_pending_job().is_none());
    assert!(!host.flow_requests().contains(&resume_request()));
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::a_success_clears_the_failure_streak
#[test]
fn monitor_streak_restarts_after_a_success() {
    let mut host = server_host();
    host.monitor_record_failure(9);
    host.monitor_record_failure(9);
    host.monitor_record_success(9);
    host.monitor_record_failure(9);
    host.monitor_record_failure(9);
    assert!(
        host.monitor_should_trigger(9, 90.0, 65.0, 0, 0),
        "two failures after the success are below the limit"
    );
    host.monitor_record_failure(9);
    assert!(!host.monitor_should_trigger(9, 90.0, 65.0, 0, 0));
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::disabled_feature_never_starts_and_never_replaces_compact
#[test]
fn flow_enabled_feature_starts_the_dialog_on_the_pane() {
    let mut host = server_host();
    let pane = host.tree().add_agent_pane("agent", "claude");
    assert!(host.flow_try_start(pane, true));
    assert_eq!(host.flow_state()["open"], true);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::escape_while_compacting_asks_the_worker_to_cancel_and_keeps_the_dialog
#[test]
fn flow_escape_while_compacting_sends_no_stop_or_resume_request() {
    let mut host = started();
    host.flow_set_phase("Compacting");
    host.flow_key("Esc");
    assert!(host.flow_requests().is_empty());
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::escape_while_waiting_cancels_without_touching_the_agent
#[test]
fn flow_cancel_is_reported_once() {
    let mut host = started();
    host.flow_key("Esc");
    assert!(host.flow_take_cancel());
    assert!(!host.flow_take_cancel());
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::failure_after_the_stop_offers_to_resume_the_original_session
#[test]
fn flow_failed_finish_keeps_the_dialog_open() {
    let mut host = started();
    host.flow_set_agent_stopped(true);
    host.flow_event(json!({"kind": "Finished", "result": "model unavailable"}));
    assert_eq!(host.flow_state()["open"], true);
    assert_eq!(host.flow_state()["failed"], true);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::interrupt_request_sends_escape_to_the_pane
#[test]
fn flow_interrupt_request_does_not_terminate_the_agent() {
    let mut host = started();
    host.flow_event(json!({"kind": "InterruptRequested"}));
    assert!(!host
        .flow_requests()
        .contains(&json!({"kind": "TerminatePaneProcess"})));
    assert_eq!(host.flow_state()["frozen"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::monitor_stops_after_repeated_failures_until_the_context_drops
#[test]
fn monitor_failure_streak_is_per_pane() {
    let mut host = server_host();
    for _ in 0..FAILURE_LIMIT {
        host.monitor_record_failure(9);
    }
    assert!(host.monitor_should_trigger(10, 90.0, 65.0, 0, 0));
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::monitor_triggers_at_the_threshold_only
#[test]
fn monitor_never_triggers_at_zero_percent() {
    let mut host = server_host();
    assert!(!host.monitor_should_trigger(9, 0.0, 65.0, 0, 0));
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::monitor_waits_out_the_cooldown_after_an_attempt
#[test]
fn monitor_cooldown_is_per_pane() {
    let mut host = server_host();
    host.monitor_record_attempt(9, 0);
    assert!(host.monitor_should_trigger(10, 80.0, 65.0, 600, 1));
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::only_claude_and_codex_map_to_a_transcript_format
#[test]
fn flow_transcript_format_rejects_other_agents() {
    let host = server_host();
    assert_eq!(host.transcript_format_for("gemini"), None);
    assert_eq!(host.transcript_format_for("opencode"), None);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::reaching_the_pause_freezes_the_pane_and_stops_the_agent
#[test]
fn flow_pause_reached_does_not_start_the_compaction_job() {
    let mut host = started();
    host.flow_event(json!({"kind": "PauseReached", "result": "ok"}));
    assert!(host.flow_pending_job().is_none());
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::success_resumes_the_same_session_and_closes_the_dialog
#[test]
fn flow_success_clears_the_failure_flag() {
    let mut host = started();
    host.flow_event(json!({"kind": "Finished", "result": "ok"}));
    assert_eq!(host.flow_state()["failed"], false);
}

/// REQ: PORT:ilium-client/src/remote_compaction_flow.rs::x_dismisses_the_privacy_banner_for_good
#[test]
fn flow_delete_key_dismisses_the_privacy_banner() {
    let mut host = started();
    host.flow_key("Delete");
    assert_eq!(host.flow_state()["banner_shown"], false);
}
