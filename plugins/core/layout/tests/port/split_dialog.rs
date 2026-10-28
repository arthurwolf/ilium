//! Twin of the "New split view" dialog of `ilium-client/src/keys.rs`: the orientation step can skip the
//! optional member picker and create an empty split view.

use ilium_plugin_sdk::types::SplitDirection;

use crate::common::*;

/// REQ: PORT:ilium-client/src/keys.rs::split_orientation_dialog_can_skip_the_optional_member_picker
#[test]
fn port_001_split_orientation_dialog_can_skip_the_optional_member_picker() {
    let mut host = activated_server();
    // Assumed channel `split-empty-vertical` (the `e` answer of the orientation step); the payload is the group.
    let group = host.tree().add_group("work");
    host.deliver_one(message_from_client("split-empty-vertical", id_bytes(group)));
    host.settle();
    let splits = create_splits(&host.submitted());
    assert_eq!(splits.len(), 1);
    assert!(splits[0].panes.is_empty(), "no members were picked");
    assert_eq!(splits[0].direction, SplitDirection::Vertical);
}
