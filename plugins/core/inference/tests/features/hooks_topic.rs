//! The extension points of the page (`before-request`, `after-response`, `provider-profiles`),
//! the `request-finished` topic and the hook failure rule ("late, traps or a reply the mode does
//! not allow counts as pass").

use ilium_plugin_core_inference::InferenceServer;
use ilium_plugin_sdk::exports::{HookReply, ProviderCall};
use ilium_plugin_sdk::host::HttpResponse;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::InferenceProvider;
use serde_json::{json, Value};

use crate::common::{client_host, openai_body, request, server_host};

const BEFORE: &str = "ilium.core.inference/before-request";
const AFTER: &str = "ilium.core.inference/after-response";
const PROFILES: &str = "ilium.core.inference/provider-profiles";
const TOPIC: &str = "ilium.core.inference/request-finished";
const CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";

fn answer_ok(host: &mut FakeHost) {
    host.http().expect(
        "POST",
        CHAT_URL,
        HttpResponse { status: 200, headers: vec![], body: openai_body("ok", "gpt-5.6-terra", 11, 2) },
    );
}

fn run_call(host: &mut FakeHost, number: u64, purpose: &str, text: &str) {
    let accepted =
        host.instance_mut::<InferenceServer>().complete(ProviderCall { call: number, request: request(purpose, text) });
    assert!(accepted.is_ok());
    host.settle();
}

fn payloads(host: &mut FakeHost, point: &str) -> Vec<Value> {
    host.extensions()
        .hook_payloads(point)
        .iter()
        .map(|text| serde_json::from_str(text).expect("payload is JSON"))
        .collect()
}

fn sent_text(host: &mut FakeHost) -> String {
    let calls = host.http().calls();
    let body: Value = serde_json::from_slice(calls[0].body.as_ref().expect("body")).expect("JSON body");
    body["messages"][0]["content"].as_str().expect("content").to_string()
}

/// REQ: F-084, SEC-293
#[test]
fn hook_084_before_request_runs_once_per_request() {
    let mut host = server_host();
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    assert_eq!(payloads(&mut host, BEFORE).len(), 1);
}

/// REQ: F-084
#[test]
fn hook_084_before_request_payload_has_exactly_the_five_documented_fields() {
    let mut host = server_host();
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    let payload = payloads(&mut host, BEFORE).remove(0);
    let mut keys: Vec<&str> = payload.as_object().expect("object").keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["messages_length", "model", "provider", "purpose", "text"]);
}

/// REQ: F-084
#[test]
fn hook_084_before_request_payload_names_purpose_provider_model_and_text() {
    let mut host = server_host();
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    let payload = payloads(&mut host, BEFORE).remove(0);
    assert_eq!(payload["purpose"], "title");
    assert_eq!(payload["provider"], "open_ai");
    assert_eq!(payload["model"], "gpt-5.6-terra");
    assert_eq!(payload["text"], "hello");
    assert!(payload["messages_length"].is_u64());
}

/// REQ: F-084
#[test]
fn hook_084_replace_rewrites_the_text_that_is_sent() {
    let mut host = server_host();
    host.extensions().on_hook(BEFORE, HookReply::Replace(json!({"text": "[redacted]"}).to_string()));
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "path /home/arthur/secret");
    assert_eq!(sent_text(&mut host), "[redacted]");
}

/// REQ: F-084
#[test]
fn hook_084_pass_sends_the_text_unchanged() {
    let mut host = server_host();
    host.extensions().on_hook(BEFORE, HookReply::Pass);
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    assert_eq!(sent_text(&mut host), "hello");
}

/// REQ: F-084
#[test]
fn hook_084_refuse_sends_nothing_and_fails_the_call() {
    let mut host = server_host();
    host.extensions().on_hook(BEFORE, HookReply::Refuse("private text".to_string()));
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    assert!(host.http().calls().is_empty());
    let replies = host.inference().provider_replies();
    assert_eq!(replies.len(), 1);
    assert!(replies[0].1.is_err());
}

/// REQ: F-084
#[test]
fn hook_084_an_answer_reply_is_not_allowed_in_transform_mode_and_counts_as_pass() {
    let mut host = server_host();
    host.extensions().on_hook(BEFORE, HookReply::Answer("{}".to_string()));
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    assert_eq!(sent_text(&mut host), "hello");
}

/// REQ: F-084
#[test]
fn hook_084_a_trapping_handler_counts_as_pass() {
    let mut host = server_host();
    host.extensions().on_hook_trap(BEFORE);
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    assert_eq!(sent_text(&mut host), "hello");
    assert_eq!(host.inference().provider_replies().len(), 1);
}

/// REQ: F-084
#[test]
fn hook_084_each_handler_gets_the_previous_output() {
    let mut host = server_host();
    host.extensions().on_hook(BEFORE, HookReply::Replace(json!({"text": "step one"}).to_string()));
    host.extensions().on_hook(BEFORE, HookReply::Replace(json!({"text": "step two"}).to_string()));
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "start");
    let seen = payloads(&mut host, BEFORE);
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0]["text"], "start");
    assert_eq!(seen[1]["text"], "step one");
    assert_eq!(sent_text(&mut host), "step two");
}

