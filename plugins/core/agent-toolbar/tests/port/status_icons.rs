//! Twins of status_icons.rs: status icon explanations, provenance and worktree tooltips.
//! Request/reply shape: see API_GAPS_ports.md.
use serde_json::Value;
use crate::common::{ask, Place};

fn check(place: Place, op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(place, op, request);
    let fields = expected.as_object().expect("expected is an object");
    for (name, value) in fields {
        assert_eq!(&reply[name.as_str()], value, "field {name} of reply {reply}");
    }
}

/// REQ: PORT:ilium-client/src/status_icons.rs::detector_provenance_names_the_rule_and_sanitizes_terminal_text
#[test]
fn port_status_icons_detector_provenance_names_the_rule_and_sanitizes_terminal_text() {
    check(
        Place::Client,
        "status.provenance",
        r####"{"esc": "\u001b"}"####,
        r####"{"includes": ["rule", "observed text", "context"], "esc_replaced_by": "�"}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::every_non_empty_signal_has_an_explanation
#[test]
fn port_status_icons_every_non_empty_signal_has_an_explanation() {
    check(
        Place::Client,
        "status.explanation",
        r####"{}"####,
        r####"{"all_non_empty_signals_explained": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::family_members_animate_from_their_own_frame_and_others_stay_static
#[test]
fn port_status_icons_family_members_animate_from_their_own_frame_and_others_stay_static() {
    check(
        Place::Client,
        "status.animate",
        r####"{"cases": [["🕗", "background-clock", 220, 0], ["🕗", "background-clock", 220, 220], ["◐", "spinner", 90, 900]]}"####,
        r####"{"first": "🕗", "second_differs": true, "spinner_static": "◐"}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::goal_and_parked_explanations_describe_observed_state_without_claiming_control
#[test]
fn port_status_icons_goal_and_parked_explanations_describe_observed_state_without_claiming_control() {
    check(
        Place::Client,
        "status.explanation",
        r####"{"signals": ["goal-paused", "goal-usage-limited", "parked"]}"####,
        r####"{"goal_contains": ["provider surfaced", "provider reports", "does not control"], "goal_excludes": ["/goal resume"], "parked_contains": ["live progress monitor linked to this pane", "depends on provider and pane state"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::identity_provenance_fallback_matches_the_server_status_kind
#[test]
fn port_status_icons_identity_provenance_fallback_matches_the_server_status_kind() {
    check(
        Place::Client,
        "status.identity",
        r####"{}"####,
        r####"{"codex_contains": ["Codex agent", "detector details were not included yet"], "editor_dirty_contains": "dirty=true", "board_contains": "board pane", "detector_reason_overrides": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::missing_identity_provenance_matches_each_pane_content_kind
#[test]
fn port_status_icons_missing_identity_provenance_matches_each_pane_content_kind() {
    check(
        Place::Client,
        "status.identity-missing",
        r####"{}"####,
        r####"{"terminal_contains": "process-classification evidence", "editor_board_contain": ["editor pane", "board pane", "rather than an agent process"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::missing_worktree_and_control_characters_are_explicit
#[test]
fn port_status_icons_missing_worktree_and_control_characters_are_explicit() {
    check(
        Place::Client,
        "status.worktree-tooltip",
        r####"{"missing": true, "branch": "agent/evil\u0007name"}"####,
        r####"{"contains": ["Live Git status: worktree missing", "Branch: agent/evil�name"], "excludes": ["Changes:"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::owned_worktree_tooltip_renders_above_a_second_line_anchor_near_bottom
#[test]
fn port_status_icons_owned_worktree_tooltip_renders_above_a_second_line_anchor_near_bottom() {
    check(
        Place::Client,
        "status.tooltip-render",
        r####"{"anchor_line": 2, "near_bottom": true}"####,
        r####"{"renders_above": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::progress_frames_are_two_cells_and_monotonic_in_fill
#[test]
fn port_status_icons_progress_frames_are_two_cells_and_monotonic_in_fill() {
    check(
        Place::Client,
        "status.progress",
        r####"{}"####,
        r####"{"frame_width": 2, "dot_count_monotonic": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::provenance_is_rendered_below_the_description_in_muted_text
#[test]
fn port_status_icons_provenance_is_rendered_below_the_description_in_muted_text() {
    check(
        Place::Client,
        "status.tooltip-render",
        r####"{"provenance": "Why:"}"####,
        r####"{"why_row_below_body": true, "colour": "dark-gray"}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::provenance_stays_visible_when_a_long_body_exceeds_the_screen_height
#[test]
fn port_status_icons_provenance_stays_visible_when_a_long_body_exceeds_the_screen_height() {
    check(
        Place::Client,
        "status.tooltip-render",
        r####"{"long_body": true}"####,
        r####"{"why_row_visible": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::shell_activity_provenance_reports_the_latest_cause_and_suppresses_stale_phase
#[test]
fn port_status_icons_shell_activity_provenance_reports_the_latest_cause_and_suppresses_stale_phase() {
    check(
        Place::Client,
        "status.shell-activity",
        r####"{}"####,
        r####"{"contains": ["KeyInput", "bytes.len() = 1 > 0", "Server/PTY receipt and execution unconfirmed"], "slow_phase": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::shell_activity_reason_sanitizes_and_bounds_observed_terminal_text
#[test]
fn port_status_icons_shell_activity_reason_sanitizes_and_bounds_observed_terminal_text() {
    check(
        Place::Client,
        "status.shell-activity",
        r####"{"visible_text": true}"####,
        r####"{"contains": ["seq 9..10", "r2 at update", "[truncated]"], "excludes": ["\u202e"], "max_chars": 512}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::shorter_progress_family_maps_zero_and_bucket_boundaries
#[test]
fn port_status_icons_shorter_progress_family_maps_zero_and_bucket_boundaries() {
    check(
        Place::Client,
        "status.progress-family",
        r####"{"buckets": [0, 1, 3, 4, 6, 7, 9, 10, 12]}"####,
        r####"{"glyphs": ["🌑", "🌒", "🌒", "🌓", "🌓", "🌔", "🌔", "🌕", "🌕"], "preset_index": 2}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::unavailable_agent_has_distinct_recovery_glyph_and_explicit_uncertainty
#[test]
fn port_status_icons_unavailable_agent_has_distinct_recovery_glyph_and_explicit_uncertainty() {
    check(
        Place::Client,
        "status.explanation",
        r####"{"signal": "agent-unavailable"}"####,
        r####"{"glyph": "🛟", "stable_glyph": "R", "contains": ["no crash or clean exit is inferred", "did not establish a live agent composer"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::workspace_tooltip_distinguishes_unchecked_live_state_from_a_clean_checkout
#[test]
fn port_status_icons_workspace_tooltip_distinguishes_unchecked_live_state_from_a_clean_checkout() {
    check(
        Place::Client,
        "status.worktree-tooltip",
        r####"{"branch": "agent/fix", "base": "main @ 3f2a9c1", "worktree": "/repo.worktrees/agent/fix", "git_dir": "/repo/.git", "created": "2023-11-14 22:13:20 UTC"}"####,
        r####"{"contains": ["Branch: agent/fix", "Base: main @ 3f2a9c1", "Worktree: /repo.worktrees/agent/fix", "Repository Git dir: /repo/.git", "Created: 2023-11-14 22:13:20 UTC", "Live Git status: not yet checked"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/status_icons.rs::workspace_tooltip_reports_live_counts_and_foreign_ownership
#[test]
fn port_status_icons_workspace_tooltip_reports_live_counts_and_foreign_ownership() {
    check(
        Place::Client,
        "status.worktree-tooltip",
        r####"{"status": {"staged": 3, "modified": 4, "untracked": 5, "conflicted": 0, "upstream": "origin/agent/fix", "ahead": 2, "behind": 1, "last_commit": "Finish fix"}, "foreign": true}"####,
        r####"{"contains": ["Changes: 3 staged, 4 modified, 5 untracked, 0 conflicted", "Upstream: origin/agent/fix (+2 / -1)", "Last commit: Finish fix", "will not remove"]}"####,
    );
}
