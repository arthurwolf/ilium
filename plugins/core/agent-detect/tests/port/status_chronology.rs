//! Twins of ilium-detect/tests/status_chronology.rs through `classify`. Only the turn outcome is observable on the
//! plugin API; the evidence and matched-line parts are listed in API_GAPS.md.

use ilium_plugin_sdk::types::AgentActivity;

use crate::common::{activity, is_busy};

const COMPOSER: &str = "────────────────────────────────────────\n❯\n────────────────────────────────────────\n  ⏵⏵ auto mode on (shift+tab to cycle)";
const OLD_WAIT: &str = "✻ Waiting for 1 background agent to finish";
const NEW_WAIT: &str = "✻ Waiting for 2 background tasks to finish";
const DONE: &str = "✻ Cooked for 1h 29m 54s · done 4:57 PM";
const AGENT_FINISHED: &str = "● Agent \"Shoreline v2 waves and foam\" finished · 20m 0s";

fn screen(lines: &[&str]) -> String {
    format!("{}\n\n{COMPOSER}", lines.join("\n"))
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::a_later_final_summary_supersedes_old_waits_and_old_still_running_suffixes
#[test]
fn port_026_a_later_final_summary_supersedes_old_waits() {
    assert_eq!(activity("claude", &screen(&[OLD_WAIT, NEW_WAIT, DONE])), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::new_wait_below_an_old_completed_summary_is_current
#[test]
fn port_026_a_new_wait_below_an_old_summary_is_current() {
    assert!(is_busy(&activity("claude", &screen(&[DONE, NEW_WAIT]))));
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::current_wait_without_a_completed_summary_remains_waiting
#[test]
fn port_026_a_wait_without_a_summary_remains_waiting() {
    assert!(is_busy(&activity("claude", &screen(&[NEW_WAIT]))));
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::old_work_cannot_mask_a_new_wait_after_completion
#[test]
fn port_026_old_work_cannot_mask_a_new_wait() {
    assert!(is_busy(&activity("claude", &screen(&["✻ Working… (3m)", DONE, NEW_WAIT]))));
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::historical_working_markers_above_completion_do_not_keep_the_turn_working
#[test]
fn port_026_historical_working_markers_above_completion_are_ignored() {
    assert_eq!(activity("claude", &screen(&["✻ Working… (3m)", DONE])), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::reported_wait_then_agent_finished_then_final_summary_is_idle
#[test]
fn port_026_wait_then_agent_finished_then_summary_is_idle() {
    assert_eq!(activity("claude", &screen(&[NEW_WAIT, AGENT_FINISHED, DONE])), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::one_subagent_finishing_does_not_prove_all_background_work_finished
#[test]
fn port_026_one_finished_subagent_keeps_the_wait() {
    assert!(is_busy(&activity("claude", &screen(&[NEW_WAIT, AGENT_FINISHED]))));
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::current_interrupt_marker_below_completion_is_working
#[test]
fn port_026_a_live_status_below_the_summary_is_working() {
    assert_eq!(activity("claude", &screen(&[DONE, "✻ Working… (5s · esc to interrupt)"])), AgentActivity::Working);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::goal_evidence_is_not_used_as_an_activity_override
#[test]
fn port_026_goal_wording_does_not_override_activity() {
    assert_eq!(activity("claude", &screen(&["◎ /goal active (4m)"])), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::claude_completion_boundaries_are_not_imposed_on_other_known_providers
#[test]
fn port_026_claude_boundaries_do_not_apply_to_codex() {
    assert_eq!(activity("codex", "• Working… (5m)\n\n✻ Cooked for 1h 29m 54s · done 4:57 PM"), AgentActivity::Working);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::codex_completed_timing_prose_still_is_not_a_live_status
#[test]
fn port_026_codex_completed_timing_prose_is_idle() {
    assert_eq!(activity("codex", "Implemented the requested change… 12s\n\nSend a message"), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::utf8_and_crlf_preserve_byte_safe_current_evidence
#[test]
fn port_026_crlf_text_keeps_the_current_wait() {
    let text = screen(&[DONE, NEW_WAIT]).replace('\n', "\r\n");
    assert!(is_busy(&activity("claude", &text)));
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::folder_trust_after_completion_is_not_mistaken_for_idle
#[test]
fn port_026_folder_trust_after_completion_is_awaiting_approval() {
    let text = format!("{DONE}\n\n{}", crate::common::fixture("claude_folder_trust.txt"));
    assert_eq!(activity("claude", &text), AgentActivity::AwaitingApproval);
}
