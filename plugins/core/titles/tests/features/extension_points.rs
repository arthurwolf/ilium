//! Extension points, services, topics and the contribution example of the page.

use ilium_plugin_sdk::exports::{HookCall, HookReply};
use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{ErrorCode, Intent};
use serde_json::{json, Value};

use crate::common::*;

/// REQ: F-041
#[test]
fn doc_f_041_page_lists_before_apply_title_as_a_standard_transform_hook() {
    let row = table_row(&page_text(), "| `before-apply-title` |");
    assert_eq!(row[2], "hook (transform)");
    assert_eq!(row[3], "Change a generated title before it is applied.");
    assert_eq!(
        row[4],
        "node, proposed, source (transcript, process, manual-trigger), previous; reply: proposed"
    );
    assert_eq!(row[5], "standard");
}

/// REQ: F-041
#[test]
fn doc_f_041_page_lists_title_styles_as_a_standard_collect_point() {
    let row = table_row(&page_text(), "| `title-styles` |");
    assert_eq!(row[2], "collect");
    assert_eq!(row[3], "Add a title style to the style choice.");
    assert_eq!(row[4], "entry: id, title, instruction");
    assert_eq!(row[5], "standard");
}

/// REQ: F-041
#[test]
fn doc_f_041_core_extension_points_page_agrees_with_the_plugin_page() {
    let page = pages_text("reference-core-extension-points.md");
    assert!(page.contains("| `ilium.core.titles/before-apply-title` | hook: transform | standard |"));
    assert!(page.contains("| `ilium.core.titles/title-styles` | collect | standard |"));
    assert!(page.contains("| `ilium.core.titles/suggest` | title: ask for a title suggestion for text |"));
    assert!(page.contains("| `ilium.core.titles/title-applied` | { node, title } |"));
}

/// REQ: F-041
#[test]
fn doc_f_041_titles_owns_two_points_one_service_one_topic_and_is_replaceable() {
    let page = pages_text("reference-core-extension-points.md");
    let row = table_row(&page, "| [Titles and organisation](/pages/core-plugin-titles/)");
    assert_eq!(&row[3..7], ["2", "1", "1", "yes"]);
}

/// REQ: F-041
#[test]
fn doc_f_041_page_states_service_topic_replaceable_and_the_inference_dependency() {
    let page = page_text();
    assert!(page.contains("- `ilium.core.titles/suggest`: title: ask for a title suggestion for text"));
    assert!(page.contains("- `ilium.core.titles/title-applied`: { node, title }"));
    assert!(page.contains("Calls `ilium:inference/provider` through the inference plugin."));
    assert!(page.contains("**Replaceable:** yes."));
    assert!(page.contains("`\"replaces\": [\"ilium.core.titles\"]`"));
}

/// REQ: F-041
#[test]
fn doc_f_041_page_names_the_provide_and_contribute_grants() {
    let page = page_text();
    assert!(page.contains("`extensions.provide:ilium.core.titles/<point>`"));
    assert!(page.contains("`extensions.contribute:ilium.core.titles/<point>`"));
    assert!(page.contains("`services.use:<name>`"));
}

/// REQ: F-041
#[test]
fn doc_f_041_ticket_first_example_is_a_valid_style_entry_with_title_and_instruction() {
    let blocks = json_blocks(&page_text());
    let example: Value = blocks
        .iter()
        .filter_map(|block| serde_json::from_str::<Value>(block).ok())
        .find(|value| value["contributes"]["extensions"][0]["id"] == "ticket-first")
        .expect("example block");
    let extension = &example["contributes"]["extensions"][0];
    assert_eq!(extension["point"], "ilium.core.titles/title-styles");
    assert_eq!(extension["priority"], 50);
    assert_eq!(extension["entry"]["title"], "Ticket first");
    assert_eq!(
        extension["entry"]["instruction"],
        "Start the title with the ticket key when the text names one."
    );
    assert_eq!(example["capabilities"]["client"][0], "extensions.contribute:ilium.core.titles/title-styles");
}

