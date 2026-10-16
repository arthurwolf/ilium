//! Page sample "New group or folder": the server side turns a message into `create-group` / `create-folder`.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    CreateFolderSpec, CreateGroupSpec, Event, Intent, MessageEvent, NodeId, Placement, PluginId, PluginSide,
    Submission,
};
use proptest::prelude::*;

use crate::common::server_host;

fn message(channel: &str, payload: Vec<u8>) -> Event {
    Event::Message(MessageEvent {
        from: PluginId("ilium.core.tree".to_string()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: channel.to_string(),
        payload,
    })
}

fn send(host: &mut FakeHost, channel: &str, payload: Vec<u8>) -> Vec<Submission> {
    host.deliver_one(message(channel, payload))
}

/// REQ: F-002
#[test]
fn feature_002_new_group_message_submits_create_group_inside_last() {
    let mut host = server_host();
    let submissions = send(&mut host, "new-group", 7u64.to_le_bytes().to_vec());
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].intent,
        Intent::CreateGroup(CreateGroupSpec {
            parent: Some(NodeId(7)),
            title: "New group".to_string(),
            placement: Placement::InsideLast,
            children: vec![],
        })
    );
}

/// REQ: F-002
#[test]
fn feature_002_new_folder_message_submits_create_folder_without_a_path() {
    let mut host = server_host();
    let submissions = send(&mut host, "new-folder", 7u64.to_le_bytes().to_vec());
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].intent,
        Intent::CreateFolder(CreateFolderSpec { parent: Some(NodeId(7)), title: "New folder".to_string(), path: None })
    );
}

/// REQ: F-002
#[test]
fn feature_002_the_dedup_key_is_channel_dash_parent() {
    let mut host = server_host();
    let group = send(&mut host, "new-group", 12u64.to_le_bytes().to_vec());
    let folder = send(&mut host, "new-folder", 12u64.to_le_bytes().to_vec());
    assert_eq!(group[0].dedup_key.as_deref(), Some("new-group-12"));
    assert_eq!(folder[0].dedup_key.as_deref(), Some("new-folder-12"));
}

/// REQ: F-002
#[test]
fn feature_002_an_unknown_channel_submits_nothing() {
    let mut host = server_host();
    assert!(send(&mut host, "new-project", 7u64.to_le_bytes().to_vec()).is_empty());
    assert!(send(&mut host, "", 7u64.to_le_bytes().to_vec()).is_empty());
}

/// REQ: F-002
#[test]
fn feature_002_a_payload_shorter_than_eight_bytes_submits_nothing() {
    let mut host = server_host();
    assert!(send(&mut host, "new-group", vec![1, 2, 3, 4, 5, 6, 7]).is_empty());
    assert!(send(&mut host, "new-group", vec![]).is_empty());
}

/// REQ: F-002
#[test]
fn feature_002_a_payload_longer_than_eight_bytes_submits_nothing() {
    let mut host = server_host();
    assert!(send(&mut host, "new-folder", vec![0; 9]).is_empty());
}

/// REQ: F-002
#[test]
fn feature_002_exactly_eight_bytes_is_accepted() {
    let mut host = server_host();
    let submissions = send(&mut host, "new-group", vec![0; 8]);
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].intent,
        Intent::CreateGroup(CreateGroupSpec {
            parent: Some(NodeId(0)),
            title: "New group".to_string(),
            placement: Placement::InsideLast,
            children: vec![],
        })
    );
    assert_eq!(submissions[0].dedup_key.as_deref(), Some("new-group-0"));
}

/// REQ: F-002
#[test]
fn feature_002_two_messages_in_one_batch_submit_two_creations_in_order() {
    let mut host = server_host();
    let submissions = host.deliver(vec![
        message("new-group", 1u64.to_le_bytes().to_vec()),
        message("new-folder", 2u64.to_le_bytes().to_vec()),
    ]);
    assert_eq!(submissions.len(), 2);
    assert!(matches!(submissions[0].intent, Intent::CreateGroup(_)));
    assert!(matches!(submissions[1].intent, Intent::CreateFolder(_)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // REQ: F-002
    #[test]
    fn feature_002_property_the_parent_round_trips_through_the_payload(parent in 1u64..u64::MAX) {
        let mut host = server_host();
        let submissions = send(&mut host, "new-group", parent.to_le_bytes().to_vec());
        prop_assert_eq!(submissions.len(), 1);
        match &submissions[0].intent {
            Intent::CreateGroup(spec) => prop_assert_eq!(spec.parent, Some(NodeId(parent))),
            other => prop_assert!(false, "unexpected intent {other:?}"),
        }
    }
}
