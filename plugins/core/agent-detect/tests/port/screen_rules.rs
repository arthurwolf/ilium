//! Twins of the Claude live-status and goal-anchor tests of ilium-detect/src/lib.rs that arrived
//! after the screen rules were vendored, asserted directly on the plugin's `screen` module (the
//! rules every `classify` and `classify-batch` verdict runs).

use ilium_plugin_core_agent_detect::screen::{
    classify_activity_for_agent, classify_activity_for_agent_detailed, goal_evidence_for_agent,
    ActivityEvidence, AgentClass, AgentTurn, GoalEvidence, GoalState,
};

use crate::common::fixture;

#[test]
fn claude_idle_truncated_custom_footer_is_not_live_activity() {
    // Captured read-only from a completed live Claude pane, 2026-10-17;
    // account information is redacted in the fixture.
    let screen = fixture("claude_idle_truncated_custom_footer.txt");
    let classification = classify_activity_for_agent_detailed(&AgentClass::Claude, &screen);
    assert_eq!(classification.turn, AgentTurn::Idle);
    assert_eq!(classification.evidence, ActivityEvidence::NoActiveMarker);
    assert_eq!(classification.matched_line, None);
}

#[test]
fn claude_live_status_requires_terminal_chrome() {
    for line in [
        "Session: shared observation (first seen 4m …",
        "The operation took 6s … and is complete.",
        "● The operation… (6s) finished successfully.",
        "✻ The operation took 6s … and is complete.",
        "✻ Thinking… yesterday took 6s",
    ] {
        assert_eq!(
            classify_activity_for_agent(&AgentClass::Claude, line),
            AgentTurn::Idle,
            "{line}"
        );
    }
}

#[test]
fn claude_custom_footer_cannot_supply_live_activity_chrome() {
    let screen = "✻ Cogitated for 29s · done 8:27 AM\n────────\n❯\n────────\n✻ Observing… (4m)\n";
    assert_eq!(
        classify_activity_for_agent(&AgentClass::Claude, screen),
        AgentTurn::Idle
    );
}

#[test]
fn claude_spinner_frames_preserve_live_status_after_summary() {
    for spinner in ['·', '✢', '✳', '∗', '✻', '✽'] {
        let live_line =
            format!("{spinner} Moonwalking… (running stop hooks… 1/2 · 6s · ↓ 4 tokens)");
        let screen = format!("✻ Cogitated for 29s · done 8:27 AM\n{live_line}\n❯");
        let classification = classify_activity_for_agent_detailed(&AgentClass::Claude, &screen);
        assert_eq!(classification.turn, AgentTurn::Working);
        assert_eq!(classification.evidence, ActivityEvidence::ClaudeLiveStatus);
        assert_eq!(
            classification.matched_line.as_deref(),
            Some(live_line.as_str())
        );
    }
}

#[test]
fn claude_ascii_and_six_point_spinner_frames_keep_active_turn_working() {
    for spinner in ['✶', '*'] {
        let screen = format!(
            "● Reading 1 file…\n  ⎿ questions/q12.txt\n\n{spinner} Considering… (51s · ↓ 3.5k tokens)\n\n────────────\n❯\n────────────\n  ⏵⏵ accept edits on (shift+tab to cycle)"
        );
        assert_eq!(
            classify_activity_for_agent(&AgentClass::Claude, &screen),
            AgentTurn::Working,
            "live Claude spinner frame {spinner:?} must not report idle"
        );
    }
}

/// Pane rows of a real Claude Haiku turn captured mid-run with a queued message: the live status
/// line carries the six-point `✶` frame, and the composer below shows the queue hint instead of
/// an empty prompt.
#[test]
fn real_claude_capture_with_six_point_spinner_and_queued_input_is_working() {
    assert_eq!(
        classify_activity_for_agent(
            &AgentClass::Claude,
            &fixture("claude_working_six_point_spinner_with_queued_input.txt")
        ),
        AgentTurn::Working
    );
}

#[test]
fn claude_ascii_bullet_in_transcript_is_not_live_spinner() {
    let screen = "* Considering… (51s · example)\n\nFinal answer.\n\n────────────\n❯\n────────────\n  ⏵⏵ accept edits on (shift+tab to cycle)";
    assert_eq!(
        classify_activity_for_agent(&AgentClass::Claude, screen),
        AgentTurn::Idle
    );
}

#[test]
fn claude_goal_indicator_below_composer_is_detected() {
    let screen = "✻ Connection refused · Retrying in 3s\n\
────────────────────────────────────────\n\
❯\n\
────────────────────────────────────────\n\
  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt\n\
                        ◎ /goal active (33s)";
    assert_eq!(
        goal_evidence_for_agent(&AgentClass::Claude, screen),
        GoalEvidence::State(GoalState::Active)
    );
}

#[test]
fn claude_goal_below_composer_requires_permission_footer() {
    let screen = "ordinary output\n\
────────────────────────────────────────\n\
❯\n\
────────────────────────────────────────\n\
  unrelated content\n\
                        ◎ /goal active (33s)";
    assert_eq!(
        goal_evidence_for_agent(&AgentClass::Claude, screen),
        GoalEvidence::Inactive
    );
}
