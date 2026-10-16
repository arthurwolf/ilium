//! Twins of the goal classification tests of ilium-detect/src/lib.rs. The plugin reports `Classification.goal` as an
//! optional text; the phase word is not specified, so the twins assert presence or absence and that the phases
//! that differ in the original yield text for the same fixtures.

use crate::common::{fixture, goal};

const CODEX_FOOTER_PREFIX: &str = "• Done\n\n› Ask Codex to do anything\n\n  model · workspace · Ready · Conte… ";
const CODEX_FOOTER_SUFFIX: &str = "    ⚠ 1 warning · f2 to view";

fn codex_footer(segment: &str) -> String {
    format!("{CODEX_FOOTER_PREFIX}{segment}{CODEX_FOOTER_SUFFIX}")
}

/// REQ: PORT:ilium-detect/src/lib.rs::captured_codex_goal_footers_map_to_their_goal_phase
#[test]
fn port_028_captured_codex_footers_report_a_goal_or_none() {
    for name in [
        "codex_goal_pursuing_warning_footer.txt",
        "codex_goal_active_wide_footer.txt",
        "codex_goal_two_line_active.txt",
        "codex_goal_two_line_resumed.txt",
        "codex_goal_paused_while_turn_finishes.txt",
        "codex_goal_paused_after_interrupt.txt",
        "codex_goal_two_line_paused.txt",
        "codex_goal_stalled_narrow.txt",
        "codex_goal_achieved_warning_footer.txt",
        "codex_goal_achieved_wide_footer.txt",
    ] {
        assert!(goal("codex", &fixture(name)).is_some(), "{name}");
    }
    for name in ["codex_goal_cleared.txt", "codex_no_goal_metadata_footer.txt"] {
        assert_eq!(goal("codex", &fixture(name)), None, "{name}");
    }
}

/// REQ: PORT:ilium-detect/src/lib.rs::every_codex_footer_goal_phase_is_recognized_between_other_segments
#[test]
fn port_028_every_codex_footer_phase_is_recognized() {
    for segment in [
        "Pursuing goal (1h 5m)",
        "Goal paused (/goal resume)",
        "Goal stalled (/goal resume)",
        "Goal blocked (/goal resume)",
        "Goal hit usage limits (/goal resume)",
        "Goal unmet (12m)",
        "Goal achieved (20m)",
    ] {
        assert!(goal("codex", &codex_footer(segment)).is_some(), "{segment}");
    }
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_goal_wording_outside_the_footer_is_ignored
#[test]
fn port_028_codex_goal_wording_outside_the_footer_is_ignored() {
    let text = "• Done\n\nThe goal was Pursuing goal (1h 5m) earlier.\n\n› Ask Codex to do anything";
    assert_eq!(goal("codex", text), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_overlays_that_replace_the_footer_are_inconclusive
#[test]
fn port_028_overlays_replacing_the_footer_report_no_goal() {
    for overlay in ["/goal", "? for shortcuts"] {
        assert_eq!(goal("codex", &format!("• Done\n\n{overlay}")), None, "{overlay}");
    }
}

/// REQ: PORT:ilium-detect/src/lib.rs::captured_claude_code_goal_screens_map_to_their_goal_phase
#[test]
fn port_028_captured_claude_goal_screens_report_a_goal_or_none() {
    for name in [
        "claude_goal_active_working.txt",
        "claude_goal_active_shared_indicator_row.txt",
        "claude_goal_interrupted_still_active.txt",
        "claude_goal_paused_checks_capped.txt",
        "claude_goal_achieved.txt",
    ] {
        assert!(goal("claude", &fixture(name)).is_some(), "{name}");
    }
    for name in ["claude_goal_cleared.txt", "claude_no_goal_idle.txt"] {
        assert_eq!(goal("claude", &fixture(name)), None, "{name}");
    }
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_goal_notices_only_count_inside_the_current_turn
#[test]
fn port_028_claude_goal_notices_count_only_in_the_current_turn() {
    let composer = "────────────────────────────────────────\n❯\n────────────────────────────────────────\n  ⏵⏵ auto mode on (shift+tab to cycle)";
    assert!(goal("claude", &format!("◎ /goal active (4m)\n\n{composer}")).is_some());
    assert!(goal("claude", &format!("✗ Goal could not be achieved\n\n{composer}")).is_some());
    assert_eq!(goal("claude", "◎ /goal active (4m)"), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_goal_wording_outside_the_indicator_row_is_ignored
#[test]
fn port_028_claude_goal_wording_outside_the_indicator_row_is_ignored() {
    let composer = "────────────────────────────────────────\n❯\n────────────────────────────────────────\n  ⏵⏵ auto mode on (shift+tab to cycle)";
    assert_eq!(goal("claude", &format!("We discussed /goal active (4m) yesterday.\n\n{composer}")), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_without_a_visible_composer_is_inconclusive
#[test]
fn port_028_no_visible_composer_reports_no_goal() {
    assert_eq!(goal("claude", "◎ /goal active (4m)\nsome output"), None);
}
