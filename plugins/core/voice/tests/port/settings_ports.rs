//! Twins of the settings and toggle rows (`app.rs`, `mouse.rs`, `value_voice.rs`) on the kernel tab.

use serde_json::json;

use crate::common::{client_host, command_invoked, configure, enabled_client};

/// REQ: PORT:ilium-client/src/app.rs::pause_media_while_active_toggle_persists_and_does_not_reconnect_voice
/// REQ: PORT:ilium-client/src/app.rs::terminal_submission_confirmation_is_opt_in_and_does_not_reconnect_voice
#[test]
fn settings_050_toggles_persist_and_do_not_open_a_new_capture() {
    let mut host = enabled_client("push_to_talk");
    let captures_before = host.audio().captures().len();
    for key in ["pause_media_while_active", "confirm_terminal_submissions"] {
        configure(&mut host, key, json!(true));
        assert_eq!(host.kernel_settings("voice-control").get(key), Some(json!(true)), "{key}");
    }
    assert_eq!(host.audio().captures().len(), captures_before);
    assert_eq!(host.kernel_settings("voice-control").get("enabled"), Some(json!(true)));
}

/// REQ: PORT:ilium-client/src/mouse.rs::bottom_right_voice_control_toggles_before_full_screen_mode_dispatch
#[test]
fn settings_050_the_toggle_command_turns_voice_on() {
    let mut host = client_host();
    host.deliver_one(command_invoked("voice.toggle"));
    host.settle();
    assert_eq!(host.kernel_settings("voice-control").get("enabled"), Some(json!(true)));
}

/// REQ: PORT:ilium-client/src/value_voice.rs::exact_volume_failure_retry_keeps_native_bounds_and_unrelated_voice_settings
#[test]
fn settings_054_volume_above_100_is_refused_and_other_settings_stay() {
    let mut host = client_host();
    configure(&mut host, "output_volume_percent", json!(73));
    let refused = host.kernel_settings("voice-control").set("output_volume_percent", json!(101));
    assert!(refused.is_err());
    assert_eq!(host.kernel_settings("voice-control").get("output_volume_percent"), Some(json!(73)));
}
