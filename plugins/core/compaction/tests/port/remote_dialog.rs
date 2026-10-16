//! Twins of the inline tests in ilium-client/src/remote_compaction_dialog.rs (the progress
//! dialog of a remote compaction). Gaps: `dialog_open`, `dialog_event`, `dialog_state`,
//! `dialog_fail`, `dialog_set_agent_stopped`, `dialog_render`, `dialog_hint`,
//! `dialog_banner_close_width`, `dialog_token_bar_lines` (see ../API_GAPS_port.md).

use serde_json::json;

use ilium_plugin_sdk::testing::FakeHost;

use crate::common::*;

const PRIVACY_NOTICE: &str = "Remote compaction sends this session's transcript (your prompts, code and tool output, possibly including secrets) to Kilo Gateway / stepfun/step-5-preview-free as currently configured in the Inference tab. That is a privacy decision.";

fn dialog() -> FakeHost {
    let mut host = client_host();
    host.dialog_open(json!({
        "provider": "claude",
        "session_id": "11111111-1111-4111-8111-111111111111",
        "project_cwd": "/tmp/project",
        "transcript_path": "/tmp/project/session.jsonl",
        "technique": "claude-code",
        "destination": "Kilo Gateway / test-model",
        "is_automatic": false,
    }));
    host
}

/// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::events_update_steps_progress_tokens_and_log
#[test]
fn rdlg_events_update_steps_progress_tokens_and_log() {
    let mut host = dialog();
    host.dialog_event(json!({"phase": "compacting"}));
    host.dialog_event(json!({"step": {"index": 3, "total": 4, "title": "Summarizing"}}));
    host.dialog_event(json!({"progress": 0.5}));
    host.dialog_event(json!({"progress": 0.2}));
    host.dialog_event(json!({"tokens": {"before_context": 130000}}));
    let state = host.dialog_state();
    assert_eq!(state["current_step"], 3);
    assert_eq!(state["active_stage"], state["first_crate_stage"].as_u64().unwrap() + 2);
    assert!((state["progress"].as_f64().unwrap() - 0.5).abs() < 1e-6, "progress never goes back");
    assert_eq!(state["tokens"]["before_context"], 130000);
    assert_eq!(state["log"].as_array().unwrap().last().unwrap(), "[3/4] Summarizing");
}

/// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::failure_marks_the_running_stage
#[test]
fn rdlg_failure_marks_the_running_stage() {
    let mut host = dialog();
    host.dialog_fail("no network");
    let state = host.dialog_state();
    assert_eq!(state["active_stage"], state["wait_stage"]);
    host.dialog_set_agent_stopped(true);
    let state = host.dialog_state();
    assert_eq!(state["active_stage"], state["stop_stage"]);
    host.dialog_event(json!({"step": {"index": 3, "total": 4, "title": "Summarizing"}}));
    let state = host.dialog_state();
    assert_eq!(state["active_stage"], state["first_crate_stage"].as_u64().unwrap() + 2);
}

/// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::render_shows_stages_tokens_destination_and_banner
#[test]
fn rdlg_render_shows_stages_tokens_destination_and_banner() {
    let mut host = dialog();
    host.dialog_event(json!({"tokens": {
        "before_context": 130000, "conversation_total": 140000, "masked_savings": 40000,
        "summarizer_input": 100000, "summarizer_output": 9000, "tail_kept": 20000,
        "after_context": 29000, "window": 200000,
    }}));
    let text = host.dialog_render(100, 44, Some("Your session is sent to Kilo Gateway / test-model."));
    for expected in [
        "Remote compaction", "Wait for a safe pause", "Restart the agent", "Context before",
        "Summarizer input", "Context after", "65% of 200", "Kilo Gateway / test-model",
        "Your session is sent",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }
    assert!(host.dialog_banner_close_width() > 0, "the close button rect is published for mouse hit testing");
}

/// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::render_without_banner_publishes_no_close_rect
#[test]
fn rdlg_render_without_banner_publishes_no_close_rect() {
    let mut host = dialog();
    let text = host.dialog_render(100, 44, None);
    assert!(!text.contains("privacy"));
    assert_eq!(host.dialog_banner_close_width(), 0);
}

/// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::failed_dialog_offers_resume_once_the_agent_is_stopped
#[test]
fn rdlg_failed_dialog_offers_resume_once_the_agent_is_stopped() {
    let mut host = dialog();
    host.dialog_fail("boom");
    assert_eq!(host.dialog_hint(), "Esc close");
    host.dialog_set_agent_stopped(true);
    assert!(host.dialog_hint().contains("resume the original session"));
}

/// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::failed_dialog_keeps_disclosure_error_and_actions_at_narrow_and_wide_sizes
#[test]
fn rdlg_failed_dialog_keeps_disclosure_error_and_actions_at_narrow_and_wide_sizes() {
    let mut host = dialog();
    host.dialog_set_agent_stopped(true);
    host.dialog_fail("Synthetic provider failure: the session can be resumed safely");
    for (width, height) in [(40, 12), (60, 20), (80, 24), (120, 40)] {
        let text = host.dialog_render(width, height, Some(PRIVACY_NOTICE));
        for expected in [
            "Privacy decision", "[x]", "prompts", "code", "tool output", "secrets", "Kilo Gateway",
            "stepfun/step-5-preview-free", "Failed:", "safely", "Enter resume", "Esc close", "x hide",
        ] {
            assert!(text.contains(expected), "{width}x{height}: missing {expected:?} in\n{text}");
        }
        assert_eq!(host.dialog_banner_close_width(), 3);
    }
    let dismissed = host.dialog_render(40, 12, None);
    assert!(dismissed.contains("Enter resume"), "{dismissed}");
    assert!(dismissed.contains("Esc close"), "{dismissed}");
    assert!(!dismissed.contains("x hide"), "{dismissed}");
    assert_eq!(host.dialog_banner_close_width(), 0);
}

/// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::running_dialog_keeps_disclosure_phase_progress_and_cancel_at_narrow_sizes
#[test]
fn rdlg_running_dialog_keeps_disclosure_phase_progress_and_cancel_at_narrow_sizes() {
    let mut host = dialog();
    host.dialog_event(json!({"progress": 0.6}));
    for (width, height) in [(40, 12), (60, 20)] {
        let text = host.dialog_render(width, height, Some(PRIVACY_NOTICE));
        for expected in ["Privacy decision", "[x]", "Waiting for Claude", "60%", "Esc cancel", "x hide"] {
            assert!(text.contains(expected), "{width}x{height}: missing {expected:?} in\n{text}");
        }
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::oversized_disclosure_requests_more_room_without_hiding_failure_actions
#[test]
fn rdlg_oversized_disclosure_requests_more_room_without_hiding_failure_actions() {
    let mut host = dialog();
    host.dialog_set_agent_stopped(true);
    host.dialog_fail("network error");
    let notice = format!("{PRIVACY_NOTICE} {}", "very-long-model-identifier".repeat(20));
    let text = host.dialog_render(40, 12, Some(&notice));
    assert!(text.contains("Privacy notice too long"), "{text}");
    assert!(text.contains("Enter resume"), "{text}");
    assert!(text.contains("Esc close"), "{text}");
    assert_eq!(host.dialog_banner_close_width(), 0);
}

/// REQ: PORT:ilium-client/src/remote_compaction_dialog.rs::bars_scale_to_the_largest_figure_and_show_small_values
#[test]
fn rdlg_bars_scale_to_the_largest_figure_and_show_small_values() {
    let host = dialog();
    let lines = host.dialog_token_bar_lines(json!({"before_context": 100000, "tail_kept": 1}), 10);
    assert!(lines[0].contains(&"\u{2588}".repeat(10)));
    assert!(lines[4].contains('\u{2588}'), "non-zero values keep one cell");
    assert!(!lines[3].contains('\u{2588}'), "zero shows no cells");
}
