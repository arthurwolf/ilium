//! The Sound tab: defaults, ranges, accepted values and the config keys that persist
//! (src/docs/notifications.md "Configuration reference"; page "Kernel tabs this plugin owns").

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::{Value, json};

use crate::common::*;

fn default_of(key: &str) -> Option<Value> {
    server_host().settings().get(key)
}

/// REQ: F-048
#[test]
fn settings_048_notifications_defaults_are_the_documented_ones() {
    let expected = [
        ("notifications.enabled", json!(true)),
        ("notifications.agent_finished", json!(true)),
        ("notifications.approval_required", json!(true)),
        ("notifications.task_succeeded", json!(false)),
        ("notifications.task_failed", json!(true)),
        ("notifications.suppress_redundant_task_outcomes", json!(true)),
        ("notifications.task_coalesce_seconds", json!(30)),
    ];
    for (key, value) in expected {
        assert_eq!(default_of(key), Some(value), "{key}");
    }
}

/// REQ: F-047
#[test]
fn settings_047_sound_defaults_are_the_documented_ones() {
    let expected = [
        ("sound.source", json!("system_beep")),
        ("sound.events.agent_finished", json!(true)),
        ("sound.events.approval_required", json!(false)),
        ("sound.events.agent_started", json!(false)),
        ("sound.events.waiting_background", json!(false)),
        ("sound.events.task_succeeded", json!(false)),
        ("sound.events.task_failed", json!(true)),
        ("sound.design.waveform", json!("triangle")),
    ];
    for (key, value) in expected {
        assert_eq!(default_of(key), Some(value), "{key}");
    }
}

/// REQ: F-047
#[test]
fn settings_047_every_documented_source_value_is_accepted() {
    for source in ["system_beep", "sound_file", "bundled_chirping", "generated", "muted"] {
        let mut host = server_host();
        host.settings().set("sound.source", json!(source)).expect(source);
        assert_eq!(host.settings().get("sound.source"), Some(json!(source)));
    }
}

/// REQ: F-047
#[test]
fn settings_047_an_unknown_source_is_refused_with_invalid_argument() {
    let mut host = server_host();
    let error = host.settings().set("sound.source", json!("loudspeaker")).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.settings().get("sound.source"), Some(json!("system_beep")));
}

/// REQ: F-047
#[test]
fn settings_047_every_documented_waveform_is_accepted() {
    for waveform in ["sine", "triangle", "saw", "square"] {
        let mut host = server_host();
        host.settings().set("sound.design.waveform", json!(waveform)).expect(waveform);
    }
}

/// REQ: F-047
#[test]
fn settings_047_an_unknown_waveform_is_refused_with_invalid_argument() {
    let mut host = server_host();
    let error = host.settings().set("sound.design.waveform", json!("noise")).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-047
#[test]
fn settings_047_design_values_are_clamped_to_their_ranges() {
    let cases: [(&str, i64, i64); 10] = [
        ("sound.design.pitch_hz", 59, 60),
        ("sound.design.pitch_hz", 1801, 1800),
        ("sound.design.pitch_slide_cents", -1801, -1800),
        ("sound.design.pitch_slide_cents", 1801, 1800),
        ("sound.design.duration_ms", 79, 80),
        ("sound.design.duration_ms", 3001, 3000),
        ("sound.design.volume_percent", -1, 0),
        ("sound.design.volume_percent", 101, 100),
        ("sound.design.pitch_hz", 60, 60),
        ("sound.design.volume_percent", 100, 100),
    ];
    for (key, given, stored) in cases {
        let mut host = server_host();
        host.settings().set(key, json!(given)).unwrap_or_else(|error| panic!("{key}={given}: {error:?}"));
        assert_eq!(host.settings().get(key), Some(json!(stored)), "{key}={given}");
    }
}

/// REQ: F-049
#[test]
fn settings_049_the_merge_window_is_clamped_to_zero_to_600() {
    let cases: [(i64, i64); 6] = [(-1, 0), (0, 0), (30, 30), (600, 600), (601, 600), (100_000, 600)];
    for (given, stored) in cases {
        let mut host = server_host();
        host.settings()
            .set("notifications.task_coalesce_seconds", json!(given))
            .unwrap_or_else(|error| panic!("{given}: {error:?}"));
        assert_eq!(host.settings().get("notifications.task_coalesce_seconds"), Some(json!(stored)), "{given}");
    }
}

/// REQ: F-048
#[test]
fn settings_048_a_switch_refuses_a_text_value_with_invalid_argument() {
    let mut host = server_host();
    let error = host.settings().set("notifications.enabled", json!("yes")).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.settings().get("notifications.enabled"), Some(json!(true)));
}

/// REQ: F-049
#[test]
fn settings_049_the_merge_window_refuses_a_text_value_with_invalid_argument() {
    let mut host = server_host();
    let error = host.settings().set("notifications.task_coalesce_seconds", json!("soon")).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-047
#[test]
fn settings_047_a_changed_value_reads_back() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.agent_started", json!(true));
    assert_eq!(host.settings().get("sound.events.agent_started"), Some(json!(true)));
}

/// REQ: F-047
#[test]
fn settings_047_a_sound_file_path_starting_with_a_dash_is_kept_as_data() {
    let mut host = server_host();
    host.settings().set("sound.file", json!("-n /tmp/evil.oga")).expect("kept as data");
    assert_eq!(host.settings().get("sound.file"), Some(json!("-n /tmp/evil.oga")));
}

/// REQ: F-047
#[test]
fn settings_047_the_setting_changes_apply_without_a_restart() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(ilium_plugin_sdk::types::AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)).len(), 1);
    set_setting(&mut host, "sound.events.agent_finished", json!(false));
    assert!(sounds(&finish_turn(&mut host, node, 2)).is_empty());
    set_setting(&mut host, "sound.events.agent_finished", json!(true));
    assert_eq!(sounds(&finish_turn(&mut host, node, 3)).len(), 1);
}

/// REQ: F-047
#[test]
fn settings_047_a_change_of_another_tab_does_not_reload_the_sound_settings() {
    let mut host = server_host();
    host.settings().set("sound.events.agent_finished", json!(false)).expect("stored");
    host.deliver_one(ilium_plugin_sdk::types::Event::KernelSettingsChanged(
        ilium_plugin_sdk::types::KernelSettingsEvent { tab: "editor".to_string(), keys: vec!["autosave".to_string()] },
    ));
    let node = add_pane(&mut host, "claude-api", Some(ilium_plugin_sdk::types::AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)).len(), 1, "the unannounced change is not applied");
}
