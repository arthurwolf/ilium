//! Synthetic screen-text regressions derived from the source packet.
//! These are not new live-provider captures and perform no PTY or network I/O.

use ilium_core::{AgentClass, AgentTurn, GoalState};
use ilium_detect::{
    classify_activity, classify_activity_detailed, classify_activity_for_agent,
    classify_activity_for_agent_detailed, goal_evidence_for_agent, ActivityEvidence,
    GoalEvidence,
};

const OLD_WAIT: &str = "✻ Waiting for 1 background agent to finish";
const NEW_WAIT: &str = "✻ Waiting for 2 background tasks to finish";
const DONE: &str = "✻ Cooked for 1h 29m 54s · done 4:57 PM";
const AGENT_FINISHED: &str = "● Agent \"Shoreline v2 waves and foam\" finished · 20m 0s";
const COMPOSER: &str = "────────────────────────────────────────\n❯\n────────────────────────────────────────\n  ⏵⏵ auto mode on (shift+tab to cycle)";

fn assert_shared_activity(
    screen: &str,
    turn: AgentTurn,
    evidence: ActivityEvidence,
    matched_line: Option<&str>,
) {
    assert_eq!(classify_activity(screen), turn, "{screen}");
    assert_eq!(
        classify_activity_for_agent(&AgentClass::Claude, screen),
        turn,
        "{screen}",
    );
    for actual in [
        classify_activity_detailed(screen),
        classify_activity_for_agent_detailed(&AgentClass::Claude, screen),
    ] {
        assert_eq!(actual.turn, turn, "{screen}");
        assert_eq!(actual.evidence, evidence, "{screen}");
        assert_eq!(actual.matched_line.as_deref(), matched_line, "{screen}");
    }
}

#[test]
fn reported_wait_then_agent_finished_then_final_summary_is_idle() {
    let screen = format!("{OLD_WAIT}\n{AGENT_FINISHED}\n{DONE}\n\n{COMPOSER}");
    assert_shared_activity(
        &screen,
        AgentTurn::Idle,
        ActivityEvidence::NoActiveMarker,
        None,
    );
}

#[test]
fn current_wait_without_a_completed_summary_remains_waiting() {
    assert_shared_activity(
        OLD_WAIT,
        AgentTurn::WaitingSubagents,
        ActivityEvidence::BackgroundWait,
        Some(OLD_WAIT),
    );
}

#[test]
fn one_subagent_finishing_does_not_prove_all_background_work_finished() {
    let screen = format!("{NEW_WAIT}\n{AGENT_FINISHED}\n\n{COMPOSER}");
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingSubagents,
        ActivityEvidence::BackgroundWait,
        Some(NEW_WAIT),
    );
}

#[test]
fn current_still_running_suffix_survives_the_completion_boundary() {
    for noun in ["shell", "monitor", "background task"] {
        let summary = format!("✻ Cooked for 3m 6s · done 7:00 PM · 1 {noun} still running");
        let screen = format!("{OLD_WAIT}\n{summary}\n\n{COMPOSER}");
        assert_shared_activity(
            &screen,
            AgentTurn::Settling,
            ActivityEvidence::BackgroundTaskWait,
            Some(&summary),
        );
    }
}

#[test]
fn still_running_suffix_without_a_done_clock_remains_settling() {
    let summary = "✻ Cogitated for 3m 11s · 1 shell still running";
    let screen = format!("{OLD_WAIT}\n{summary}");
    assert_shared_activity(
        &screen,
        AgentTurn::Settling,
        ActivityEvidence::BackgroundTaskWait,
        Some(summary),
    );
}

#[test]
fn a_later_final_summary_supersedes_old_waits_and_old_still_running_suffixes() {
    let screen = format!(
        "{OLD_WAIT}\n✻ Cogitated for 3m 11s · 1 shell still running\n{DONE}\n\n{COMPOSER}"
    );
    assert_shared_activity(
        &screen,
        AgentTurn::Idle,
        ActivityEvidence::NoActiveMarker,
        None,
    );
}

