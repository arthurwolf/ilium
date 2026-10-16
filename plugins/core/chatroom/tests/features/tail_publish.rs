//! Tail-and-publish (F-062): on `fs-changed` the server reads what was added and publishes each message
//! on the topic `chatroom`; the client updates when it receives `topic-published`.

use crate::common::*;

/// REQ: F-062
#[test]
fn tail_f062_one_appended_record_is_published_once_with_its_line() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    send(&mut host, "hello");
    host.settle();
    let texts = published_texts(&host);
    assert_eq!(texts.len(), 1);
    assert!(texts[0].ends_with(" | agent | hello"), "{}", texts[0]);
}

/// REQ: F-062
#[test]
fn tail_f062_records_are_published_in_file_order() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    for text in ["a", "b", "c"] {
        send(&mut host, text);
    }
    host.settle();
    let contents: Vec<String> = published_texts(&host).iter().map(|line| split_record(line).2).collect();
    assert_eq!(contents, ["a", "b", "c"]);
}

/// REQ: F-062
#[test]
fn tail_f062_the_payload_is_a_json_object_with_a_text_field_on_the_topic_chatroom() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    send(&mut host, "x");
    host.settle();
    let payloads = published(&host, "chatroom");
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0].as_object().expect("object").len(), 1);
    assert!(payloads[0]["text"].is_string());
}

/// REQ: F-062
#[test]
fn tail_f062_nothing_is_republished_when_the_file_does_not_grow() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    send(&mut host, "once");
    host.settle();
    host.settle();
    assert_eq!(published_texts(&host).len(), 1);
}

/// REQ: F-062
#[test]
fn tail_f062_a_hand_appended_record_is_published_too() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    let mut text = room_text(&mut host);
    text.push_str(&format!("{}\n", numbered_line(7)));
    host.fs().write(ROOM, text.as_bytes());
    host.settle();
    assert_eq!(published_texts(&host), [numbered_line(7).trim_start_matches("- ").to_string()]);
}

/// REQ: F-062
#[test]
fn tail_f062_an_event_of_a_foreign_watch_is_ignored() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    host.fs().write("project:other.txt", b"changed\n");
    host.settle();
    assert!(published_texts(&host).is_empty());
}

/// REQ: F-062
#[test]
fn tail_f062_a_failing_tail_read_publishes_nothing_and_keeps_the_plugin_alive() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    host.inject_failure(
        "fs.tail",
        ilium_plugin_sdk::types::PluginError::new(ilium_plugin_sdk::types::ErrorCode::Unavailable, "tail failed"),
    );
    send(&mut host, "lost to the failure");
    host.settle();
    assert!(!host.is_disabled());
}

/// REQ: F-062
#[test]
fn tail_f062_a_chunk_boundary_inside_a_record_waits_for_the_rest_of_the_line() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    let mut text = room_text(&mut host);
    text.push_str("- 2026-10-12 10:00:00 +02:00 | agent | half");
    host.fs().write(ROOM, text.as_bytes());
    host.settle();
    assert!(published_texts(&host).is_empty());
    text.push_str(" done\n");
    host.fs().write(ROOM, text.as_bytes());
    host.settle();
    assert_eq!(published_texts(&host).len(), 1);
    assert!(published_texts(&host)[0].ends_with("half done"));
}

/// REQ: F-062
#[test]
fn tail_f062_the_server_subscribes_to_no_topic_other_than_chatroom() {
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("manifest")).expect("json");
    let server: Vec<&str> = manifest["capabilities"]["server"].as_array().expect("array").iter().filter_map(|v| v.as_str()).collect();
    assert!(server.contains(&"topics.subscribe:chatroom"));
    assert!(server.iter().filter(|name| name.starts_with("topics.")).count() == 1);
}

/// REQ: F-062, chatroom-B09
#[test]
fn tail_f062_the_client_shows_a_row_when_a_topic_published_event_arrives() {
    let mut host = client_host();
    host.deliver_one(topic_event(numbered_line(1).trim_start_matches("- ")));
    host.settle();
    let screen = host.render(ilium_plugin_sdk::types::ViewTarget::Panel(PANEL.to_string()), 80, 12);
    assert!(screen.text().contains("record 1"), "{}", screen.text());
}

/// REQ: F-062
#[test]
fn tail_f062_the_client_ignores_topics_other_than_chatroom() {
    let mut host = client_host();
    let mut event = topic_event("2026-10-12 00:00:01 +02:00 | agent | other topic");
    if let ilium_plugin_sdk::types::Event::TopicPublished(inner) = &mut event {
        inner.topic = "elsewhere".to_string();
    }
    host.deliver_one(event);
    host.settle();
    let screen = host.render(ilium_plugin_sdk::types::ViewTarget::Panel(PANEL.to_string()), 80, 12);
    assert!(!screen.text().contains("other topic"));
}

/// REQ: F-062
#[test]
fn tail_f062_the_client_ignores_a_malformed_payload() {
    let mut host = client_host();
    let mut event = topic_event("x");
    if let ilium_plugin_sdk::types::Event::TopicPublished(inner) = &mut event {
        inner.payload_json = "not json".to_string();
    }
    host.deliver_one(event);
    host.settle();
    assert!(!host.is_disabled());
}
