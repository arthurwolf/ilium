//! EXT-042/F-066: the `after-copy` hook chain and the `copied` topic (page "Extension points").

use ilium_plugin_sdk::exports::HookReply;
use ilium_plugin_sdk::types::{GrabId, Intent, KeyEvent};
use serde_json::Value;

use crate::common::{add_terminal_pane, candidates_message, client_host, PLUGIN_ID};

const AFTER_COPY: &str = "ilium.core.smart-copy/after-copy";
const COPIED: &str = "ilium.core.smart-copy/copied";

fn copy_first(host: &mut ilium_plugin_sdk::testing::FakeHost, text: &str) {
    host.deliver_one(candidates_message(&[text]));
    host.deliver_one(ilium_plugin_sdk::types::Event::KeyPressed(KeyEvent {
        grab: GrabId(0),
        pane: None,
        chord: "1".to_string(),
        repeat: false,
    }));
    host.settle();
}

/// REQ: EXT-042, F-066
#[test]
fn feature_066_after_copy_chain_receives_text_kind_and_pane() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.hook_chain(AFTER_COPY).reply_all(HookReply::Pass);
    host.deliver_one(crate::common::palette_command("smart-copy", Some(pane)));
    copy_first(&mut host, "src/a.rs");
    let payload: Value = host.hook_chain(AFTER_COPY).last_payload().expect("payload");
    assert_eq!(payload["text"], "src/a.rs");
    assert!(
        payload["kind"]
            .as_str()
            .is_some_and(|kind| !kind.is_empty()),
        "{payload}"
    );
    assert_eq!(payload["pane"], Value::from(pane), "{payload}");
}

/// REQ: EXT-042
#[test]
fn feature_066_a_replace_reply_changes_the_clipboard_text() {
    let mut host = client_host();
    host.hook_chain(AFTER_COPY)
        .reply_all(HookReply::Replace("redacted".to_string()));
    copy_first(&mut host, "secret");
    assert_eq!(host.clipboard().last_text().as_deref(), Some("redacted"));
}

/// REQ: EXT-042
#[test]
fn feature_066_a_pass_reply_keeps_the_text() {
    let mut host = client_host();
    host.hook_chain(AFTER_COPY).reply_all(HookReply::Pass);
    copy_first(&mut host, "keep");
    assert_eq!(host.clipboard().last_text().as_deref(), Some("keep"));
}

/// REQ: EXT-042
#[test]
fn feature_066_a_refuse_reply_counts_as_pass_because_the_point_allows_transform_only() {
    let mut host = client_host();
    host.hook_chain(AFTER_COPY)
        .reply_all(HookReply::Refuse("no".to_string()));
    copy_first(&mut host, "keep");
    assert_eq!(host.clipboard().last_text().as_deref(), Some("keep"));
}

/// REQ: EXT-042
#[test]
fn feature_066_the_chain_runs_in_priority_order() {
    let mut host = client_host();
    host.hook_chain(AFTER_COPY)
        .add("late", 200, HookReply::Replace("late".to_string()));
    host.hook_chain(AFTER_COPY)
        .add("early", 100, HookReply::Replace("early".to_string()));
    copy_first(&mut host, "x");
    assert_eq!(host.clipboard().last_text().as_deref(), Some("late"));
}

/// REQ: F-066
#[test]
fn feature_066_copied_topic_is_published_with_kind_and_length_after_hooks() {
    let mut host = client_host();
    host.hook_chain(AFTER_COPY)
        .reply_all(HookReply::Replace("héllo!".to_string()));
    copy_first(&mut host, "x");
    let published = host.published(COPIED);
    assert_eq!(published.len(), 1);
    let value: Value = serde_json::from_str(&published[0]).expect("json");
    assert_eq!(value["length"], 6);
    assert!(value.get("kind").is_some());
    let _ = PLUGIN_ID;
    let _ = Intent::FreezePane;
}

/// REQ: F-066
#[test]
fn feature_066_copied_topic_is_not_published_when_the_clipboard_write_fails() {
    let mut host = client_host();
    host.clipboard().fail_next_write("Io");
    copy_first(&mut host, "x");
    assert!(host.published(COPIED).is_empty());
}