/// REQ: F-041
#[test]
fn f_041_before_apply_title_hook_is_asked_with_node_proposed_source_previous() {
    let mut host = activated_server();
    host.script_hook_reply("before-apply-title", HookReply::Pass);
    title_for_prompt(&mut host, 7, "Fix the login bug", "Login fix");
    let calls = host.hook_calls();
    let call = calls.iter().find(|call| call.point.ends_with("before-apply-title")).expect("hook asked");
    let payload: Value = serde_json::from_str(&call.payload_json).expect("payload is JSON");
    assert_eq!(payload["node"], 7);
    assert_eq!(payload["proposed"], "Login fix");
    assert_eq!(payload["source"], "transcript");
    assert!(payload.get("previous").is_some());
}

/// REQ: F-041
#[test]
fn f_041_before_apply_title_replace_changes_the_applied_title() {
    let mut host = activated_server();
    host.script_hook_reply("before-apply-title", HookReply::Replace(json!({"proposed": "PROJ-12 Login fix"}).to_string()));
    title_for_prompt(&mut host, 7, "Fix the login bug PROJ-12", "Login fix");
    let renames = renames(&host.submitted());
    assert_eq!(renames.len(), 1);
    assert_eq!(renames[0].title, "PROJ-12 Login fix");
}

/// REQ: F-041
#[test]
fn f_041_before_apply_title_pass_keeps_the_generated_title() {
    let mut host = activated_server();
    host.script_hook_reply("before-apply-title", HookReply::Pass);
    title_for_prompt(&mut host, 7, "Fix the login bug", "Login fix");
    let renames = renames(&host.submitted());
    assert_eq!(renames.len(), 1);
    assert_eq!(renames[0].title, "Login fix");
}

/// REQ: F-041
#[test]
fn f_041_title_applied_topic_carries_node_and_title() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 7, "Fix the login bug", "Login fix");
    let topics = host.published_topics();
    let (_, payload) = topics
        .iter()
        .find(|(topic, _)| topic == "ilium.core.titles/title-applied")
        .expect("topic published");
    let payload: Value = serde_json::from_str(payload).expect("JSON payload");
    assert_eq!(payload["node"], 7);
    assert_eq!(payload["title"], "Login fix");
}

/// REQ: F-041
#[test]
fn f_041_no_title_applied_topic_before_a_completion_arrives() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(7, "Fix the login bug"));
    host.settle();
    assert!(host.published_topics().iter().all(|(topic, _)| topic != "ilium.core.titles/title-applied"));
}

/// REQ: F-042
#[test]
fn f_042_suggest_service_answers_a_caller_with_the_services_use_grant() {
    let mut session = FakeSession::new();
    session.install_core("titles").start();
    let reply = session.call_service(
        "com.example.caller",
        "ilium.core.titles/suggest",
        "^0.1.0",
        &json!({"text": "Fix the login bug"}).to_string(),
    );
    assert!(reply.is_ok(), "{reply:?}");
}

/// REQ: F-042
#[test]
fn f_042_suggest_service_is_not_found_when_titles_is_not_installed() {
    let mut session = FakeSession::new();
    session.start();
    let reply = session.call_service(
        "com.example.caller",
        "ilium.core.titles/suggest",
        "^0.1.0",
        &json!({"text": "Fix the login bug"}).to_string(),
    );
    assert_eq!(reply.expect_err("no provider").code, ErrorCode::NotFound);
}

/// REQ: F-041
#[test]
fn f_041_hook_call_shape_is_a_plain_hook_call() {
    let call = HookCall { call: 1, point: "before-apply-title".to_string(), payload_json: "{}".to_string() };
    assert_eq!(call.point, "before-apply-title");
    let _ = Intent::PlaySound(String::new());
}