/// REQ: F-084
#[test]
fn hook_084_after_response_runs_once_with_the_six_documented_fields() {
    let mut host = server_host();
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    let seen = payloads(&mut host, AFTER);
    assert_eq!(seen.len(), 1);
    let mut keys: Vec<&str> = seen[0].as_object().expect("object").keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["input_tokens", "millis", "model", "output_tokens", "provider", "purpose"]);
}

/// REQ: F-084
#[test]
fn hook_084_after_response_reports_the_token_counts_of_the_response() {
    let mut host = server_host();
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    let payload = payloads(&mut host, AFTER).remove(0);
    assert_eq!(payload["input_tokens"], 11);
    assert_eq!(payload["output_tokens"], 2);
    assert_eq!(payload["purpose"], "title");
    assert_eq!(payload["provider"], "open_ai");
    assert_eq!(payload["model"], "gpt-5.6-terra");
}

/// REQ: F-084
#[test]
fn hook_084_after_response_is_observe_only_so_a_replacement_changes_nothing() {
    let mut host = server_host();
    host.extensions().on_hook(AFTER, HookReply::Replace(json!({"purpose": "other"}).to_string()));
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    let replies = host.inference().provider_replies();
    assert_eq!(replies[0].1.clone().expect("ok").text, "ok");
}

/// REQ: F-084
#[test]
fn hook_084_after_response_runs_after_before_request() {
    let mut host = server_host();
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    assert_eq!(payloads(&mut host, BEFORE).len(), 1);
    assert_eq!(payloads(&mut host, AFTER).len(), 1);
}

/// REQ: F-084
#[test]
fn topic_084_request_finished_is_published_once_per_finished_request() {
    let mut host = server_host();
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    let published: Vec<_> = host.published_topics().into_iter().filter(|(topic, _)| topic == TOPIC).collect();
    assert_eq!(published.len(), 1);
}

/// REQ: F-084
#[test]
fn topic_084_request_finished_payload_is_purpose_provider_and_millis() {
    let mut host = server_host();
    answer_ok(&mut host);
    run_call(&mut host, 1, "title", "hello");
    let (_, payload) = host.published_topics().into_iter().find(|(topic, _)| topic == TOPIC).expect("published");
    let payload: Value = serde_json::from_str(&payload).expect("JSON");
    let mut keys: Vec<&str> = payload.as_object().expect("object").keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["millis", "provider", "purpose"]);
    assert_eq!(payload["purpose"], "title");
    assert_eq!(payload["provider"], "open_ai");
    assert!(payload["millis"].is_u64());
}

/// REQ: F-084
#[test]
fn topic_084_nothing_is_published_before_a_request_runs() {
    let mut host = server_host();
    host.settle();
    assert!(host.published_topics().is_empty());
}

/// REQ: F-084
#[test]
fn profiles_084_without_contributors_the_chooser_offers_only_the_built_in_providers() {
    let mut host = client_host();
    let target = ilium_plugin_sdk::types::ViewTarget::SettingsPage("inference".to_string());
    for _ in 0..6 {
        host.key(target.clone(), "right");
    }
    let screen = host.render(target, 80, 12);
    assert!(!screen.text().contains("LM Studio"));
}

/// REQ: F-084
#[test]
fn profiles_084_a_contributed_profile_appears_in_the_provider_chooser() {
    let mut host = client_host();
    host.extensions().contribute_profile(
        &json!({
            "id": "lmstudio", "title": "LM Studio", "base_url": "http://localhost:1234/v1",
            "models_path": "/models", "key_name": "LM_STUDIO_KEY"
        })
        .to_string(),
    );
    let target = ilium_plugin_sdk::types::ViewTarget::SettingsPage("inference".to_string());
    let mut shown = false;
    for _ in 0..8 {
        host.key(target.clone(), "right");
        if host.render(target.clone(), 80, 12).text().contains("LM Studio") {
            shown = true;
            break;
        }
    }
    assert!(shown);
}

/// REQ: F-084
#[test]
fn profiles_084_an_entry_without_a_base_url_is_not_offered() {
    let mut host = client_host();
    host.extensions().contribute_profile(
        &json!({"id": "broken", "title": "Broken Profile", "models_path": "/models", "key_name": "K"}).to_string(),
    );
    let target = ilium_plugin_sdk::types::ViewTarget::SettingsPage("inference".to_string());
    for _ in 0..8 {
        host.key(target.clone(), "right");
        assert!(!host.render(target.clone(), 80, 12).text().contains("Broken Profile"));
    }
}

/// REQ: F-084, SEC-293
#[test]
fn profiles_293_the_collect_point_name_is_owned_by_this_plugin() {
    assert_eq!(PROFILES, "ilium.core.inference/provider-profiles");
    assert!(crate::common::page_text().contains("| `provider-profiles` | collect |"));
}
