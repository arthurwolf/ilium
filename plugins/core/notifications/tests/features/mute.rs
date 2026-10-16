//! Page sample 5, mute sound with a toggle: `ui-action` `toggled` with a flag on the key
//! `sound-mute` is stored under `muted` and play-sound is skipped while it is set; the flag is kept
//! across restarts through the plugin's storage.

use ilium_plugin_core_notifications::NotificationsServer;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::host::Bucket;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, AgentActivity, DeactivationReason, Event, UiAction, ViewTarget};

use crate::common::*;

fn toggle(host: &mut FakeHost, key: &str, kind: ActionKind, value: Option<ActionValue>) {
    host.deliver_one(Event::UiAction(UiAction {
        view: ViewTarget::Popup("notifications".to_string()),
        key: key.to_string(),
        action_id: String::new(),
        kind,
        value,
    }));
    host.settle();
}

fn mute(host: &mut FakeHost, muted: bool) {
    toggle(host, "sound-mute", ActionKind::Toggled, Some(ActionValue::Flag(muted)));
}

/// REQ: F-047
#[test]
fn mute_047_a_muted_plugin_skips_the_finished_sound() {
    let mut host = server_host();
    mute(&mut host, true);
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert!(sounds(&finish_turn(&mut host, node, 1)).is_empty());
}

/// REQ: F-047
#[test]
fn mute_047_a_muted_plugin_still_raises_the_desktop_notice() {
    let mut host = server_host();
    mute(&mut host, true);
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(notices(&finish_turn(&mut host, node, 1)).len(), 1);
}

/// REQ: F-047
#[test]
fn mute_047_unmuting_plays_sounds_again() {
    let mut host = server_host();
    mute(&mut host, true);
    mute(&mut host, false);
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)), ["finished"]);
}

/// REQ: F-047
#[test]
fn mute_047_the_flag_is_stored_as_the_byte_one_under_muted() {
    let mut host = server_host();
    mute(&mut host, true);
    assert_eq!(host.storage().get(Bucket::Global, "muted"), Some(b"1".to_vec()));
}

/// REQ: F-047
#[test]
fn mute_047_clearing_the_flag_stores_the_byte_zero() {
    let mut host = server_host();
    mute(&mut host, true);
    mute(&mut host, false);
    assert_eq!(host.storage().get(Bucket::Global, "muted"), Some(b"0".to_vec()));
}

/// REQ: F-047
#[test]
fn mute_047_the_flag_survives_a_restart_of_the_plugin() {
    let mut host = server_host();
    mute(&mut host, true);
    host.deactivate(DeactivationReason::Shutdown);
    host.activate::<NotificationsServer>();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert!(sounds(&finish_turn(&mut host, node, 1)).is_empty());
}

/// REQ: F-047
#[test]
fn mute_047_a_toggle_on_another_key_does_not_mute() {
    let mut host = server_host();
    toggle(&mut host, "something-else", ActionKind::Toggled, Some(ActionValue::Flag(true)));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)), ["finished"]);
}

/// REQ: F-047
#[test]
fn mute_047_an_activation_of_the_mute_key_is_not_a_toggle() {
    let mut host = server_host();
    toggle(&mut host, "sound-mute", ActionKind::Activated, Some(ActionValue::Flag(true)));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)), ["finished"]);
}

/// REQ: F-047
#[test]
fn mute_047_a_toggle_without_a_flag_value_is_ignored() {
    let mut host = server_host();
    toggle(&mut host, "sound-mute", ActionKind::Toggled, Some(ActionValue::Text("on".to_string())));
    toggle(&mut host, "sound-mute", ActionKind::Toggled, None);
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(sounds(&finish_turn(&mut host, node, 1)), ["finished"]);
}

/// REQ: F-047
#[test]
fn mute_047_a_toggle_submits_nothing_itself() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::UiAction(UiAction {
        view: ViewTarget::Popup("notifications".to_string()),
        key: "sound-mute".to_string(),
        action_id: String::new(),
        kind: ActionKind::Toggled,
        value: Some(ActionValue::Flag(true)),
    }));
    assert!(submissions.is_empty());
}

/// REQ: F-047
#[test]
fn mute_047_a_muted_plugin_still_marks_attention() {
    let mut host = server_host();
    mute(&mut host, true);
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = transition(&mut host, node, AgentActivity::Working, AgentActivity::AwaitingApproval, 1);
    assert_eq!(marks(&submissions).len(), 1);
}
