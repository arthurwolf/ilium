//! Twins of the activity classification tests of ilium-detect/src/lib.rs, through the `classify` export.

use ilium_plugin_sdk::types::AgentActivity;

use crate::common::{activity, fixture, goal, is_busy};

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_mid_turn_is_working
#[test]
fn port_026_claude_mid_turn_screen_is_working() {
    assert_eq!(
        activity("claude", &fixture("claude_code_working.txt")),
        AgentActivity::Working
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_idle_prompt_is_idle
#[test]
fn port_026_claude_idle_prompt_is_idle() {
    assert_eq!(
        activity("claude", &fixture("claude_code_idle.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_awaiting_approval_is_waiting_approval
#[test]
fn port_026_claude_approval_dialog_is_awaiting_approval() {
    assert_eq!(
        activity("claude", &fixture("claude_code_awaiting_approval.txt")),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_ask_user_question_with_goal_history_is_waiting_approval
#[test]
fn port_026_claude_question_with_goal_history_is_awaiting_approval() {
    assert_eq!(
        activity(
            "claude",
            &fixture("claude_ask_user_question_with_goal_history.txt")
        ),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_mid_turn_is_working
#[test]
fn port_026_codex_mid_turn_screen_is_working() {
    assert_eq!(
        activity("codex", &fixture("codex_working.txt")),
        AgentActivity::Working
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_idle_prompt_is_idle
#[test]
fn port_026_codex_idle_prompt_is_idle() {
    assert_eq!(
        activity("codex", &fixture("codex_idle.txt")),
        AgentActivity::Idle
    );
    assert_eq!(
        activity("codex", &fixture("codex_dynamic_placeholder_idle.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_awaiting_approval_is_waiting_approval
#[test]
fn port_026_codex_approval_dialog_is_awaiting_approval() {
    assert_eq!(
        activity("codex", &fixture("codex_awaiting_approval.txt")),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::a_numbered_modal_using_codex_own_cursor_glyph_is_waiting_approval
#[test]
fn port_026_numbered_codex_modal_is_awaiting_approval() {
    let text = "Select a mode\n› 1. Read only\n  2. Full access";
    assert_eq!(activity("codex", text), AgentActivity::AwaitingApproval);
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_completed_timing_summary_is_idle
#[test]
fn port_026_codex_completed_timing_summary_is_idle() {
    let text = "Implemented the requested change… 12s\n\nSend a message";
    assert_eq!(activity("codex", text), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_activity_word_embedded_in_a_longer_word_is_not_working
#[test]
fn port_026_activity_word_inside_a_longer_word_is_not_working() {
    assert_eq!(
        activity("codex", "Regenerating… (5m) is not a status"),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_status_segment_after_a_prose_occurrence_of_the_same_word_is_working
#[test]
fn port_026_status_segment_after_prose_is_working() {
    assert_eq!(
        activity("codex", "tests are running fine · Running… (5m)"),
        AgentActivity::Working
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_working_status_behind_a_leading_glyph_is_working
#[test]
fn port_026_status_behind_a_leading_glyph_is_working() {
    assert_eq!(activity("codex", "• Working… (5m)"), AgentActivity::Working);
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_waiting_on_background_agents_is_waiting_background
#[test]
fn port_026_waiting_on_background_agents_is_not_at_rest() {
    assert!(is_busy(&activity(
        "claude",
        &fixture("claude_code_waiting_background.txt")
    )));
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_completed_turn_with_shell_still_running_is_background_task_still_running
#[test]
fn port_026_completed_turn_with_shell_still_running_is_not_at_rest() {
    assert!(is_busy(&activity(
        "claude",
        &fixture("claude_code_shell_still_running.txt")
    )));
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_completed_turn_with_monitor_still_running_is_background_task_still_running
#[test]
fn port_026_completed_turn_with_monitor_still_running_is_not_at_rest() {
    assert!(is_busy(&activity(
        "claude",
        &fixture("claude_code_monitor_still_running.txt")
    )));
}

/// REQ: PORT:ilium-detect/src/lib.rs::completed_goal_does_not_wait_on_historical_background_agent
#[test]
fn port_026_completed_goal_ignores_a_historical_background_agent() {
    assert_eq!(
        activity(
            "claude",
            &fixture("claude_completed_goal_after_background_wait.txt")
        ),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::background_wait_in_explanatory_prose_is_not_live_status
#[test]
fn port_026_background_wait_in_prose_is_idle() {
    let text = "I'll run this in the background and keep waiting for input.";
    assert_eq!(activity("claude", text), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/src/lib.rs::prose_mentioning_background_or_waiting_alone_is_idle_not_waiting_background
#[test]
fn port_026_prose_about_waiting_is_idle() {
    assert_eq!(
        activity("claude", "Waiting for the build to finish."),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::prose_mentioning_shell_or_running_alone_is_idle_not_waiting_background
#[test]
fn port_026_prose_about_a_shell_or_running_is_idle() {
    assert_eq!(
        activity("claude", "I opened a new shell for the migration."),
        AgentActivity::Idle
    );
    assert_eq!(
        activity("claude", "The dev server is running now."),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::prose_mentioning_yes_and_no_is_idle_not_waiting_approval
#[test]
fn port_026_prose_with_yes_and_no_is_idle() {
    assert_eq!(
        activity("claude", &fixture("claude_code_prose_with_yes_no.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::rhetorical_question_with_not_is_not_waiting_approval
#[test]
fn port_026_rhetorical_question_is_idle() {
    assert_eq!(
        activity("claude", "Does that make sense, or not?"),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::question_with_yes_no_as_substrings_of_other_words_is_not_waiting_approval
#[test]
fn port_026_question_with_substring_yes_no_is_idle() {
    assert_eq!(
        activity(
            "claude",
            "Did yesterday's changes land, or is there nothing new?"
        ),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::numbered_analysis_with_stray_cursor_elsewhere_is_idle
#[test]
fn port_026_numbered_analysis_with_stray_cursor_is_idle() {
    assert_eq!(
        activity(
            "claude",
            &fixture("claude_code_numbered_analysis_with_stray_cursor.txt")
        ),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::plain_shell_prompt_has_no_activity_signal
#[test]
fn port_026_plain_shell_is_idle() {
    assert_eq!(
        activity("claude", &fixture("plain_shell.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::rename_plan_confirmation_menu_is_waiting_approval
#[test]
fn port_026_rename_confirmation_menu_is_awaiting_approval() {
    assert_eq!(
        activity("claude", &fixture("claude_code_rename_confirm_prompt.txt")),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::question_dialog_outranks_stale_working_marker
#[test]
fn port_026_dialog_outranks_a_stale_working_marker() {
    assert_eq!(
        activity(
            "claude",
            &fixture("claude_ask_user_question_with_goal_history.txt")
        ),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_folder_trust_modal_never_receives_initial_input
#[test]
fn port_026_claude_folder_trust_modal_is_awaiting_approval() {
    assert_eq!(
        activity("claude", &fixture("claude_folder_trust.txt")),
        AgentActivity::AwaitingApproval
    );
    assert_eq!(
        activity("claude", "Yes, I trust this folder"),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_folder_trust_modal_never_receives_initial_input
#[test]
fn port_026_codex_folder_trust_modal_is_awaiting_approval() {
    assert_eq!(
        activity("codex", &fixture("codex_trust_folder.txt")),
        AgentActivity::AwaitingApproval
    );
}

const ALT_OLD_WAIT: &str = "✻ Waiting for 1 background agent to finish";
const ALT_DONE: &str = "✻ Cooked for 1h 29m 54s · done 4:57 PM";
const ALT_COMPOSER: &str = "────────────────────────────────────────\n❯\n────────────────────────────────────────\n  ⏵⏵ auto mode on (shift+tab to cycle)";

/// REQ: PORT:ilium-detect/src/lib.rs::a_numbered_modal_using_codex_own_cursor_glyph_is_waiting_approval
#[test]
fn port_026_alt_numbered_codex_modal_reports_no_goal() {
    assert_eq!(
        goal("codex", "Select a mode\n› 1. Read only\n  2. Full access"),
        None
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::background_wait_in_explanatory_prose_is_not_live_status
#[test]
fn port_026_alt_detector_explanation_about_waiting_is_idle() {
    let text = "The detector says waiting for background agents when it sees this sentence.\n❯";
    assert_eq!(activity("claude", text), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_ask_user_question_with_goal_history_is_waiting_approval
#[test]
fn port_026_alt_selection_prompt_after_completed_summary_awaits_approval() {
    let screen = format!(
        "{ALT_OLD_WAIT}\n{ALT_DONE}\n✻ Herding… (3m 2s · esc to interrupt)\nChoose a path\n❯ 1. Continue\n  2. Chat about this\nEnter to select · Esc to cancel\n"
    );
    assert_eq!(activity("claude", &screen), AgentActivity::AwaitingApproval);
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_awaiting_approval_is_waiting_approval
#[test]
fn port_026_alt_confirmation_question_after_summary_awaits_approval() {
    let text = format!("Continue, yes or no?\n{ALT_DONE}");
    assert_eq!(activity("claude", &text), AgentActivity::AwaitingApproval);
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_completed_turn_with_monitor_still_running_is_background_task_still_running
#[test]
fn port_026_alt_monitor_still_running_after_summary_is_not_at_rest() {
    let screen = format!(
        "{ALT_OLD_WAIT}\n✻ Cooked for 3m 6s · done 7:00 PM · 1 monitor still running\n\n{ALT_COMPOSER}"
    );
    assert!(is_busy(&activity("claude", &screen)));
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_completed_turn_with_shell_still_running_is_background_task_still_running
#[test]
fn port_026_alt_shell_still_running_without_done_clock_is_not_at_rest() {
    let screen = format!("{ALT_OLD_WAIT}\n✻ Cogitated for 3m 11s · 1 shell still running");
    assert!(is_busy(&activity("claude", &screen)));
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_idle_prompt_is_idle
#[test]
fn port_026_alt_truncated_custom_footer_is_idle() {
    assert_eq!(
        activity("claude", &fixture("claude_idle_truncated_custom_footer.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_mid_turn_is_working
#[test]
fn port_026_alt_claude_spinner_status_after_summary_is_working() {
    let screen = "✻ Cogitated for 29s · done 8:27 AM\n✻ Moonwalking… (running stop hooks… 1/2 · 6s · ↓ 4 tokens)\n❯";
    assert_eq!(activity("claude", screen), AgentActivity::Working);
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_waiting_on_background_agents_is_waiting_background
#[test]
fn port_026_alt_new_background_wait_after_summary_is_not_at_rest() {
    let screen = "✻ Cooked for 12s · done 4:57 PM\n✻ Waiting for 1 background agent to finish";
    assert!(is_busy(&activity("claude", screen)));
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_folder_trust_modal_never_receives_initial_input
#[test]
fn port_026_alt_claude_folder_trust_fixture_reports_no_goal() {
    assert_eq!(goal("claude", &fixture("claude_folder_trust.txt")), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_activity_word_embedded_in_a_longer_word_is_not_working
#[test]
fn port_026_alt_regenerating_word_reports_no_goal() {
    assert_eq!(
        goal("codex", "Regenerating… (5m) is not a status"),
        None
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_awaiting_approval_is_waiting_approval
#[test]
fn port_026_alt_codex_numbered_modal_awaits_approval() {
    let text = "Select a mode\n› 1. Read only\n  2. Full access";
    assert_eq!(activity("codex", text), AgentActivity::AwaitingApproval);
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_completed_timing_summary_is_idle
#[test]
fn port_026_alt_codex_timing_summary_reports_no_goal() {
    let text = "Implemented the requested change… 12s\n\nSend a message";
    assert_eq!(goal("codex", text), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_folder_trust_modal_never_receives_initial_input
#[test]
fn port_026_alt_codex_trust_fixture_reports_no_goal() {
    assert_eq!(goal("codex", &fixture("codex_trust_folder.txt")), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_idle_prompt_is_idle
#[test]
fn port_026_alt_codex_idle_fixture_reports_no_goal() {
    assert_eq!(goal("codex", &fixture("codex_idle.txt")), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_mid_turn_is_working
#[test]
fn port_026_alt_codex_bullet_status_is_working() {
    assert_eq!(activity("codex", "• Working… (5m)"), AgentActivity::Working);
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_working_status_behind_a_leading_glyph_is_working
#[test]
fn port_026_alt_codex_status_after_prose_is_working() {
    let text = "tests are running fine · Running… (5m)";
    assert_eq!(activity("codex", text), AgentActivity::Working);
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_status_segment_after_a_prose_occurrence_of_the_same_word_is_working
#[test]
fn port_026_alt_codex_bullet_working_segment_is_working() {
    assert_eq!(activity("codex", "• Working… (5m)"), AgentActivity::Working);
}

/// REQ: PORT:ilium-detect/src/lib.rs::completed_goal_does_not_wait_on_historical_background_agent
#[test]
fn port_026_alt_latest_summary_after_running_suffix_is_idle() {
    let screen = "✻ Cooked for 12s · 1 shell still running\n● Shell finished\n✻ Cooked for 3s · done 5:00 PM";
    assert_eq!(activity("claude", screen), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/src/lib.rs::numbered_analysis_with_stray_cursor_elsewhere_is_idle
#[test]
fn port_026_alt_numbered_analysis_fixture_reports_no_goal() {
    assert_eq!(
        goal(
            "claude",
            &fixture("claude_code_numbered_analysis_with_stray_cursor.txt")
        ),
        None
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::plain_shell_prompt_has_no_activity_signal
#[test]
fn port_026_alt_plain_shell_reports_no_goal() {
    assert_eq!(goal("claude", &fixture("plain_shell.txt")), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::prose_mentioning_background_or_waiting_alone_is_idle_not_waiting_background
#[test]
fn port_026_alt_background_prose_reports_no_goal() {
    let text = "I'll run this in the background and keep waiting for input.";
    assert_eq!(goal("claude", text), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::prose_mentioning_shell_or_running_alone_is_idle_not_waiting_background
#[test]
fn port_026_alt_running_prose_reports_no_goal() {
    assert_eq!(goal("claude", "The dev server is running now."), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::prose_mentioning_yes_and_no_is_idle_not_waiting_approval
#[test]
fn port_026_alt_yes_and_no_prose_reports_no_goal() {
    assert_eq!(
        goal("claude", &fixture("claude_code_prose_with_yes_no.txt")),
        None
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::question_dialog_outranks_stale_working_marker
#[test]
fn port_026_alt_question_dialog_outranks_a_stale_herding_marker() {
    let stale = format!(
        "✻ Herding… (3m 2s · esc to interrupt)\n{}",
        fixture("claude_ask_user_question_with_goal_history.txt")
    );
    assert_eq!(activity("claude", &stale), AgentActivity::AwaitingApproval);
}

/// REQ: PORT:ilium-detect/src/lib.rs::question_with_yes_no_as_substrings_of_other_words_is_not_waiting_approval
#[test]
fn port_026_alt_rhetorical_question_is_idle() {
    assert_eq!(
        activity("claude", "Does that make sense, or not?"),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::rhetorical_question_with_not_is_not_waiting_approval
#[test]
fn port_026_alt_substring_question_is_idle() {
    let text = "Did yesterday's changes land, or is there nothing new?";
    assert_eq!(activity("claude", text), AgentActivity::Idle);
}

/// REQ: PORT:ilium-detect/src/lib.rs::rename_plan_confirmation_menu_is_waiting_approval
#[test]
fn port_026_alt_rename_confirmation_fixture_reports_no_goal() {
    assert_eq!(
        goal("claude", &fixture("claude_code_rename_confirm_prompt.txt")),
        None
    );
}
