//! F-027 activity classification through the `detector` export `classify`, on the captured screens of the
//! existing detector suite (ilium-detect/tests/fixtures), plus the page sample for `example-agent`.

use ilium_plugin_sdk::types::AgentActivity;
use proptest::prelude::*;

use crate::common::{activity_of, classify_text};

/// REQ: F-027
#[test]
fn feature_027_claude_mid_turn_is_working() {
    assert_eq!(
        activity_of("claude", "claude_code_working.txt"),
        AgentActivity::Working
    );
}

/// REQ: F-027
#[test]
fn feature_027_claude_idle_prompt_is_idle() {
    assert_eq!(
        activity_of("claude", "claude_code_idle.txt"),
        AgentActivity::Idle
    );
}

/// REQ: F-027
#[test]
fn feature_027_claude_awaiting_approval_is_awaiting_approval() {
    assert_eq!(
        activity_of("claude", "claude_code_awaiting_approval.txt"),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: F-027
#[test]
fn feature_027_claude_question_dialog_with_goal_history_is_awaiting_approval() {
    assert_eq!(
        activity_of("claude", "claude_ask_user_question_with_goal_history.txt"),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: F-027
#[test]
fn feature_027_a_question_dialog_outranks_a_stale_working_marker() {
    let dialog = crate::common::fixture("claude_ask_user_question_with_goal_history.txt");
    let stale = format!("esc to interrupt\n{dialog}");
    assert_eq!(
        classify_text("claude", &stale).activity,
        AgentActivity::AwaitingApproval
    );
}

/// REQ: F-027
#[test]
fn feature_027_a_quoted_question_dialog_above_live_work_stays_working() {
    let dialog = crate::common::fixture("claude_ask_user_question_with_goal_history.txt");
    let quoted = format!("{dialog}\n\u{273b} Herding\u{2026} (3m 2s \u{b7} esc to interrupt)\n");
    assert_eq!(
        classify_text("claude", &quoted).activity,
        AgentActivity::Working
    );
}

/// REQ: F-027
#[test]
fn feature_027_claude_rename_confirmation_menu_is_awaiting_approval() {
    assert_eq!(
        activity_of("claude", "claude_code_rename_confirm_prompt.txt"),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: F-027
#[test]
fn feature_027_claude_folder_trust_modal_is_awaiting_approval() {
    assert_eq!(
        activity_of("claude", "claude_folder_trust.txt"),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: F-027
#[test]
fn feature_027_prose_mentioning_yes_and_no_is_idle() {
    assert_eq!(
        activity_of("claude", "claude_code_prose_with_yes_no.txt"),
        AgentActivity::Idle
    );
}

/// REQ: F-027
#[test]
fn feature_027_numbered_analysis_with_a_stray_cursor_is_idle() {
    assert_eq!(
        activity_of(
            "claude",
            "claude_code_numbered_analysis_with_stray_cursor.txt"
        ),
        AgentActivity::Idle
    );
}

/// REQ: F-027
#[test]
fn feature_027_a_completed_goal_does_not_wait_on_a_historical_background_agent() {
    assert_eq!(
        activity_of("claude", "claude_completed_goal_after_background_wait.txt"),
        AgentActivity::Idle
    );
}

/// REQ: F-027
#[test]
fn feature_027_waiting_on_background_agents_is_not_idle() {
    assert_ne!(
        activity_of("claude", "claude_code_waiting_background.txt"),
        AgentActivity::Idle
    );
}

/// REQ: F-027
#[test]
fn feature_027_a_shell_still_running_after_the_turn_is_not_reported_done_or_idle() {
    let activity = activity_of("claude", "claude_code_shell_still_running.txt");
    assert_ne!(activity, AgentActivity::Idle);
    assert_ne!(activity, AgentActivity::Done);
}

/// REQ: F-027
#[test]
fn feature_027_a_monitor_still_running_after_the_turn_is_not_reported_done_or_idle() {
    let activity = activity_of("claude", "claude_code_monitor_still_running.txt");
    assert_ne!(activity, AgentActivity::Idle);
    assert_ne!(activity, AgentActivity::Done);
}

/// REQ: F-027
#[test]
fn feature_027_codex_mid_turn_is_working() {
    assert_eq!(
        activity_of("codex", "codex_working.txt"),
        AgentActivity::Working
    );
}

/// REQ: F-027
#[test]
fn feature_027_codex_idle_prompt_is_idle() {
    assert_eq!(activity_of("codex", "codex_idle.txt"), AgentActivity::Idle);
}

/// REQ: F-027
#[test]
fn feature_027_codex_placeholder_composer_is_idle() {
    assert_eq!(
        activity_of("codex", "codex_dynamic_placeholder_idle.txt"),
        AgentActivity::Idle
    );
}

/// REQ: F-027
#[test]
fn feature_027_codex_awaiting_approval_is_awaiting_approval() {
    assert_eq!(
        activity_of("codex", "codex_awaiting_approval.txt"),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: F-027
#[test]
fn feature_027_codex_folder_trust_modal_is_awaiting_approval() {
    assert_eq!(
        activity_of("codex", "codex_trust_folder.txt"),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: F-027
#[test]
fn feature_027_codex_status_segment_with_the_interrupt_marker_is_working() {
    let screen = "older transcript\nWorking (esc to interrupt)\nSend a message";
    assert_eq!(
        classify_text("codex", screen).activity,
        AgentActivity::Working
    );
}

/// REQ: F-027
#[test]
fn feature_027_codex_activity_word_inside_a_longer_word_is_not_working() {
    let screen = "Regenerating\u{2026} (5m) is not a status";
    assert_eq!(classify_text("codex", screen).activity, AgentActivity::Idle);
}

/// REQ: F-027
#[test]
fn feature_027_codex_status_behind_a_leading_glyph_is_working() {
    assert_eq!(
        classify_text("codex", "\u{2022} Working\u{2026} (5m)").activity,
        AgentActivity::Working
    );
}

/// REQ: F-027
#[test]
fn feature_027_codex_status_after_a_prose_occurrence_of_the_word_is_working() {
    assert_eq!(
        classify_text(
            "codex",
            "tests are running fine \u{b7} Running\u{2026} (5m)"
        )
        .activity,
        AgentActivity::Working
    );
}

/// REQ: F-027
#[test]
fn feature_027_a_plain_shell_prompt_has_no_activity_signal() {
    assert_eq!(
        activity_of("claude", "plain_shell.txt"),
        AgentActivity::Idle
    );
}

/// REQ: F-027
#[test]
fn feature_027_an_empty_screen_is_idle() {
    assert_eq!(classify_text("claude", "").activity, AgentActivity::Idle);
}

/// REQ: F-027
#[test]
fn feature_027_the_page_sample_marks_allow_this_command_as_awaiting_approval() {
    assert_eq!(
        classify_text("example-agent", "Allow this command?").activity,
        AgentActivity::AwaitingApproval
    );
}

/// REQ: F-027
#[test]
fn feature_027_the_page_sample_marks_the_interrupt_hint_as_working() {
    assert_eq!(
        classify_text("example-agent", "thinking... esc to interrupt").activity,
        AgentActivity::Working
    );
}

/// REQ: F-027
#[test]
fn feature_027_the_page_sample_treats_any_other_screen_as_idle() {
    assert_eq!(
        classify_text("example-agent", "$ ready").activity,
        AgentActivity::Idle
    );
}

/// REQ: F-027
#[test]
fn feature_027_approval_outranks_the_working_hint_in_the_page_sample() {
    let screen = "esc to interrupt\nAllow this command?";
    assert_eq!(
        classify_text("example-agent", screen).activity,
        AgentActivity::AwaitingApproval
    );
}

/// REQ: F-027
#[test]
fn feature_027_a_screen_without_goal_evidence_carries_no_goal() {
    assert_eq!(classify_text("claude", "plain text only").goal, None);
}

proptest! {
    // REQ: F-027
    #[test]
    fn feature_027_screens_of_digits_and_spaces_are_idle(rows in proptest::collection::vec("[0-9 ]{0,30}", 0..12)) {
        let screen = rows.join("\n");
        prop_assert_eq!(classify_text("claude", &screen).activity, AgentActivity::Idle);
        prop_assert_eq!(classify_text("codex", &screen).activity, AgentActivity::Idle);
    }

    // REQ: F-027
    #[test]
    fn feature_027_classification_is_deterministic(rows in proptest::collection::vec("[a-z0-9 ?>]{0,30}", 0..12)) {
        let screen = rows.join("\n");
        let first = classify_text("claude", &screen);
        let second = classify_text("claude", &screen);
        prop_assert_eq!(first, second);
    }
}
