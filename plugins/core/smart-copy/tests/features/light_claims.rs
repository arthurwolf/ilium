//! F-065: Smart Copy light starts from a modifier held over a terminal pane. The client holds a
//! gesture claim (`claim-pointer-gesture`) with the modifier of `terminal.smart_copy_light_key`
//! on every terminal pane while `terminal.smart_copy_light` is on (smart-copy.md "Smart Copy light",
//! primitives-terminal-pointer.md "Claims, gestures and drag-over").

use ilium_plugin_core_smart_copy::SmartCopyClient;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Event, KernelSettingsEvent, PointerModifiers, PrimitiveKind};
use serde_json::json;

use crate::common::{
    add_editor_pane, add_terminal_pane, client_host, client_host_unactivated, pane_created,
};

fn gesture_claims(host: &mut FakeHost, pane: u64) -> Vec<PointerModifiers> {
    host.terminal()
        .pointer_claims(pane)
        .into_iter()
        .flatten()
        .collect()
}

fn light_setting_changed(key: &str) -> Event {
    Event::KernelSettingsChanged(KernelSettingsEvent {
        tab: "terminal".to_string(),
        keys: vec![key.to_string()],
    })
}

fn new_terminal(host: &mut FakeHost) -> u64 {
    let pane = add_terminal_pane(host, "api");
    host.deliver_one(pane_created(
        pane,
        "ilium.terminal",
        PrimitiveKind::Terminal,
    ));
    pane
}

/// REQ: F-065
#[test]
fn feature_065_default_key_claims_a_control_gesture() {
    let mut host = client_host();
    let pane = new_terminal(&mut host);
    assert_eq!(gesture_claims(&mut host, pane), [PointerModifiers::CONTROL]);
}

/// REQ: F-065
#[test]
fn feature_065_alt_key_claims_an_alt_gesture() {
    let mut host = client_host_unactivated();
    host.settings()
        .set("terminal.smart_copy_light_key", json!("alt"))
        .expect("allowed");
    host.activate::<SmartCopyClient>();
    let pane = new_terminal(&mut host);
    assert_eq!(gesture_claims(&mut host, pane), [PointerModifiers::ALT]);
}

/// REQ: F-065
#[test]
fn feature_065_shift_key_claims_a_shift_gesture() {
    let mut host = client_host_unactivated();
    host.settings()
        .set("terminal.smart_copy_light_key", json!("shift"))
        .expect("allowed");
    host.activate::<SmartCopyClient>();
    let pane = new_terminal(&mut host);
    assert_eq!(gesture_claims(&mut host, pane), [PointerModifiers::SHIFT]);
}

/// REQ: F-065
#[test]
fn feature_065_light_off_holds_no_gesture_claim() {
    let mut host = client_host_unactivated();
    host.settings()
        .set("terminal.smart_copy_light", json!(false))
        .expect("boolean");
    host.activate::<SmartCopyClient>();
    let pane = new_terminal(&mut host);
    assert!(gesture_claims(&mut host, pane).is_empty());
}

/// REQ: F-065
#[test]
fn feature_065_changing_the_key_moves_the_claim_to_the_new_modifier() {
    let mut host = client_host();
    let pane = new_terminal(&mut host);
    host.settings()
        .set("terminal.smart_copy_light_key", json!("alt"))
        .expect("allowed");
    host.deliver_one(light_setting_changed("smart_copy_light_key"));
    assert_eq!(gesture_claims(&mut host, pane), [PointerModifiers::ALT]);
}

/// REQ: F-065
#[test]
fn feature_065_switching_light_off_releases_the_claim_and_on_restores_it() {
    let mut host = client_host();
    let pane = new_terminal(&mut host);
    host.settings()
        .set("terminal.smart_copy_light", json!(false))
        .expect("boolean");
    host.deliver_one(light_setting_changed("smart_copy_light"));
    assert!(gesture_claims(&mut host, pane).is_empty());
    host.settings()
        .set("terminal.smart_copy_light", json!(true))
        .expect("boolean");
    host.deliver_one(light_setting_changed("smart_copy_light"));
    assert_eq!(gesture_claims(&mut host, pane), [PointerModifiers::CONTROL]);
}

/// REQ: F-065
#[test]
fn feature_065_the_light_variant_is_independent_of_terminal_text_selection() {
    // smart-copy.md Settings: "Terminal text selection ... separate from Smart Copy".
    let mut host = client_host_unactivated();
    host.settings()
        .set("ui.terminal_text_selection_enabled", json!(false))
        .expect("boolean");
    host.activate::<SmartCopyClient>();
    let pane = new_terminal(&mut host);
    assert_eq!(gesture_claims(&mut host, pane), [PointerModifiers::CONTROL]);
}

/// REQ: F-065
#[test]
fn feature_065_editors_and_boards_are_not_affected() {
    let mut host = client_host();
    let pane = add_editor_pane(&mut host, "notes");
    host.deliver_one(pane_created(pane, "ilium.editor", PrimitiveKind::Editor));
    assert!(gesture_claims(&mut host, pane).is_empty());
}

/// REQ: F-065
#[test]
fn feature_065_each_terminal_pane_gets_its_own_claim() {
    let mut host = client_host();
    let first = new_terminal(&mut host);
    let second = new_terminal(&mut host);
    assert_ne!(first, second);
    assert_eq!(
        gesture_claims(&mut host, first),
        [PointerModifiers::CONTROL]
    );
    assert_eq!(
        gesture_claims(&mut host, second),
        [PointerModifiers::CONTROL]
    );
}

/// REQ: F-065
#[test]
fn feature_065_repeated_settings_events_do_not_duplicate_the_claim() {
    let mut host = client_host();
    let pane = new_terminal(&mut host);
    for _ in 0..3 {
        host.deliver_one(light_setting_changed("smart_copy_light_key"));
    }
    assert_eq!(gesture_claims(&mut host, pane), [PointerModifiers::CONTROL]);
}

/// REQ: F-065
#[test]
fn feature_065_a_wheel_event_with_the_modifier_does_not_start_light() {
    // smart-copy.md Rules: "A wheel event with the modifier keeps its normal meaning".
    let mut host = client_host();
    let pane = new_terminal(&mut host);
    let wheel = Event::Pointer(ilium_plugin_sdk::types::PointerEvent {
        node: ilium_plugin_sdk::types::NodeId(pane),
        row: 2,
        column: 4,
        kind: ilium_plugin_sdk::types::PointerKind::WheelUp,
        button: ilium_plugin_sdk::types::PointerButton::None,
        modifiers: PointerModifiers::CONTROL,
        sub_x: 0,
        sub_y: 0,
    });
    let submissions = host.deliver_one(wheel);
    assert!(!submissions.iter().any(|submission| matches!(
        submission.intent,
        ilium_plugin_sdk::types::Intent::FreezePane(_)
    )));
}