#[test]
fn new_wait_below_an_old_completed_summary_is_current() {
    let screen = format!("{OLD_WAIT}\n{DONE}\n{NEW_WAIT}\n\n{COMPOSER}");
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingSubagents,
        ActivityEvidence::BackgroundWait,
        Some(NEW_WAIT),
    );
}

#[test]
fn new_wait_after_a_settling_summary_keeps_mid_turn_wait_precedence() {
    let screen = format!("✻ Cogitated for 3m 11s · 1 shell still running\n{NEW_WAIT}");
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingSubagents,
        ActivityEvidence::BackgroundWait,
        Some(NEW_WAIT),
    );
}

#[test]
fn historical_working_markers_above_completion_do_not_keep_the_turn_working() {
    for old_work in [
        "Working (esc to interrupt)",
        "✢ Moonwalking… (running stop hooks… 1/2 · 6s · ↓ 4 tokens)",
    ] {
        let screen = format!("{old_work}\n{OLD_WAIT}\n{DONE}\n\n{COMPOSER}");
        assert_shared_activity(
            &screen,
            AgentTurn::Idle,
            ActivityEvidence::NoActiveMarker,
            None,
        );
    }
}

#[test]
fn old_work_cannot_mask_a_new_wait_after_completion() {
    let screen = format!("Working (esc to interrupt)\n{DONE}\n{NEW_WAIT}");
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingSubagents,
        ActivityEvidence::BackgroundWait,
        Some(NEW_WAIT),
    );
}

#[test]
fn current_interrupt_marker_below_completion_is_working() {
    let current = "✻ Herding… (3m 2s · esc to interrupt)";
    let screen = format!("{OLD_WAIT}\n{DONE}\n{current}");
    assert_shared_activity(
        &screen,
        AgentTurn::Working,
        ActivityEvidence::InterruptMarker,
        Some(current),
    );
}

#[test]
fn current_live_status_keeps_generic_and_claude_evidence_distinct() {
    let current = "✢ Moonwalking… (running stop hooks… 1/2 · 6s · ↓ 4 tokens)";
    let screen = format!("{OLD_WAIT}\n{DONE}\n{current}");
    let generic = classify_activity_detailed(&screen);
    let claude = classify_activity_for_agent_detailed(&AgentClass::Claude, &screen);
    assert_eq!(generic.turn, AgentTurn::Working);
    assert_eq!(claude.turn, AgentTurn::Working);
    assert_eq!(generic.evidence, ActivityEvidence::GenericLiveStatus);
    assert_eq!(claude.evidence, ActivityEvidence::ClaudeLiveStatus);
    assert_eq!(generic.matched_line.as_deref(), Some(current));
    assert_eq!(claude.matched_line.as_deref(), Some(current));
}

#[test]
fn background_diagnostics_choose_the_newest_eligible_matching_line() {
    let screen = format!("{OLD_WAIT}\n{NEW_WAIT}");
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingSubagents,
        ActivityEvidence::BackgroundWait,
        Some(NEW_WAIT),
    );
}

#[test]
fn bottom_question_dialog_still_outranks_working_and_waiting() {
    let screen = format!(
        "{OLD_WAIT}\n{DONE}\n✻ Herding… (3m 2s · esc to interrupt)\n\
         Choose a path\n❯ 1. Continue\n  2. Chat about this\n\
         Enter to select · Esc to cancel\n"
    );
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingApproval,
        ActivityEvidence::SelectionPrompt,
        Some("❯ 1. Continue"),
    );
}

#[test]
fn visible_confirmation_remains_detectable_after_completion() {
    let question = "Continue, yes or no?";
    let screen = format!("{OLD_WAIT}\n{DONE}\n{question}");
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingApproval,
        ActivityEvidence::ConfirmationPrompt,
        Some(question),
    );
}

#[test]
fn approval_predicates_still_receive_the_complete_screen() {
    let question = "Continue, yes or no?";
    let screen = format!("{question}\n{DONE}");
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingApproval,
        ActivityEvidence::ConfirmationPrompt,
        Some(question),
    );
}

