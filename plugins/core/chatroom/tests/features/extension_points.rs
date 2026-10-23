//! The extension points, services and topic the page declares: `before-post` (hook, transform),
//! `message-renderers` (collect), services `post` and `tail`, topic `posted`.

use ilium_plugin_sdk::exports::{HookCall, HookReply, ServiceReply};
use ilium_plugin_sdk::testing::FakeSession;
use serde_json::Value;

use crate::common::{page_text, workspace_root, PLUGIN};

fn point_rows() -> Vec<[String; 5]> {
    let page = page_text();
    let start = page
        .find("| Point | Kind |")
        .expect("extension point table");
    page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect();
            [
                cells[0].clone(),
                cells[1].clone(),
                cells[2].clone(),
                cells[3].clone(),
                cells[4].clone(),
            ]
        })
        .collect()
}

fn reference_page() -> String {
    std::fs::read_to_string(
        workspace_root().join("docs/plugins-manual/pages/reference-core-extension-points.md"),
    )
    .expect("core extension points page")
}

/// REQ: F-062
#[test]
fn points_062_the_page_lists_exactly_two_points() {
    let names: Vec<String> = point_rows()
        .into_iter()
        .map(|row| row[0].trim_matches('`').to_string())
        .collect();
    assert_eq!(names, ["before-post", "message-renderers"]);
}

/// REQ: F-062
#[test]
fn points_062_before_post_is_a_transform_hook_with_author_text_kind() {
    let rows = point_rows();
    let row = rows
        .iter()
        .find(|row| row[0] == "`before-post`")
        .expect("row");
    assert_eq!(row[1], "hook (transform)");
    assert_eq!(row[2], "Change a message before it is written to the room.");
    assert_eq!(row[3], "author, text, kind; reply: text");
    assert_eq!(row[4], "standard");
}

/// REQ: F-062
#[test]
fn points_062_message_renderers_is_a_collect_point_with_entry_id_kind_view() {
    let rows = point_rows();
    let row = rows
        .iter()
        .find(|row| row[0] == "`message-renderers`")
        .expect("row");
    assert_eq!(row[1], "collect");
    assert_eq!(
        row[2],
        "Draw a message kind in a special way, such as a task claim."
    );
    assert_eq!(
        row[3],
        "entry: id, kind, view (a plugin view that draws the message)"
    );
    assert_eq!(row[4], "standard");
}

/// REQ: F-062
#[test]
fn points_062_the_reference_page_agrees_with_the_plugin_page() {
    let reference = reference_page();
    assert!(reference.contains("`ilium.core.chatroom/before-post`"));
    assert!(reference.contains("`ilium.core.chatroom/message-renderers`"));
    assert!(reference.contains("`ilium.core.chatroom/post`"));
    assert!(reference.contains("`ilium.core.chatroom/tail`"));
    assert!(reference.contains("`ilium.core.chatroom/posted`"));
}

/// REQ: F-062
#[test]
fn points_062_services_are_post_and_tail() {
    let page = page_text();
    assert!(page.contains("`ilium.core.chatroom/post`: send: post a message"));
    assert!(page.contains("`ilium.core.chatroom/tail`: get: read recent messages"));
}

/// REQ: F-062
#[test]
fn points_062_the_topic_posted_carries_author_kind_length() {
    assert!(page_text().contains("`ilium.core.chatroom/posted`: { author, kind, length }"));
}

/// REQ: F-062
#[test]
fn points_062_the_core_plugin_is_replaceable_with_a_replaces_list() {
    assert!(page_text().contains(
        "**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.chatroom\"]`"
    ));
}

/// REQ: F-062
#[test]
fn points_062_the_example_package_transforms_before_post_with_a_handler() {
    let page = page_text();
    let start = page
        .find("#### Example: Removes lines that look like keys")
        .expect("example");
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("block") + "```json\n".len();
    let close = rest[open..].find("```").expect("end");
    let example: Value = serde_json::from_str(&rest[open..open + close]).expect("json");
    assert_eq!(example["exports"], serde_json::json!(["hook-handler"]));
    assert_eq!(
        example["capabilities"]["server"],
        serde_json::json!(["extensions.contribute:ilium.core.chatroom/before-post"])
    );
    let contribution = &example["contributes"]["extensions"][0];
    assert_eq!(contribution["point"], "ilium.core.chatroom/before-post");
    assert_eq!(contribution["id"], "strip-secrets");
    assert_eq!(contribution["priority"], 50);
    assert_eq!(contribution["handler"], "strip_secrets");
}

