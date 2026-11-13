//! Further twins of ilium-detect/src/lib.rs and ilium-detect/tests/status_chronology.rs through the `classify` export
//! (turn outcome only; evidence and matched-line parts are listed in API_GAPS.md), plus the running-indicator keys.

use ilium_plugin_sdk::types::AgentActivity;
use serde_json::json;

use crate::common::{activity, client_host, fixture, is_busy};

const OLD_WAIT: &str = "✻ Waiting for 1 background agent to finish";
const NEW_WAIT: &str = "✻ Waiting for 2 background tasks to finish";
const DONE: &str = "✻ Cooked for 1h 29m 54s · done 4:57 PM";
const COMPOSER: &str = "────────────────────────────────────────\n❯\n────────────────────────────────────────\n  ⏵⏵ auto mode on (shift+tab to cycle)";

/// REQ: PORT:ilium-detect/src/lib.rs::quoted_question_dialog_above_live_work_stays_working
#[test]
fn port_026_a_quoted_question_dialog_above_live_work_stays_working() {
    let quoted = format!(
        "{}\n✻ Herding… (3m 2s · esc to interrupt)\n",
        fixture("claude_ask_user_question_with_goal_history.txt")
    );
    assert_eq!(activity("claude", &quoted), AgentActivity::Working);
}

/// REQ: PORT:ilium-detect/src/lib.rs::new_background_wait_after_completed_turn_remains_waiting
#[test]
fn port_026_a_new_background_wait_after_a_completed_turn_remains_waiting() {
    let screen = "✻ Cooked for 12s · done 4:57 PM\n✻ Waiting for 1 background agent to finish";
    let found = activity("claude", screen);
    assert!(is_busy(&found), "{found:?}");
}

/// REQ: PORT:ilium-detect/src/lib.rs::latest_completed_turn_supersedes_historical_running_suffix
#[test]
fn port_026_the_latest_completed_turn_supersedes_a_historical_running_suffix() {
    let screen = "✻ Cooked for 12s · 1 shell still running\n● Shell finished\n✻ Cooked for 3s · done 5:00 PM";
    assert_eq!(activity("claude", screen), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::bottom_question_dialog_still_outranks_working_and_waiting
#[test]
fn port_026_a_bottom_question_dialog_outranks_working_and_waiting() {
    let screen = format!(
        "{OLD_WAIT}\n{DONE}\n✻ Herding… (3m 2s · esc to interrupt)\nChoose a path\n❯ 1. Continue\n  2. Chat about this\nEnter to select · Esc to cancel\n"
    );
    assert_eq!(activity("claude", &screen), AgentActivity::AwaitingApproval);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::visible_confirmation_remains_detectable_after_completion
#[test]
fn port_026_a_visible_confirmation_stays_detectable_after_completion() {
    let screen = format!("{OLD_WAIT}\n{DONE}\nContinue, yes or no?");
    assert_eq!(activity("claude", &screen), AgentActivity::AwaitingApproval);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::approval_predicates_still_receive_the_complete_screen
#[test]
fn port_026_approval_predicates_see_the_complete_screen() {
    let screen = format!("Continue, yes or no?\n{DONE}");
    assert_eq!(activity("claude", &screen), AgentActivity::AwaitingApproval);
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::current_still_running_suffix_survives_the_completion_boundary
#[test]
fn port_026_a_current_still_running_suffix_survives_the_completion_boundary() {
    for noun in ["shell", "monitor", "background task"] {
        let summary = format!("✻ Cooked for 3m 6s · done 7:00 PM · 1 {noun} still running");
        let screen = format!("{OLD_WAIT}\n{summary}\n\n{COMPOSER}");
        let found = activity("claude", &screen);
        assert!(is_busy(&found), "{noun}: {found:?}");
    }
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::still_running_suffix_without_a_done_clock_remains_settling
#[test]
fn port_026_a_still_running_suffix_without_a_done_clock_is_not_idle() {
    let screen = format!("{OLD_WAIT}\n✻ Cogitated for 3m 11s · 1 shell still running");
    let found = activity("claude", &screen);
    assert!(is_busy(&found), "{found:?}");
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::new_wait_after_a_settling_summary_keeps_mid_turn_wait_precedence
#[test]
fn port_026_a_new_wait_after_a_settling_summary_is_a_live_wait() {
    let screen = format!("✻ Cogitated for 3m 11s · 1 shell still running\n{NEW_WAIT}");
    let found = activity("claude", &screen);
    assert!(is_busy(&found), "{found:?}");
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::supported_summary_durations_do_not_depend_on_a_particular_verb
#[test]
fn port_026_summary_durations_do_not_depend_on_the_verb() {
    for summary in [
        "✻ Cogitated for 10s",
        "✻ Baked for 1m 59s · done 12:45 AM",
        "✻ Worked for 3m · done 12:45 AM",
        "✻ Cooked for 1h 29m 54s · done 4:57 PM",
        "✻ Cogitated for 1h",
    ] {
        let screen = format!("{OLD_WAIT}\n{summary}");
        assert_eq!(
            activity("claude", &screen),
            AgentActivity::Idle,
            "{summary}"
        );
    }
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::prose_mentions_and_incomplete_summaries_do_not_erase_current_waits
#[test]
fn port_026_prose_and_incomplete_summaries_do_not_erase_a_current_wait() {
    for non_boundary in [
        "The footer says ✻ Cooked for 10s",
        "● I cooked for 10s",
        "> ✻ Cooked for 10s",
        "`✻ Cooked for 10s`",
        "Cooked for 10s",
        "✻ I cooked for 10s",
        "✻ Cooked for",
        "✻ Cooked for 10seconds",
        "✻ Cooked for 10s extra prose",
        "✻ Cooked for 10s ·",
        "✻ Cooked for 10s · 1 shell still",
        "✻ Cooked for 10s · done",
        "✻ Cooked for 10s · done 4:57",
        "✻ Cooked for 10s · done 13:57 PM",
        "✻ Cooked for 10s · done 4:99 PM",
        "✻ Cooked for 10s · unfamiliar footer item",
        "Done",
        "✔ Goal achieved (3m · 2 turns)",
    ] {
        let screen = format!("{OLD_WAIT}\n{non_boundary}");
        let found = activity("claude", &screen);
        assert!(is_busy(&found), "{non_boundary}: {found:?}");
    }
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::running_indicator_keys_round_trip_and_step_wraps
#[test]
fn port_030_running_indicator_keys_are_accepted_and_unknown_keys_refused() {
    let mut host = client_host();
    for key in [
        "off",
        "icon",
        "spinner",
        "pulsing_dot",
        "steady_dot",
        "title_accent",
    ] {
        host.settings()
            .set("ui.attention_running_indicator", json!(key))
            .expect("known key");
        assert_eq!(
            host.settings().get("ui.attention_running_indicator"),
            Some(json!(key))
        );
    }
    assert!(host
        .settings()
        .set("ui.attention_running_indicator", json!("blink"))
        .is_err());
    assert_eq!(
        host.settings().get("ui.attention_running_indicator"),
        Some(json!("title_accent"))
    );
}
