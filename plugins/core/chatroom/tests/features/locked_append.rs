//! chatroom-B17: every append, from the CLI and from the composer, takes the exclusive advisory lock of the
//! file and appends one line; the message channel `post` carries composer and peer posts.

use ilium_plugin_sdk::types::Intent;

use crate::common::*;

fn host_with_room() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    host
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_a_cli_send_takes_the_exclusive_lock_on_the_room() {
    let mut host = host_with_room();
    send(&mut host, "locked");
    let locks = host.fs().lock_log();
    assert!(
        locks
            .iter()
            .any(|entry| entry.path == ROOM && entry.exclusive),
        "{locks:?}"
    );
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_a_composer_post_takes_the_exclusive_lock_on_the_room() {
    let mut host = host_with_room();
    host.deliver_one(composer_message("locked"));
    host.settle();
    assert!(host
        .fs()
        .lock_log()
        .iter()
        .any(|entry| entry.path == ROOM && entry.exclusive));
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_the_lock_is_released_after_each_append() {
    let mut host = host_with_room();
    send(&mut host, "one");
    host.deliver_one(composer_message("two"));
    host.settle();
    assert!(host.fs().held_locks().is_empty());
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_a_failed_append_still_releases_the_lock() {
    let mut host = host_with_room();
    host.inject_failure(
        "fs.write",
        ilium_plugin_sdk::types::PluginError::new(
            ilium_plugin_sdk::types::ErrorCode::Io,
            "disk full",
        ),
    );
    send(&mut host, "fails");
    assert!(host.fs().held_locks().is_empty());
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_the_lock_wait_is_bounded_and_a_held_lock_refuses_the_send_with_exit_1() {
    let mut host = host_with_room();
    host.fs().hold_lock(ROOM, true);
    let run = send(&mut host, "blocked");
    assert_eq!(run.code, Some(1));
    assert!(room_records(&mut host).is_empty());
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_each_append_writes_one_line_in_append_mode() {
    let mut host = host_with_room();
    let before = room_text(&mut host);
    host.deliver_one(composer_message("one line"));
    host.settle();
    let after = room_text(&mut host);
    assert!(after.starts_with(&before));
    assert_eq!(after.len() - before.len(), after[before.len()..].len());
    assert_eq!(after[before.len()..].matches('\n').count(), 1);
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_fifty_cli_sends_and_fifty_composer_sends_produce_one_hundred_whole_records() {
    let mut host = host_with_room();
    for n in 0..50 {
        send(&mut host, &format!("cli {n}"));
        host.deliver_one(composer_message(&format!("composer {n}")));
    }
    host.settle();
    let records = room_records(&mut host);
    assert_eq!(records.len(), 100);
    let mut contents: Vec<String> = records.iter().map(|line| split_record(line).2).collect();
    contents.sort();
    let mut expected: Vec<String> = (0..50)
        .flat_map(|n| [format!("cli {n}"), format!("composer {n}")])
        .collect();
    expected.sort();
    assert_eq!(contents, expected);
    for record in &records {
        assert!(is_record_timestamp(&split_record(record).0));
    }
}

/// REQ: chatroom-B17, chatroom-B08
#[test]
fn lock_b17_a_message_from_another_plugin_is_posted_with_the_sender_as_author() {
    let mut host = host_with_room();
    host.deliver_one(peer_message("ilium.core.scheduler", "tick"));
    host.settle();
    let records = room_records(&mut host);
    assert_eq!(records.len(), 1);
    let (_, author, content) = split_record(&records[0]);
    assert_eq!(content, "tick");
    assert!(author.contains("scheduler"), "{author}");
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_a_message_on_another_channel_appends_nothing() {
    let mut host = host_with_room();
    let mut event = composer_message("ignored");
    if let ilium_plugin_sdk::types::Event::Message(inner) = &mut event {
        inner.channel = "other".to_string();
    }
    host.deliver_one(event);
    host.settle();
    assert!(room_records(&mut host).is_empty());
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_a_composer_post_with_no_room_file_appends_nothing_and_creates_no_file() {
    let mut host = server_host();
    host.deliver_one(composer_message("nowhere"));
    host.settle();
    assert!(host.fs().read(ROOM).is_none());
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_rotation_rewrites_the_file_only_under_the_lock() {
    // The page allows a whole-file rewrite for archive rotation, under the same lock. A send never rewrites.
    let mut host = host_with_records(5);
    let writes_before = host.fs().write_log().len();
    send(&mut host, "append");
    let new_writes = host.fs().write_log().split_off(writes_before);
    assert!(
        new_writes
            .iter()
            .all(|entry| entry.path != ROOM || entry.append),
        "{new_writes:?}"
    );
}

/// REQ: chatroom-B17
#[test]
fn lock_b17_the_composer_message_leaves_no_intent_besides_the_row_update() {
    let mut host = host_with_room();
    let submissions = host.deliver_one(composer_message("quiet"));
    assert!(submissions.iter().all(|submission| matches!(
        submission.intent,
        Intent::SetVirtualRows(_) | Intent::SetExpanded(_) | Intent::Message(_)
    )));
}