/// REQ: F-062
#[test]
fn points_062_a_late_trapping_or_wrong_mode_handler_counts_as_pass() {
    assert!(page_text().contains("A handler that is late, traps or returns a reply the mode does not allow counts as `pass`."));
}

/// REQ: F-062
#[test]
fn points_062_the_stub_hook_reply_for_before_post_is_pass() {
    let mut session = FakeSession::new();
    session.install_core(PLUGIN).start();
    let reply = session
        .run_hook(
            PLUGIN,
            HookCall {
                call: 1,
                point: "ilium.core.chatroom/before-post".to_string(),
                extension: "strip-secrets".to_string(),
                payload_json: r#"{"author":"agent","text":"hello","kind":"message"}"#.to_string(),
            },
        )
        .expect("hook runs");
    assert_eq!(reply, HookReply::Pass);
}

/// REQ: F-062
#[test]
fn points_062_the_stub_serves_post_and_tail_with_ok_true() {
    let mut session = FakeSession::new();
    session.install_core(PLUGIN).start();
    for service in ["ilium.core.chatroom/post", "ilium.core.chatroom/tail"] {
        let reply = session
            .call_service(PLUGIN, service, "*", r#"{}"#)
            .expect("service answers");
        let ServiceReply::Done(json) = reply else {
            panic!("deferred reply for {service}")
        };
        let value: Value = serde_json::from_str(&json).expect("JSON reply");
        assert_eq!(value, serde_json::json!({"ok": true}), "{service}");
    }
}

/// REQ: F-062
#[test]
fn points_062_the_harness_page_lists_the_same_two_points_for_the_core_stub() {
    let harness = std::fs::read_to_string(
        workspace_root().join("docs/plugins-manual/pages/reference-test-harness.md"),
    )
    .expect("harness page");
    assert!(harness.contains("| `ilium.core.chatroom` | `before-post` (transform), `message-renderers` (collect) | none | collect: empty list; hooks: `pass` |"));
}

/// REQ: F-062, chatroom-B01
#[test]
fn points_062_a_before_post_handler_that_replaces_the_text_changes_the_written_record() {
    let mut host = crate::common::server_host();
    crate::common::seed_room(&mut host, &[]);
    host.set_hook_handler("ilium.core.chatroom/before-post", |call: HookCall| {
        let mut payload: Value = serde_json::from_str(&call.payload_json).expect("payload");
        payload["text"] = Value::String("[filtered]".to_string());
        HookReply::Replace(payload.to_string())
    });
    crate::common::send(&mut host, "secret=abc");
    let records = crate::common::room_records(&mut host);
    assert_eq!(crate::common::split_record(&records[0]).2, "[filtered]");
}

/// REQ: F-062, chatroom-B01
#[test]
fn points_062_a_before_post_handler_that_traps_leaves_the_text_unchanged() {
    let mut host = crate::common::server_host();
    crate::common::seed_room(&mut host, &[]);
    host.set_hook_trap("ilium.core.chatroom/before-post");
    crate::common::send(&mut host, "plain");
    assert_eq!(
        crate::common::split_record(&crate::common::room_records(&mut host)[0]).2,
        "plain"
    );
}

/// REQ: F-062, chatroom-B11
#[test]
fn points_062_a_before_post_handler_that_empties_the_text_makes_the_send_fail_as_empty() {
    let mut host = crate::common::server_host();
    crate::common::seed_room(&mut host, &[]);
    host.set_hook_handler("ilium.core.chatroom/before-post", |call: HookCall| {
        let mut payload: Value = serde_json::from_str(&call.payload_json).expect("payload");
        payload["text"] = Value::String(String::new());
        HookReply::Replace(payload.to_string())
    });
    let run = crate::common::send(&mut host, "anything");
    assert_eq!(run.code, Some(1));
    assert!(crate::common::room_records(&mut host).is_empty());
}

/// REQ: F-062
#[test]
fn points_062_a_posted_record_publishes_the_topic_posted_with_author_kind_and_length() {
    let mut host = crate::common::server_host();
    crate::common::seed_room(&mut host, &[]);
    crate::common::chat(
        &mut host,
        &["send", "--message", "twelve chars", "--author", "fix-auth"],
    );
    let payloads = crate::common::published(&host, "ilium.core.chatroom/posted");
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["author"], "fix-auth");
    assert_eq!(payloads[0]["length"], 12);
    assert!(payloads[0]["kind"].is_string());
}
