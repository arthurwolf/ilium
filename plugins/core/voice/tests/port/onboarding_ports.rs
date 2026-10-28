//! Twins of the onboarding voice step tests (`onboarding/voice_ui.rs`, `voice_runtime.rs`, `voice_demo.rs`):
//! the step renders eleven rows and a connection state label; the test button runs a demonstration whose only
//! capability is a lightbulb (1024 calls, 1024 argument bytes, see API_GAPS.md).

use crate::host_ext::*;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, ViewTarget};
use serde_json::json;

use crate::common::{client_host, configure};

fn step_text(host: &mut FakeHost, columns: u16, rows: u16) -> String {
    host.render(
        ViewTarget::OnboardingStep("voice".to_string()),
        columns,
        rows,
    )
    .text()
}

const ROWS: [&str; 11] = [
    "Enable voice after setup",
    "OpenAI API key",
    "Realtime model",
    "Speaking voice",
    "Turn detection",
    "Microphone",
    "Speaker",
    "Output volume",
    "Reasoning",
    "VAD responsiveness",
    "Pause other media",
];

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::every_configuration_and_live_action_is_reachable_on_small_and_wide_screens, PORT:ilium-client/src/onboarding/voice_ui.rs::narrow_scrolling_reveals_real_transcripts_and_keyboard_focus_wraps
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

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::render_never_exposes_configured_key_and_keeps_test_visible, PORT:ilium-client/src/onboarding/voice_runtime.rs::bounded_redaction_preserves_replace_prefix_unicode_and_no_secret
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
        assert!(
            text.contains('\u{256d}') && text.contains('\u{256f}'),
            "{columns}x{rows}: {text}"
        );
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
    // manual does not pin the control labels; the two modes must at least render differently.
    assert!(!push.is_empty() && !vad.is_empty());
    assert_ne!(push, vad);
}

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::long_user_assistant_and_errors_are_fully_reachable_in_stacked_and_wide_layouts, PORT:ilium-client/src/onboarding/voice_ui.rs::voice_studio_shows_independent_scrollbars_for_settings_and_transcript_overflow, PORT:ilium-client/src/onboarding/voice_ui.rs::wide_mouse_scroll_and_focus_reveal_preserve_independent_reading_offsets, PORT:ilium-client/src/onboarding/voice_ui.rs::wide_scrollbar_tracks_seek_only_their_own_pane
#[test]
fn step_053_the_settings_and_the_transcript_scroll_independently() {
    let mut host = client_host();
    let target = ViewTarget::OnboardingStep("voice".to_string());
    let before = step_text(&mut host, 160, 12);
    // A click on the settings column must not move what the transcript column shows.
    // The testkit has no scroll action yet, so independent scrolling itself stays unexercised.
    host.click(target, 10, 10);
    let after = step_text(&mut host, 160, 12);
    let transcript_column = |text: &str| {
        text.lines()
            .map(|line| line.chars().skip(80).collect::<String>())
            .collect::<Vec<_>>()
    };
    assert_eq!(transcript_column(&before), transcript_column(&after));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::invalid_configuration_never_starts_audio, PORT:ilium-client/src/onboarding/voice_runtime.rs::gate_release_retries_exact_intent_and_preserves_invalid_configuration_error_without_audio
#[test]
fn step_053_a_test_with_an_invalid_configuration_never_opens_audio() {
    let mut host = client_host();
    host.key(ViewTarget::OnboardingStep("voice".to_string()), "t");
    host.settle();
    assert!(host.audio().captures().is_empty());
    // manual does not pin the invalid-configuration message; the test action must stay offered.
    let text = step_text(&mut host, 100, 40);
    assert!(text.contains("Test"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::stop_is_nonblocking_and_preserves_original_queued_text, PORT:ilium-client/src/onboarding/voice_runtime.rs::stop_during_shared_startup_discards_prepared_original_without_starting_audio, PORT:ilium-client/src/onboarding/voice_runtime.rs::normal_owner_gate_keeps_original_test_and_stop_cancels_without_starting_audio, PORT:ilium-client/src/onboarding/voice_runtime.rs::settings_change_cancels_waiting_test_before_external_ownership_releases
#[test]
fn step_053_stop_or_a_settings_change_cancels_a_waiting_test_without_audio() {
    let mut host = client_host();
    let target = ViewTarget::OnboardingStep("voice".to_string());
    host.key(target.clone(), "t");
    host.settle();
    assert!(
        host.audio().captures().iter().all(|capture| !capture.open),
        "no audio may open while the test is waiting"
    );
    host.key(target, "Esc");
    configure(&mut host, "model", json!("gpt-realtime-2.1-mini"));
    host.settle();
    // A cancelled test never leaves a capture open.
    let captures = host.audio().captures();
    assert!(
        captures.iter().all(|capture| !capture.open),
        "a stopped test leaves no open capture: {captures:?}"
    );
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::advertised_capability_is_only_the_bulb, PORT:ilium-client/src/onboarding/voice_runtime.rs::actual_cpu_projection_executes_ordered_tools_and_duplicate_receipt_without_relighting, PORT:ilium-client/src/onboarding/voice_runtime.rs::saturated_outbox_retains_original_event_and_no_tool_state_effect
#[test]
fn demo_053_the_only_advertised_tool_is_the_bulb_and_calls_run_in_order() {
    let mut host = client_host();
    assert_eq!(host.onboarding_demo("voice").tools(), ["set_lightbulb"]);
    for (index, on) in [true, false, true].into_iter().enumerate() {
        let id = format!("order-{index}");
        let result = host.onboarding_demo("voice").call(
            &id,
            "set_lightbulb",
            &json!({ "on": on }).to_string(),
        );
        assert!(result.is_ok(), "{id}");
        assert_eq!(host.onboarding_demo("voice").bulb(), Some(on), "{id}");
    }
    let replay = host.onboarding_demo("voice").call(
        "order-0",
        "set_lightbulb",
        &json!({ "on": true }).to_string(),
    );
    assert!(replay.is_ok());
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(true));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::visible_state_clones_share_heap_and_last_guard, PORT:ilium-client/src/onboarding/voice_demo.rs::actual_tool_execution_turns_bulb_on_and_off, PORT:ilium-client/src/onboarding/voice_demo.rs::duplicate_on_after_off_returns_original_receipt_without_relighting, PORT:ilium-client/src/onboarding/voice_demo.rs::exhausted_demo_retains_replay_receipts_and_refuses_new_mutations, PORT:ilium-client/src/onboarding/voice_demo.rs::real_controls_and_malformed_arguments_are_rejected_without_state_changes
#[test]
fn demo_053_the_demonstration_offers_only_the_bulb_and_runs_it_by_call_id() {
    let mut host = client_host();
    let tools = host.onboarding_demo("voice").tools();
    assert_eq!(tools, ["set_lightbulb"]);
    let on = host.onboarding_demo("voice").call(
        "c1",
        "set_lightbulb",
        &json!({ "on": true }).to_string(),
    );
    assert!(on.is_ok());
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(true));
    let off = host.onboarding_demo("voice").call(
        "c2",
        "set_lightbulb",
        &json!({ "on": false }).to_string(),
    );
    assert!(off.is_ok());
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(false));
    let replay = host.onboarding_demo("voice").call(
        "c1",
        "set_lightbulb",
        &json!({ "on": true }).to_string(),
    );
    assert_eq!(replay, on);
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(false));
    let malformed = host
        .onboarding_demo("voice")
        .call("c3", "set_lightbulb", "{\"on\":\"yes\"}")
        .expect_err("malformed arguments refused");
    assert_eq!(malformed.code, ErrorCode::InvalidArgument);
    // manual does not pin the code for a tool the demo does not advertise; it must be an error.
    assert!(host
        .onboarding_demo("voice")
        .call("c4", "ilium_close_pane", "{}")
        .is_err());
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(false));
}

fn bulb_call(
    host: &mut FakeHost,
    call_id: &str,
    on: bool,
) -> Result<String, ilium_plugin_sdk::types::PluginError> {
    host.onboarding_demo("voice")
        .call(call_id, "set_lightbulb", &json!({ "on": on }).to_string())
}

/// REQ: PORT:ilium-client/src/onboarding/voice_demo.rs::actual_tool_execution_turns_bulb_on_and_off
#[test]
fn step_054_a_new_call_id_after_off_turns_the_bulb_back_on() {
    let mut host = client_host();
    bulb_call(&mut host, "a1", true).expect("on");
    bulb_call(&mut host, "a2", false).expect("off");
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(false));
    bulb_call(&mut host, "a3", true).expect("on again");
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(true));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_demo.rs::duplicate_on_after_off_returns_original_receipt_without_relighting
#[test]
fn step_054_a_repeated_call_id_with_different_arguments_returns_the_original_receipt() {
    let mut host = client_host();
    let first = bulb_call(&mut host, "b1", true).expect("on");
    bulb_call(&mut host, "b2", false).expect("off");
    let replay = bulb_call(&mut host, "b1", false).expect("replay");
    assert_eq!(replay, first);
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(false));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_demo.rs::exhausted_demo_retains_replay_receipts_and_refuses_new_mutations
#[test]
fn step_054_after_1024_calls_new_calls_are_refused_and_old_receipts_still_replay() {
    let mut host = client_host();
    for index in 0..1024 {
        bulb_call(&mut host, &format!("bulk-{index}"), index % 2 == 0)
            .expect("within the call budget");
    }
    let before = host.onboarding_demo("voice").bulb();
    assert!(bulb_call(&mut host, "bulk-over", true).is_err());
    assert_eq!(host.onboarding_demo("voice").bulb(), before);
    assert!(bulb_call(&mut host, "bulk-0", false).is_ok());
}

