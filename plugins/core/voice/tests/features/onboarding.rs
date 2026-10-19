//! F-053: the voice step of the guided setup (onboarding step `voice`), a demonstration with eleven
//! configuration rows and a connection state label. Voice owns only this step.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::ViewTarget;

use crate::common::client_host;

fn step() -> ViewTarget {
    ViewTarget::OnboardingStep("voice".to_string())
}

fn text(host: &mut FakeHost) -> String {
    host.render(step(), 100, 40).text()
}

/// REQ: F-053
#[test]
fn onboarding_053_the_step_lists_eleven_configuration_rows_in_order() {
    let mut host = client_host();
    let rendered = text(&mut host);
    let labels = [
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
    let mut last = 0;
    for label in labels {
        let at = rendered[last..]
            .find(label)
            .unwrap_or_else(|| panic!("row {label} in {rendered}"));
        last += at + label.len();
    }
}

/// REQ: F-053
#[test]
fn onboarding_053_the_defaults_are_off_not_set_and_system_devices() {
    let mut host = client_host();
    let rendered = text(&mut host);
    assert!(rendered.contains("Not set / environment"), "{rendered}");
    assert!(rendered.contains("System default"), "{rendered}");
    assert!(rendered.contains("80%"), "{rendered}");
}

/// REQ: F-053
#[test]
fn onboarding_053_the_idle_connection_label_is_ready_to_test() {
    let mut host = client_host();
    assert!(text(&mut host).contains("Ready to test"));
}

/// REQ: F-053
#[test]
fn onboarding_053_the_model_and_voice_rows_show_the_default_labels() {
    let mut host = client_host();
    let rendered = text(&mut host);
    for (label, expected) in [
        ("Realtime model", "GPT Realtime 2.1"),
        ("Speaking voice", "Marin"),
    ] {
        let row = rendered
            .lines()
            .find(|line| line.contains(label))
            .unwrap_or_else(|| panic!("row {label}"));
        let value = row.rsplit('\u{2026}').next().unwrap_or("");
        let value = value.trim_matches(|c: char| c.is_whitespace() || "<>[]".contains(c));
        assert_eq!(value, expected, "{label}");
    }
}

/// REQ: F-053
#[test]
fn onboarding_053_the_demo_does_not_open_a_capture_before_the_user_starts_the_test() {
    let mut host = client_host();
    let _ = text(&mut host);
    assert!(host.audio().captures().is_empty());
}

/// REQ: F-053
#[test]
fn onboarding_053_the_step_is_optional_and_reopened_with_the_onboarding_flag() {
    let guide = crate::common::user_guide();
    assert!(guide.contains("The first-run guided setup includes an optional voice step with a demonstration; reopen it with `ilium --onboarding`."));
}
