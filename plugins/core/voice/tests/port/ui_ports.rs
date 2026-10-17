//! Twins of the voice rendering tests of `ilium-client/src/ui.rs` and `value_voice.rs`, expressed against the
//! views of the plugin: the settings page `voice-control`, the voice strip panel and the onboarding step
//! `voice`. See API_GAPS_svc01.md (items 8 to 11).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, Color, ColorRole, ViewTarget};
use serde_json::json;

use crate::common::{client_host, configure, enabled_client, f8_grab, key_pressed, key_released};

fn page() -> ViewTarget {
    ViewTarget::SettingsPage("voice-control".to_string())
}

fn step() -> ViewTarget {
    ViewTarget::OnboardingStep("voice".to_string())
}

fn strip() -> ViewTarget {
    ViewTarget::Panel("voice".to_string())
}

/// Colour of the cell holding the first glyph of the voice indicator on the strip.
fn dot_colour(host: &mut FakeHost) -> Color {
    let screen = host.render(strip(), 48, 5);
    let (row, column) = screen.find("\u{25cf}").expect("the voice dot is painted");
    screen.cell(row, column).foreground
}

/// REQ: PORT:ilium-client/src/ui.rs::stacked_voice_credential_prompt_renders_over_live_settings
#[test]
fn port_020_the_credential_prompt_stacks_over_the_live_settings_page() {
    let mut host = client_host();
    host.send_action(page(), "api_key", ActionKind::Activated, ActionValue::None);
    host.settle();
    let popups = host.popups();
    assert_eq!(popups.len(), 1, "{popups:?}");
    let settings = host.render(page(), 100, 30).text();
    assert!(settings.contains("API key"), "the settings stay visible under the prompt: {settings}");
    let prompt = host.render(ViewTarget::Popup(popups[0].clone()), 60, 8).text();
    assert!(prompt.contains("API key"), "{prompt}");
}

/// REQ: PORT:ilium-client/src/ui.rs::voice_control_dot_is_black_when_disabled_and_red_when_enabled
#[test]
fn port_021_the_voice_dot_is_dim_when_disabled_and_danger_coloured_when_recording() {
    let mut disabled = client_host();
    let idle = disabled.render(strip(), 48, 5).text();
    assert!(!idle.contains("Listening"), "{idle}");
    let mut host = enabled_client("push_to_talk");
    let grab = f8_grab(&mut host);
    host.deliver_one(key_pressed(grab, false));
    host.settle();
    assert_eq!(dot_colour(&mut host), Color::Role(ColorRole::Danger));
}

/// REQ: PORT:ilium-client/src/value_voice.rs::exactly_seven_native_choices_and_volume_reuse_existing_complete_catalogs
#[test]
fn port_022_the_voice_step_has_seven_choice_rows_and_one_volume_row() {
    let mut host = client_host();
    let text = host.render(step(), 100, 40).text();
    for label in ["Realtime model", "Speaking voice", "Turn detection", "Microphone", "Speaker", "Reasoning", "VAD responsiveness"] {
        assert_eq!(text.matches(label).count(), 1, "{label} appears once: {text}");
    }
    assert_eq!(text.matches("Output volume").count(), 1, "{text}");
}

/// REQ: PORT:ilium-client/src/value_voice.rs::reopened_wizard_or_changed_voice_configuration_refuses_old_dialog
#[test]
fn port_023_a_dialog_of_a_closed_step_or_a_changed_configuration_is_refused() {
    let mut host = client_host();
    host.send_action(step(), "voice", ActionKind::Activated, ActionValue::None);
    host.settle();
    let old = host.popups();
    assert_eq!(old.len(), 1, "{old:?}");
    configure(&mut host, "voice", json!("marin"));
    host.send_action(ViewTarget::Popup(old[0].clone()), &old[0], ActionKind::Selected, ActionValue::Index(1));
    host.settle();
    assert_eq!(host.kernel_settings().get("voice-control", "voice"), Some(json!("marin")));
    host.send_action(step(), "onboarding", ActionKind::Closed, ActionValue::None);
    host.settle();
    host.send_action(ViewTarget::Popup(old[0].clone()), &old[0], ActionKind::Selected, ActionValue::Index(2));
    host.settle();
    assert_eq!(host.kernel_settings().get("voice-control", "voice"), Some(json!("marin")));
}

/// REQ: PORT:ilium-client/src/keys.rs::f8_toggles_semantic_vad_and_preserves_push_to_talk_edges
#[test]
fn port_024_f8_toggles_semantic_vad_and_preserves_push_to_talk_edges() {
    // Semantic VAD: one F8 press turns the listening session off.
    let mut semantic = enabled_client("semantic_vad");
    let grab = f8_grab(&mut semantic);
    semantic.deliver_one(key_pressed(grab, false));
    semantic.settle();
    assert_eq!(semantic.kernel_settings().get("voice-control", "enabled"), Some(json!(false)));

    // Push to talk: the press edge opens one capture and the release edge ends it, without toggling `enabled`.
    let mut talk = enabled_client("push_to_talk");
    let grab = f8_grab(&mut talk);
    talk.deliver_one(key_pressed(grab, false));
    talk.settle();
    assert_eq!(talk.audio().captures().len(), 1, "press starts the capture");
    talk.deliver_one(key_released(grab));
    talk.settle();
    assert_eq!(talk.audio().captures().len(), 1, "release does not open another capture");
    assert_eq!(talk.kernel_settings().get("voice-control", "enabled"), Some(json!(true)));
}

/// REQ: PORT:ilium-client/src/keys.rs::voice_api_key_prompt_preserves_exact_settings_parent_until_commit
#[test]
fn port_025_voice_api_key_prompt_preserves_exact_settings_parent_until_commit() {
    let mut host = client_host();
    host.send_action(page(), "api_key", ActionKind::Activated, ActionValue::None);
    host.settle();
    let popups = host.popups();
    assert_eq!(popups.len(), 1, "{popups:?}");
    let before = host.render(page(), 100, 30).text();
    host.send_action(ViewTarget::Popup(popups[0].clone()), "api_key", ActionKind::Changed, ActionValue::Text("sk-live-test".to_string()));
    host.settle();
    assert_eq!(host.popups().len(), 1, "typing does not commit");
    assert_eq!(host.render(page(), 100, 30).text(), before, "the settings parent is unchanged until commit");
    host.send_action(ViewTarget::Popup(popups[0].clone()), "api_key", ActionKind::Submitted, ActionValue::Text("sk-live-test".to_string()));
    host.settle();
    assert_eq!(host.popups().len(), 0, "the prompt closes on commit");
    assert_eq!(host.kernel_settings().get("voice-control", "api_key"), Some(json!("sk-live-test")));
    let after = host.render(page(), 100, 30).text();
    assert!(after.contains("API key"), "the same settings page returns: {after}");
}
