//! The Chatroom row of the tree and its unread count (chatroom-B16, F-062): `list-virtual-children`,
//! `set-virtual-rows` and `set-expanded`.

use ilium_plugin_sdk::types::intents::{ExpandTarget, Intent};
use ilium_plugin_sdk::VirtualRowsProvider;

use crate::common::*;

fn titles(host: &mut ilium_plugin_sdk::testing::FakeHost) -> Vec<String> {
    row_titles(&host.submitted())
}

fn last_title(host: &mut ilium_plugin_sdk::testing::FakeHost) -> String {
    titles(host).pop().expect("a set-virtual-rows submission")
}

/// REQ: chatroom-B16, F-062
#[test]
fn unread_b16_with_no_unread_message_the_row_title_is_chatroom() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    assert_eq!(last_title(&mut host), "Chatroom");
}

/// REQ: chatroom-B16
#[test]
fn unread_b16_one_unread_message_shows_1_new() {
    let mut host = server_host();
    seed_room(&mut host, &[numbered_line(1)]);
    assert_eq!(last_title(&mut host), "Chatroom (1 new)");
}

/// REQ: chatroom-B16
#[test]
fn unread_b16_three_unread_messages_show_3_new() {
    let mut host = server_host();
    seed_room(
        &mut host,
        &[numbered_line(1), numbered_line(2), numbered_line(3)],
    );
    assert_eq!(last_title(&mut host), "Chatroom (3 new)");
}

/// REQ: chatroom-B16
#[test]
fn unread_b16_a_new_record_raises_the_count_by_one() {
    let mut host = server_host();
    seed_room(&mut host, &[numbered_line(1)]);
    send(&mut host, "two");
    host.settle();
    assert_eq!(last_title(&mut host), "Chatroom (2 new)");
}

/// REQ: chatroom-B16, chatroom-B15
#[test]
fn unread_b16_the_count_is_computed_for_the_reader_key_user() {
    let mut host = server_host();
    seed_room(&mut host, &[numbered_line(1), numbered_line(2)]);
    // Another reader's cursor does not change the count of `user`.
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-9"],
    );
    host.settle();
    assert_eq!(last_title(&mut host), "Chatroom (2 new)");
}

/// REQ: chatroom-B16
#[test]
fn unread_b16_the_row_is_expandable_and_has_the_kind_chatroom() {
    let mut host = server_host();
    seed_room(&mut host, &[numbered_line(1)]);
    let spec = host
        .submitted()
        .into_iter()
        .rev()
        .find_map(|submission| match submission.intent {
            Intent::SetVirtualRows(spec) if spec.row_kind == "chatroom" => Some(spec),
            _ => None,
        })
        .expect("set-virtual-rows");
    assert_eq!(spec.rows.len(), 1);
    assert_eq!(spec.rows[0].id, "chatroom");
    assert!(spec.rows[0].expandable);
}

/// REQ: chatroom-B16, F-062
#[test]
fn unread_b16_without_a_room_file_no_row_is_published() {
    let mut host = server_host();
    host.settle();
    assert!(titles(&mut host).is_empty());
}

/// REQ: F-062
#[test]
fn unread_f062_list_virtual_children_answers_one_file_child() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    let plugin = host.instance_mut::<ilium_plugin_core_chatroom::ChatroomServer>();
    let (children, cursor) = plugin
        .list_virtual_children("chatroom".into(), "chatroom".into(), None)
        .expect("children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, "file");
    let label: String = children[0]
        .label
        .iter()
        .map(|span| span.text.as_str())
        .collect();
    assert_eq!(label, "CHATROOM.md");
    assert_eq!(cursor, None);
}

/// REQ: F-062
#[test]
fn unread_f062_list_virtual_children_for_an_unknown_row_kind_is_not_found() {
    let mut host = server_host();
    let plugin = host.instance_mut::<ilium_plugin_core_chatroom::ChatroomServer>();
    let error = plugin
        .list_virtual_children("other".into(), "x".into(), None)
        .expect_err("unknown kind");
    assert_eq!(error.code, ilium_plugin_sdk::types::ErrorCode::NotFound);
}

/// REQ: F-062
#[test]
fn unread_f062_list_virtual_children_answers_within_20_ms_of_fake_time() {
    let mut host = host_with_records(2000);
    let before = host.clock().now();
    let plugin = host.instance_mut::<ilium_plugin_core_chatroom::ChatroomServer>();
    plugin
        .list_virtual_children("chatroom".into(), "chatroom".into(), None)
        .expect("children");
    assert!(host.clock().now() - before <= 20);
}

/// REQ: F-062
#[test]
fn unread_f062_opening_the_row_sets_it_expanded() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    let expanded = host
        .submitted()
        .into_iter()
        .any(|submission| match submission.intent {
            Intent::SetExpanded(spec) => {
                matches!(spec.target, ExpandTarget::VirtualRow(ref id) if id == "chatroom")
                    && spec.expanded
            }
            _ => false,
        });
    assert!(expanded);
}