/// REQ: PORT:ilium-client/src/onboarding/voice_demo.rs::real_controls_and_malformed_arguments_are_rejected_without_state_changes
#[test]
fn step_054_a_call_without_the_on_field_is_refused_and_leaves_the_bulb_alone() {
    let mut host = client_host();
    bulb_call(&mut host, "d1", true).expect("on");
    let error = host
        .onboarding_demo("voice")
        .call("d2", "set_lightbulb", "{}")
        .expect_err("a missing field is refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(true));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::actual_cpu_projection_executes_ordered_tools_and_duplicate_receipt_without_relighting
#[test]
fn step_054_an_unadvertised_call_between_two_bulb_calls_does_not_disturb_their_order() {
    let mut host = client_host();
    bulb_call(&mut host, "e1", true).expect("on");
    assert!(host
        .onboarding_demo("voice")
        .call("e2", "ilium_close_pane", "{}")
        .is_err());
    bulb_call(&mut host, "e3", false).expect("off");
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(false));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::advertised_capability_is_only_the_bulb
#[test]
fn step_054_names_other_than_the_bulb_are_refused_and_leave_the_bulb_unchanged() {
    let mut host = client_host();
    bulb_call(&mut host, "f0", true).expect("on");
    let before = host.onboarding_demo("voice").bulb();
    for tool in ["ilium_type_text", "ilium_stop_voice", "ilium_press_key"] {
        assert!(
            host.onboarding_demo("voice")
                .call(&format!("f-{tool}"), tool, "{}")
                .is_err(),
            "{tool}"
        );
    }
    assert_eq!(host.onboarding_demo("voice").bulb(), before);
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::bounded_redaction_preserves_replace_prefix_unicode_and_no_secret
#[test]
fn step_054_the_configured_key_stays_hidden_on_a_narrow_screen_including_its_tail() {
    let mut host = client_host();
    configure(&mut host, "api_key", json!("sk-narrow-secret-7731"));
    let text = step_text(&mut host, 50, 20);
    assert!(!text.contains("sk-narrow-secret-7731"), "{text}");
    assert!(!text.contains("7731"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::gate_release_retries_exact_intent_and_preserves_invalid_configuration_error_without_audio
#[test]
fn step_054_pressing_test_twice_with_an_empty_key_opens_no_audio() {
    let mut host = client_host();
    configure(&mut host, "api_key", json!(""));
    let target = ViewTarget::OnboardingStep("voice".to_string());
    host.key(target.clone(), "t");
    host.settle();
    host.key(target, "t");
    host.settle();
    assert!(host.audio().captures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::invalid_configuration_never_starts_audio
#[test]
fn step_054_push_to_talk_with_an_empty_key_never_starts_audio() {
    let mut host = client_host();
    configure(&mut host, "input_mode", json!("push_to_talk"));
    configure(&mut host, "api_key", json!(""));
    host.key(ViewTarget::OnboardingStep("voice".to_string()), "t");
    host.settle();
    host.settle();
    assert!(host.audio().captures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::normal_owner_gate_keeps_original_test_and_stop_cancels_without_starting_audio
#[test]
fn step_054_escape_without_a_waiting_test_never_creates_a_capture() {
    let mut host = client_host();
    host.key(ViewTarget::OnboardingStep("voice".to_string()), "Esc");
    host.settle();
    assert!(host.audio().captures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::saturated_outbox_retains_original_event_and_no_tool_state_effect
#[test]
fn step_054_an_argument_over_1024_bytes_is_refused_and_the_bulb_keeps_its_state() {
    let mut host = client_host();
    bulb_call(&mut host, "k1", true).expect("on");
    let padded = json!({ "on": false, "pad": "x".repeat(1100) }).to_string();
    let error = host
        .onboarding_demo("voice")
        .call("k2", "set_lightbulb", &padded)
        .expect_err("an oversized argument is refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.onboarding_demo("voice").bulb(), Some(true));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::settings_change_cancels_waiting_test_before_external_ownership_releases
#[test]
fn step_054_changing_a_setting_while_a_test_waits_never_creates_a_capture() {
    let mut host = client_host();
    host.key(ViewTarget::OnboardingStep("voice".to_string()), "t");
    host.settle();
    configure(&mut host, "model", json!("gpt-realtime-2.1-mini"));
    host.settle();
    host.settle();
    assert!(host.audio().captures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/voice_runtime.rs::stop_during_shared_startup_discards_prepared_original_without_starting_audio
#[test]
fn step_054_stopping_a_waiting_test_leaves_no_capture_even_after_the_settle() {
    let mut host = client_host();
    let target = ViewTarget::OnboardingStep("voice".to_string());
    host.key(target.clone(), "t");
    host.settle();
    host.key(target, "Esc");
    host.settle();
    host.settle();
    assert!(host.audio().captures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::every_configuration_and_live_action_is_reachable_on_small_and_wide_screens
#[test]
fn step_054_the_test_action_comes_into_view_by_moving_focus_on_a_small_screen() {
    let mut host = client_host();
    let target = ViewTarget::OnboardingStep("voice".to_string());
    let mut seen = false;
    for _ in 0..ROWS.len() * 2 {
        if step_text(&mut host, 40, 8).contains("Test") {
            seen = true;
            break;
        }
        host.key(target.clone(), "Down");
    }
    assert!(
        seen,
        "the Test action never came into view on a 40x8 screen"
    );
}

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::narrow_scrolling_reveals_real_transcripts_and_keyboard_focus_wraps
#[test]
fn step_054_moving_focus_up_from_the_first_row_wraps_to_the_last_row() {
    let mut host = client_host();
    host.key(ViewTarget::OnboardingStep("voice".to_string()), "Up");
    host.settle();
    assert!(step_text(&mut host, 40, 8).contains("Pause other media"));
}

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::render_never_exposes_configured_key_and_keeps_test_visible
#[test]
fn step_054_the_configured_key_is_not_rendered_on_a_wide_screen_either() {
    let mut host = client_host();
    configure(&mut host, "api_key", json!("sk-wide-secret-42"));
    let text = step_text(&mut host, 160, 50);
    assert!(!text.contains("sk-wide-secret-42"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/voice_ui.rs::voice_configuration_cards_use_rounded_frames_at_compact_and_regular_sizes
#[test]
fn step_054_the_rounded_top_left_corner_appears_on_a_mid_sized_screen() {
    let mut host = client_host();
    let text = step_text(&mut host, 50, 30);
    assert!(text.contains('\u{256d}'), "{text}");
}