#[test]
fn folder_trust_after_completion_is_not_mistaken_for_idle() {
    let screen = format!(
        "{OLD_WAIT}\n{DONE}\nQuick safety check:\n\
         Yes, I trust this folder\nNo, continue without these permissions"
    );
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingApproval,
        ActivityEvidence::FolderTrustPrompt,
        Some("Yes, I trust this folder"),
    );
}

#[test]
fn goal_evidence_is_not_used_as_an_activity_override() {
    let screen = format!(
        "✔ Goal achieved (3m · 2 turns · 2.6k tokens)\n{DONE}\n{NEW_WAIT}\n\n{COMPOSER}"
    );
    assert_eq!(
        goal_evidence_for_agent(&AgentClass::Claude, &screen),
        GoalEvidence::State(GoalState::Reached),
    );
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingSubagents,
        ActivityEvidence::BackgroundWait,
        Some(NEW_WAIT),
    );
}

#[test]
fn supported_summary_durations_do_not_depend_on_a_particular_verb() {
    for summary in [
        "✻ Cogitated for 10s",
        "✻ Baked for 1m 59s · done 12:45 AM",
        "✻ Worked for 3m · done 12:45 AM",
        "✻ Cooked for 1h 29m 54s · done 4:57 PM",
        "✻ Cogitated for 1h",
    ] {
        let screen = format!("{OLD_WAIT}\n{summary}");
        assert_shared_activity(
            &screen,
            AgentTurn::Idle,
            ActivityEvidence::NoActiveMarker,
            None,
        );
    }
}

#[test]
fn prose_mentions_and_incomplete_summaries_do_not_erase_current_waits() {
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
        assert_shared_activity(
            &screen,
            AgentTurn::WaitingSubagents,
            ActivityEvidence::BackgroundWait,
            Some(OLD_WAIT),
        );
    }
}

#[test]
fn claude_completion_boundaries_are_not_imposed_on_other_known_providers() {
    let screen = format!("{OLD_WAIT}\n{DONE}");
    for class in [
        AgentClass::Codex,
        AgentClass::Antigravity,
        AgentClass::Other("custom".to_owned()),
    ] {
        let actual = classify_activity_for_agent_detailed(&class, &screen);
        assert_eq!(actual.turn, AgentTurn::WaitingSubagents);
        assert_eq!(actual.evidence, ActivityEvidence::BackgroundWait);
        assert_eq!(actual.matched_line.as_deref(), Some(OLD_WAIT));
    }
}

#[test]
fn codex_completed_timing_prose_still_is_not_a_live_status() {
    assert_eq!(
        classify_activity_for_agent(
            &AgentClass::Codex,
            "Implemented the requested change… 12s\n\nSend a message",
        ),
        AgentTurn::Idle,
    );
}

#[test]
fn utf8_and_crlf_preserve_byte_safe_current_evidence() {
    let screen = format!("é🦀\r\n{OLD_WAIT}\r\n{DONE}\r\n{NEW_WAIT}\r\n");
    assert_shared_activity(
        &screen,
        AgentTurn::WaitingSubagents,
        ActivityEvidence::BackgroundWait,
        Some(NEW_WAIT),
    );
}

#[test]
fn newest_status_evidence_remains_bounded_and_control_character_free() {
    let current = format!("{NEW_WAIT}\u{7} {}", "x".repeat(300));
    let screen = format!("{OLD_WAIT}\n{DONE}\n{current}");
    for actual in [
        classify_activity_detailed(&screen),
        classify_activity_for_agent_detailed(&AgentClass::Claude, &screen),
    ] {
        assert_eq!(actual.turn, AgentTurn::WaitingSubagents);
        assert_eq!(actual.evidence, ActivityEvidence::BackgroundWait);
        let line = actual.matched_line.expect("current waiting evidence");
        assert!(line.starts_with(NEW_WAIT));
        assert!(line.contains('�'));
        assert!(!line.chars().any(char::is_control));
        assert_eq!(line.chars().count(), 241);
        assert!(line.ends_with('…'));
    }
}
