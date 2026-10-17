//! F-054: device selection and reconnect. The input device name goes to `audio.open_capture`; `None` is the
//! system default; an unplugged device is recovered with Reconnect now.

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};
use serde_json::json;

use crate::common::{client_host, configure, enabled_client, f8_grab, key_pressed};

fn page() -> ViewTarget {
    ViewTarget::SettingsPage("voice-control".to_string())
}

fn press_f8(host: &mut ilium_plugin_sdk::testing::FakeHost) {
    let grab = f8_grab(host);
    host.deliver_one(key_pressed(grab, false));
    host.settle();
}

/// REQ: F-054
#[test]
fn device_054_the_system_default_opens_the_capture_without_a_device_name() {
    let mut host = enabled_client("push_to_talk");
    press_f8(&mut host);
    assert_eq!(host.audio().captures()[0].device, None);
}

/// REQ: F-054
#[test]
fn device_054_a_saved_input_device_name_is_passed_to_the_capture() {
    let mut host = client_host();
    configure(&mut host, "input_mode", json!("push_to_talk"));
    configure(&mut host, "input_device_name", json!("USB Microphone"));
    configure(&mut host, "enabled", json!(true));
    press_f8(&mut host);
    assert_eq!(
        host.audio().captures()[0].device.as_deref(),
        Some("USB Microphone")
    );
}

/// REQ: F-054
#[test]
fn device_054_choosing_a_discovered_microphone_saves_its_name() {
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
    assert_eq!(
        host.kernel_settings()
            .get("voice-control", "input_device_name"),
        Some(json!("USB Microphone"))
    );
}

/// REQ: F-054
#[test]
fn device_054_index_zero_is_the_system_default_and_removes_the_saved_name() {
    let mut host = client_host();
    host.audio()
        .set_input_devices(vec!["USB Microphone".to_string()]);
    host.send_action(
        page(),
        "input_device_name",
        ActionKind::Selected,
        ActionValue::Index(1),
    );
    host.send_action(
        page(),
        "input_device_name",
        ActionKind::Selected,
        ActionValue::Index(0),
    );
    host.settle();
    assert_eq!(
        host.kernel_settings()
            .get("voice-control", "input_device_name"),
        None
    );
}

/// REQ: F-054
#[test]
fn device_054_choosing_a_discovered_speaker_saves_its_name() {
    let mut host = client_host();
    host.audio().set_output_devices(vec!["Headset".to_string()]);
    host.send_action(
        page(),
        "output_device_name",
        ActionKind::Selected,
        ActionValue::Index(1),
    );
    host.settle();
    assert_eq!(
        host.kernel_settings()
            .get("voice-control", "output_device_name"),
        Some(json!("Headset"))
    );
}

/// REQ: F-054
#[test]
fn device_054_the_device_rows_list_the_discovered_names() {
    let mut host = client_host();
    host.audio()
        .set_input_devices(vec!["Built-in Microphone".to_string()]);
    host.send_action(
        page(),
        "input_device_name",
        ActionKind::Activated,
        ActionValue::None,
    );
    host.settle();
    let text = host.render(page(), 100, 30).text();
    assert!(
        text.contains("System default") && text.contains("Built-in Microphone"),
        "{text}"
    );
}

/// REQ: F-054
#[test]
fn device_054_changing_the_input_device_while_listening_reopens_the_capture_on_it() {
    let mut host = enabled_client("semantic_vad");
    configure(&mut host, "input_device_name", json!("USB Microphone"));
    let last = host.audio().captures().last().cloned().expect("capture");
    assert!(last.open);
    assert_eq!(last.device.as_deref(), Some("USB Microphone"));
}

/// REQ: F-054
#[test]
fn device_054_an_unplugged_device_leaves_no_open_capture() {
    let mut host = client_host();
    host.audio()
        .set_input_devices(vec!["USB Microphone".to_string()]);
    configure(&mut host, "input_device_name", json!("USB Microphone"));
    configure(&mut host, "enabled", json!(true));
    host.audio().unplug("USB Microphone");
    configure(&mut host, "output_volume_percent", json!(70));
    assert!(host.audio().captures().iter().all(|capture| !capture.open));
}

/// REQ: F-054
#[test]
fn device_054_reconnect_now_recovers_when_the_device_returns() {
    let mut host = client_host();
    host.audio()
        .set_input_devices(vec!["USB Microphone".to_string()]);
    configure(&mut host, "input_device_name", json!("USB Microphone"));
    configure(&mut host, "enabled", json!(true));
    host.audio().unplug("USB Microphone");
    configure(&mut host, "output_volume_percent", json!(70));
    host.audio().plug("USB Microphone");
    host.send_action(
        page(),
        "reconnect",
        ActionKind::Activated,
        ActionValue::None,
    );
    host.settle();
    let last = host.audio().captures().last().cloned().expect("capture");
    assert!(last.open);
    assert_eq!(last.device.as_deref(), Some("USB Microphone"));
}

/// REQ: F-054
#[test]
fn device_054_reconnect_now_with_voice_off_opens_nothing() {
    let mut host = client_host();
    host.send_action(
        page(),
        "reconnect",
        ActionKind::Activated,
        ActionValue::None,
    );
    host.settle();
    assert!(host.audio().captures().is_empty());
}

/// REQ: F-054
#[test]
fn device_054_the_capture_format_does_not_depend_on_the_device() {
    let mut host = client_host();
    configure(&mut host, "input_mode", json!("push_to_talk"));
    configure(&mut host, "input_device_name", json!("Headset"));
    configure(&mut host, "enabled", json!(true));
    press_f8(&mut host);
    let capture = host.audio().captures()[0].clone();
    assert_eq!((capture.sample_rate_hz, capture.channels), (16_000, 1));
}

/// REQ: F-054
#[test]
fn device_054_the_audio_devices_section_of_the_guide_names_the_three_platform_backends() {
    let guide = crate::common::user_guide();
    assert!(guide.contains("Linux uses ALSA, Windows uses WASAPI and macOS uses CoreAudio."));
    assert!(guide.contains("Press **Reconnect now** after plugging in or changing a device."));
}
