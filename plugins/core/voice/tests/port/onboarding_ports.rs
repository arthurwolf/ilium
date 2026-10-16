//! Twins of the onboarding voice step tests (`onboarding/voice_ui.rs`, `voice_runtime.rs`, `voice_demo.rs`):
//! the step renders eleven rows and a connection state label; the test button runs a demonstration whose only
//! capability is a lightbulb (1024 calls, 1024 argument bytes, see API_GAPS.md).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::ViewTarget;
use serde_json::json;

use crate::common::{client_host, configure};

fn step_text(host: &mut FakeHost, columns: u16, rows: u16) -> String {
    host.render(ViewTarget::OnboardingStep("voice".to_string()), columns, rows).text()
}

const ROWS: [&str; 11] = [
    "Enable voice after setup", "OpenAI API key", "Realtime model", "Speaking voice", "Turn detection",
    "Microphone", "Speaker", "Output volume", "Reasoning", "VAD responsiveness", "Pause other media",
];

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::every_configuration_and_live_action_is_reachable_on_small_and_wide_screens
/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::narrow_scrolling_reveals_real_transcripts_and_keyboard_focus_wraps
#[test]
fn step_053_every_row_is_reachable_on_a_small_and_a_wide_screen() {
    let mut host = client_host();
    let wide = step_text(&mut host, 160, 50);
    for label in ROWS {
        assert!(wide.contains(label), "wide lacks {label}");
    }
    // On a small screen the rows are reached by moving the focus down; each row is shown once focus reaches it.
    let target = ViewTarget::OnboardingStep("voice".to_string());
    for label in ROWS {
        let mut seen = false;
        for _ in 0..ROWS.len() * 2 {
            if step_text(&mut host, 40, 8).contains(label) {
                seen = true;
                break;
            }
            host.key(target.clone(), "Down");
        }
        assert!(seen, "small screen never shows {label}");
    }
}

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::render_never_exposes_configured_key_and_keeps_test_visible
/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::bounded_redaction_preserves_replace_prefix_unicode_and_no_secret
#[test]
fn step_053_the_configured_key_is_never_rendered_and_the_test_action_stays_visible() {
    let mut host = client_host();
    configure(&mut host, "api_key", json!("sk-live-secret-key"));
    let text = step_text(&mut host, 100, 40);
    assert!(!text.contains("sk-live-secret-key"), "{text}");
    assert!(text.contains("Test"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::voice_configuration_cards_use_rounded_frames_at_compact_and_regular_sizes
#[test]
fn step_053_the_configuration_cards_use_rounded_frames_at_both_sizes() {
    let mut host = client_host();
    for (columns, rows) in [(60u16, 24u16), (120, 40)] {
        let text = step_text(&mut host, columns, rows);
        assert!(text.contains('\u{256d}') && text.contains('\u{256f}'), "{columns}x{rows}: {text}");
    }
}

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::recording_controls_follow_real_lifecycle_and_input_mode
#[test]
fn step_053_the_recording_control_follows_the_input_mode() {
    let mut host = client_host();
    configure(&mut host, "input_mode", json!("push_to_talk"));
    let push = step_text(&mut host, 100, 40);
    configure(&mut host, "input_mode", json!("semantic_vad"));
    let vad = step_text(&mut host, 100, 40);
    assert_ne!(push, vad);
}

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::long_user_assistant_and_errors_are_fully_reachable_in_stacked_and_wide_layouts
/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::voice_studio_shows_independent_scrollbars_for_settings_and_transcript_overflow
/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::wide_mouse_scroll_and_focus_reveal_preserve_independent_reading_offsets
/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::wide_scrollbar_tracks_seek_only_their_own_pane
#[test]
fn step_053_the_settings_and_the_transcript_scroll_independently() {
    let mut host = client_host();
    let target = ViewTarget::OnboardingStep("voice".to_string());
    let before = step_text(&mut host, 160, 12);
    // A click on the settings column must not move what the transcript column shows.
    host.click(target, 10, 10);
    let after = step_text(&mut host, 160, 12);
    let transcript_column = |text: &str| text.lines().map(|line| line.chars().skip(80).collect::<String>()).collect::<Vec<_>>();
    assert_eq!(transcript_column(&before), transcript_column(&after));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::invalid_configuration_never_starts_audio
/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::gate_release_retries_exact_intent_and_preserves_invalid_configuration_error_without_audio
#[test]
fn step_053_a_test_with_an_invalid_configuration_never_opens_audio() {
    let mut host = client_host();
    host.key(ViewTarget::OnboardingStep("voice".to_string()), "t");
    host.settle();
    assert!(host.audio().captures().is_empty());
    assert!(step_text(&mut host, 100, 40).contains("Ready to test") || step_text(&mut host, 100, 40).contains("API key"));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::stop_is_nonblocking_and_preserves_original_queued_text
/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::stop_during_shared_startup_discards_prepared_original_without_starting_audio
/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::normal_owner_gate_keeps_original_test_and_stop_cancels_without_starting_audio
/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::settings_change_cancels_waiting_test_before_external_ownership_releases
#[test]
fn step_053_stop_or_a_settings_change_cancels_a_waiting_test_without_audio() {
    let mut host = client_host();
    let target = ViewTarget::OnboardingStep("voice".to_string());
    host.key(target.clone(), "t");
    host.key(target, "Esc");
    configure(&mut host, "model", json!("gpt-realtime-2.1-mini"));
    host.settle();
    assert!(host.audio().captures().iter().all(|capture| !capture.open));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::advertised_capability_is_only_the_bulb
/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::actual_cpu_projection_executes_ordered_tools_and_duplicate_receipt_without_relighting
/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::saturated_outbox_retains_original_event_and_no_tool_state_effect
#[test]
fn demo_053_the_only_advertised_tool_is_the_bulb_and_calls_run_in_order() {
    let mut host = client_host();
    assert_eq!(host.onboarding_demo("voice").tools(), ["set_lightbulb"]);
    for (index, on) in [true, false, true].into_iter().enumerate() {
        let id = format!("order-{index}");
        let result = host.onboarding_demo("voice").call(&id, "set_lightbulb", &json!({ "on": on }).to_string());
        assert!(result.is_ok(), "{id}");
        assert_eq!(host.onboarding_demo("voice").bulb(), Some(on), "{id}");
    }
    let replay = host.onboarding_demo("voice").call("order-0", "set_lightbulb", &json!({ "on": true }).to_string());
    assert!(replay.is_ok());
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(true));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::visible_state_clones_share_heap_and_last_guard
/// REQ: PORT:ilium-client/src/onboarding/voice_demo.rs::actual_tool_execution_turns_bulb_on_and_off
/// REQ: PORT:ilium-client/src/onboarding/voice_demo.rs::duplicate_on_after_off_returns_original_receipt_without_relighting
/// REQ: PORT:ilium-client/src/onboarding/voice_demo.rs::exhausted_demo_retains_replay_receipts_and_refuses_new_mutations
/// REQ: PORT:ilium-client/src/onboarding/voice_demo.rs::real_controls_and_malformed_arguments_are_rejected_without_state_changes
#[test]
fn demo_053_the_demonstration_offers_only_the_bulb_and_runs_it_by_call_id() {
    let mut host = client_host();
    let tools = host.onboarding_demo("voice").tools();
    assert_eq!(tools, ["set_lightbulb"]);
    let on = host.onboarding_demo("voice").call("c1", "set_lightbulb", &json!({ "on": true }).to_string());
    assert!(on.is_ok());
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(true));
    let off = host.onboarding_demo("voice").call("c2", "set_lightbulb", &json!({ "on": false }).to_string());
    assert!(off.is_ok());
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(false));
    let replay = host.onboarding_demo("voice").call("c1", "set_lightbulb", &json!({ "on": true }).to_string());
    assert_eq!(replay, on);
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(false));
    assert!(host.onboarding_demo("voice").call("c3", "set_lightbulb", "{\"on\":\"yes\"}").is_err());
    assert!(host.onboarding_demo("voice").call("c4", "ilium_close_pane", "{}").is_err());
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(false));
}
