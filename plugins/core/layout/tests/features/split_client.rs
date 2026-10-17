//! Client side of "Split four ways": the command is forwarded to the server side with the focused pane.

use ilium_plugin_sdk::types::PluginSide;
use proptest::prelude::*;

use crate::common::*;

fn forwarded(command: &str, focused: u64) -> Vec<ilium_plugin_sdk::types::MessageSpec> {
    let mut host = activated_client();
    let submissions = host.deliver_one(command_event(command, Some(focused)));
    messages(&submissions)
}

/// REQ: F-003
#[test]
fn f_003_split_right_command_is_forwarded_to_the_server_side() {
    let sent = forwarded("split-right", 7);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].channel, "split-right");
}

/// REQ: F-003
#[test]
fn f_003_split_left_command_is_forwarded_to_the_server_side() {
    let sent = forwarded("split-left", 7);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].channel, "split-left");
}

/// REQ: F-003
#[test]
fn f_003_split_up_command_is_forwarded_to_the_server_side() {
    let sent = forwarded("split-up", 7);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].channel, "split-up");
}

/// REQ: F-003
#[test]
fn f_003_split_down_command_is_forwarded_to_the_server_side() {
    let sent = forwarded("split-down", 7);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].channel, "split-down");
}

/// REQ: F-003
#[test]
fn f_003_forwarded_message_targets_the_server_side_of_the_layout_plugin() {
    let sent = forwarded("split-right", 7);
    assert_eq!(sent[0].to.plugin.0, PLUGIN_ID);
    assert_eq!(sent[0].to.side, PluginSide::Server);
    assert_eq!(sent[0].to.client, None);
}

/// REQ: F-003
#[test]
fn f_003_forwarded_payload_is_the_focused_node_id_as_8_little_endian_bytes() {
    let sent = forwarded("split-down", 0x0102_0304_0506_0708);
    assert_eq!(
        sent[0].payload,
        vec![0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
    );
}

/// REQ: F-003
#[test]
fn f_003_forwarding_sends_nothing_else() {
    let mut host = activated_client();
    let submissions = host.deliver_one(command_event("split-left", Some(3)));
    assert_eq!(submissions.len(), 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    // REQ: F-003
    #[test]
    fn f_003_forwarded_payload_round_trips_the_focused_id(focused in any::<u64>(), which in 0usize..4) {
        let channel = SPLIT_CHANNELS[which];
        let sent = forwarded(channel, focused);
        prop_assert_eq!(sent.len(), 1);
        prop_assert_eq!(&sent[0].channel, channel);
        prop_assert_eq!(sent[0].payload.len(), 8);
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&sent[0].payload);
        prop_assert_eq!(u64::from_le_bytes(bytes), focused);
    }
}
