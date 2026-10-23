//! The extension points, the service and the topic the page declares: `transcript` (hook, transform) and
//! `voice-intents` (collect), the `say` service and the `heard` topic.

use ilium_plugin_sdk::exports::{HookCall, HookReply};
use ilium_plugin_sdk::testing::FakeSession;
use serde_json::json;

use crate::common::{enabled_client, hear};
use crate::manual::{manual_page, page_text, table_after};

fn point_rows() -> Vec<Vec<String>> {
    table_after(&page_text(), "| Point | Kind |")
}

/// REQ: F-050
#[test]
fn points_050_the_page_lists_exactly_two_points() {
    let names: Vec<String> = point_rows()
        .into_iter()
        .map(|row| row[0].trim_matches('`').to_string())
        .collect();
    assert_eq!(names, ["transcript", "voice-intents"]);
}

/// REQ: F-050
#[test]
fn points_050_transcript_is_a_transform_hook_with_text_confidence_and_language() {
    let rows = point_rows();
    let row = rows
        .iter()
        .find(|row| row[0] == "`transcript`")
        .expect("row");
    assert_eq!(row[1], "hook (transform)");
    assert_eq!(
        row[2],
        "Correct the transcript before commands are matched."
    );
    assert_eq!(row[3], "text, confidence, language; reply: text");
    assert_eq!(row[4], "standard");
}

/// REQ: F-050
#[test]
fn points_050_voice_intents_is_a_collect_point_with_phrases_command_and_confirm() {
    let rows = point_rows();
    let row = rows
        .iter()
        .find(|row| row[0] == "`voice-intents`")
        .expect("row");
    assert_eq!(row[1], "collect");
    assert_eq!(row[2], "Add a spoken phrase that runs a command.");
    assert_eq!(row[3], "entry: id, phrases, command, confirm");
    assert_eq!(row[4], "standard");
}

/// REQ: F-050
#[test]
fn points_050_the_page_names_the_provide_and_contribute_grants() {
    let page = page_text();
    assert!(page.contains("(grant `extensions.provide:ilium.core.voice/<point>`)"));
    assert!(page.contains("`extensions.contribute:ilium.core.voice/<point>`"));
}

/// REQ: F-050
#[test]
fn points_050_the_core_extension_point_tables_agree_with_the_page() {
    let core = manual_page("reference-core-extension-points.md");
    assert!(core
        .contains("| [Voice](/pages/core-plugin-voice/) | `ilium.core.voice` | 2 | 1 | 1 | yes |"));
    assert!(core.contains("| `ilium.core.voice/transcript` | hook: transform | standard | Correct the transcript before commands are matched. |"));
    assert!(core.contains("| `ilium.core.voice/voice-intents` | collect | standard | Add a spoken phrase that runs a command. |"));
}

/// REQ: F-050
#[test]
fn points_050_the_say_service_and_heard_topic_are_declared() {
    let page = page_text();
    assert!(page
        .contains("- `ilium.core.voice/say`: type: type a sentence into the live voice session"));
    assert!(page.contains("(grant `services.use:<name>`)"));
    assert!(page.contains("- `ilium.core.voice/heard`: { text, command }"));
}

/// REQ: F-050
#[test]
fn points_050_the_plugin_is_replaceable() {
    assert!(page_text().contains(
        "**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.voice\"]`"
    ));
}

/// REQ: F-050
#[test]
fn points_050_the_example_package_adds_the_open_the_board_phrase_without_code() {
    let page = page_text();
    assert!(page.contains("#### Example: Adds the phrase `open the board`"));
    assert!(page.contains("\"point\": \"ilium.core.voice/voice-intents\""));
    assert!(page.contains("\"priority\": 50"));
    assert!(page.contains("\"command\": \"open-board\""));
}

/// REQ: F-050
#[test]
fn points_050_the_default_harness_replies_are_an_empty_collect_and_a_pass_hook() {
    let harness = manual_page("reference-test-harness.md");
    assert!(harness.contains("| `ilium.core.voice` | `transcript` (transform), `voice-intents` (collect) | none | collect: empty list; hooks: `pass` |"));
}

/// REQ: F-050
#[test]
fn points_050_the_transcript_hook_is_called_with_text_confidence_and_language() {
    let mut host = enabled_client("push_to_talk");
    host.set_hook_reply("ilium.core.voice/transcript", HookReply::Pass);
    hear(&mut host, "hello there");
    let calls = host.hook_payloads("ilium.core.voice/transcript");
    assert_eq!(calls.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&calls[0]).expect("payload JSON");
    assert_eq!(payload["text"], "hello there");
    assert!(payload.get("confidence").is_some());
    assert!(payload.get("language").is_some());
}

/// REQ: F-050
#[test]
fn points_050_the_hook_call_shape_is_a_plain_hook_call() {
    let call = HookCall {
        call: 1,
        point: "transcript".to_string(),
        extension: "test-contributor".to_string(),
        payload_json: "{}".to_string(),
    };
    assert_eq!(call.point, "transcript");
}

/// REQ: F-050
#[test]
fn points_050_heard_topic_publishes_text_and_command_for_a_matched_phrase() {
    let mut host = enabled_client("push_to_talk");
    crate::common::contribute_phrase(
        &mut host,
        "open-board",
        "open the board",
        "open-board",
        false,
    );
    hear(&mut host, "open the board");
    let topics = host.published_topics();
    let heard: Vec<_> = topics
        .iter()
        .filter(|(name, _)| name == "ilium.core.voice/heard")
        .collect();
    assert_eq!(heard.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&heard[0].1).expect("payload JSON");
    assert_eq!(
        payload,
        json!({ "text": "open the board", "command": "open-board" })
    );
}

/// REQ: F-050
#[test]
fn points_050_heard_topic_carries_a_null_command_for_an_unmatched_phrase() {
    let mut host = enabled_client("push_to_talk");
    hear(&mut host, "nothing matches this");
    let topics = host.published_topics();
    let heard: Vec<_> = topics
        .iter()
        .filter(|(name, _)| name == "ilium.core.voice/heard")
        .collect();
    assert_eq!(heard.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&heard[0].1).expect("payload JSON");
    assert_eq!(payload["text"], "nothing matches this");
    assert!(payload["command"].is_null());
}

/// REQ: F-050
#[test]
fn points_050_say_service_is_not_found_when_voice_is_not_installed() {
    let mut session = FakeSession::new();
    session.start();
    let reply = session.call_service(
        "com.example.caller",
        "ilium.core.voice/say",
        "^0.1.0",
        &json!({ "text": "open the board" }).to_string(),
    );
    assert_eq!(
        reply.expect_err("no provider").code,
        ilium_plugin_sdk::types::ErrorCode::NotFound
    );
}

/// REQ: F-050
#[test]
fn points_050_say_service_types_a_sentence_into_the_live_session() {
    let mut session = FakeSession::new();
    session.install_core("voice").start();
    let reply = session.call_service(
        "com.example.caller",
        "ilium.core.voice/say",
        "^0.1.0",
        &json!({ "text": "what agents are running?" }).to_string(),
    );
    assert!(reply.is_ok(), "{reply:?}");
}
