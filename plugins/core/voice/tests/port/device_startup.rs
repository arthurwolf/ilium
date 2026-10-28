//! Twins of the native audio start-up tests of `ilium-client/src/startup_audio.rs`. The monolith audited the
//! byte budget and admission of its native discovery work; a plugin sees only the outcome: the device list the
//! settings page shows, the device name a capture is opened with, and the refusals of the audio import.
//! See API_GAPS_svc01.md (items 4 to 7).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, PluginError, ViewTarget};
use serde_json::json;

use crate::common::{client_host, configure, enabled_client, f8_grab, key_pressed};

fn page() -> ViewTarget {
    ViewTarget::SettingsPage("voice-control".to_string())
}

fn press_f8(host: &mut FakeHost) {
    let grab = f8_grab(host);
    host.deliver_one(key_pressed(grab, false));
    host.settle();
}

fn open_device_rows(host: &mut FakeHost) -> String {
    host.send_action(
        page(),
        "input_device_name",
        ActionKind::Activated,
        ActionValue::None,
    );
    host.settle();
    host.render(page(), 100, 30).text()
}

/// REQ: PORT:ilium-client/src/startup_audio.rs::admission_refusal_does_not_enter_native_discovery
#[test]
fn port_014_a_refused_discovery_lists_only_the_system_default_and_opens_no_capture() {
    let mut host = client_host();
    host.audio()
        .set_input_devices(vec!["Built-in Microphone".to_string()]);
    host.inject_failure(
        "audio.devices",
        PluginError {
            code: ErrorCode::Busy,
            message: "admission refused".to_string(),
            retry_after_ms: Some(10),
        },
    );
    let text = open_device_rows(&mut host);
    assert!(text.contains("System default"), "{text}");
    assert!(!text.contains("Built-in Microphone"), "{text}");
    assert_eq!(host.audio().captures().len(), 0);
}

/// REQ: PORT:ilium-client/src/startup_audio.rs::byte_audit_includes_spare_device_name_storage
#[test]
fn port_015_a_long_saved_device_name_reaches_the_capture_unchanged() {
    let name = format!("USB Microphone {}", "x".repeat(200));
    let mut host = client_host();
    configure(&mut host, "input_mode", json!("push_to_talk"));
    configure(&mut host, "input_device_name", json!(name));
    configure(&mut host, "enabled", json!(true));
    press_f8(&mut host);
    assert_eq!(
        host.audio().captures()[0].device.as_deref(),
        Some(name.as_str())
    );
    assert_eq!(
        host.kernel_settings()
            .get("voice-control", "input_device_name"),
        Some(json!(name))
    );
}

/// REQ: PORT:ilium-client/src/startup_audio.rs::cancelling_observation_keeps_running_native_work_admitted
#[test]
fn port_016_closing_the_device_list_does_not_lose_the_pending_choice() {
    let mut host = client_host();
    host.audio().set_input_devices(vec![
        "Built-in Microphone".to_string(),
        "USB Microphone".to_string(),
    ]);
    open_device_rows(&mut host);
    host.send_action(
        page(),
        "input_device_name",
        ActionKind::Closed,
        ActionValue::None,
    );
    host.settle();
    host.send_action(
        page(),
        "input_device_name",
        ActionKind::Selected,
        ActionValue::Index(2),
    );
    host.settle();
    assert_eq!(
        host.kernel_settings()
            .get("voice-control", "input_device_name"),
        Some(json!("USB Microphone"))
    );
}

/// REQ: PORT:ilium-client/src/startup_audio.rs::native_io_catalogue_moves_originals_with_their_storage_charge
#[test]
fn port_017_a_new_catalogue_leaves_the_saved_original_name_intact() {
    let mut host = client_host();
    host.audio().set_input_devices(vec![
        "Built-in Microphone".to_string(),
        "USB Microphone".to_string(),
    ]);
    host.send_action(
        page(),
        "input_device_name",
        ActionKind::Selected,
        ActionValue::Index(2),
    );
    host.settle();
    host.audio().set_input_devices(vec!["Headset".to_string()]);
    assert_eq!(
        host.kernel_settings()
            .get("voice-control", "input_device_name"),
        Some(json!("USB Microphone"))
    );
    let mut enabled = enabled_client("push_to_talk");
    enabled
        .audio()
        .set_input_devices(vec!["Headset".to_string()]);
    press_f8(&mut enabled);
    assert_eq!(enabled.audio().captures()[0].device, None);
}

/// REQ: PORT:ilium-client/src/startup_audio.rs::admission_refusal_does_not_enter_native_discovery
#[test]
fn port_014_a_refused_discovery_leaves_the_saved_device_choice_unchanged() {
    let mut host = client_host();
    configure(&mut host, "input_device_name", json!("Built-in Microphone"));
    host.audio()
        .set_input_devices(vec!["Built-in Microphone".to_string()]);
    host.inject_failure(
        "audio.devices",
        PluginError {
            code: ErrorCode::Busy,
            message: "admission refused".to_string(),
            retry_after_ms: Some(10),
        },
    );
    open_device_rows(&mut host);
    assert_eq!(
        host.kernel_settings()
            .get("voice-control", "input_device_name"),
        Some(json!("Built-in Microphone"))
    );
}

/// REQ: PORT:ilium-client/src/startup_audio.rs::byte_audit_includes_spare_device_name_storage
#[test]
fn port_015_a_multibyte_saved_device_name_reaches_the_capture_unchanged() {
    let name = "Mikrofon Müller 麦克风 🎙 USB".to_string();
    let mut host = client_host();
    configure(&mut host, "input_mode", json!("push_to_talk"));
    configure(&mut host, "input_device_name", json!(name));
    configure(&mut host, "enabled", json!(true));
    press_f8(&mut host);
    assert_eq!(
        host.audio().captures()[0].device.as_deref(),
        Some(name.as_str())
    );
}

/// REQ: PORT:ilium-client/src/startup_audio.rs::cancelling_observation_keeps_running_native_work_admitted
#[test]
fn port_016_closing_the_device_list_without_a_choice_keeps_the_saved_device() {
    let mut host = client_host();
    configure(&mut host, "input_device_name", json!("Built-in Microphone"));
    host.audio().set_input_devices(vec![
        "Built-in Microphone".to_string(),
        "USB Microphone".to_string(),
    ]);
    open_device_rows(&mut host);
    host.send_action(
        page(),
        "input_device_name",
        ActionKind::Closed,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(
        host.kernel_settings()
            .get("voice-control", "input_device_name"),
        Some(json!("Built-in Microphone"))
    );
}

/// REQ: PORT:ilium-client/src/startup_audio.rs::native_io_catalogue_moves_originals_with_their_storage_charge
#[test]
fn port_017_a_saved_device_missing_from_the_new_catalogue_is_still_used_for_capture() {
    let mut host = enabled_client("push_to_talk");
    configure(&mut host, "input_device_name", json!("USB Microphone"));
    host.audio().set_input_devices(vec!["Headset".to_string()]);
    press_f8(&mut host);
    assert_eq!(
        host.audio().captures()[0].device.as_deref(),
        Some("USB Microphone")
    );
}
