//! Extension points `formats` (collect, max 8) and `after-convert` (observe hook) of F-077, and the Replaceable rule.

use ilium_plugin_sdk::types::{ErrorCode, Event, PluginError, PluginId, TopicPublishedEvent};
use serde_json::{json, Value};

use crate::common::*;
use crate::start::{server_with_pane, start_claude};
use crate::write_verify::{finish_job, job_output, run_until_job};

const FORMATS: &str = "ilium.core.session-convert/formats";
const AFTER: &str = "ilium.core.session-convert/after-convert";

/// Rows of the Extension points table: `(point, kind)`.
fn point_rows() -> Vec<(String, String)> {
    let page = page_text();
    let start = page.find("| Point | Kind |").expect("extension point table");
    page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<String> = line.trim_matches('|').split('|').map(|cell| cell.trim().to_string()).collect();
            (cells[0].trim_matches('`').to_string(), cells[1].clone())
        })
        .collect()
}

fn changed() -> Event {
    Event::TopicPublished(TopicPublishedEvent {
        topic: "ilium.extensions/changed".to_string(),
        from: PluginId("ilium.kernel".to_string()),
        payload_json: json!({"point": FORMATS}).to_string(),
    })
}

fn hook_payloads_after_success() -> Vec<Value> {
    let (mut host, pane) = server_with_pane("codex", true);
    let request = run_until_job(&mut host, pane);
    finish_job(&mut host, request, Ok(job_output(3, 3)));
    host.extensions().hook_payloads(AFTER).iter().map(|text| serde_json::from_str(text).expect("JSON payload")).collect()
}

/// REQ: F-077
#[test]
fn ext_077_the_page_lists_exactly_the_formats_and_after_convert_points() {
    assert_eq!(
        point_rows(),
        [("formats".to_string(), "collect".to_string()), ("after-convert".to_string(), "hook (observe)".to_string())]
    );
}

/// REQ: F-077
#[test]
fn ext_077_the_page_states_the_plugin_is_replaceable() {
    assert!(page_text().contains("**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.session-convert\"]`"));
}

/// REQ: F-077
#[test]
fn ext_077_a_contributed_format_does_not_stop_a_conversion_from_starting() {
    let (mut host, pane) = server_with_pane("claude", true);
    host.extensions().contribute(
        FORMATS,
        "gemini",
        50,
        r#"{"id":"gemini","title":"Gemini CLI","direction":"both","service":"example.gemini/convert"}"#,
    );
    assert_eq!(frozen(&start_claude(&mut host, pane)), [pane]);
}

/// REQ: F-077
#[test]
fn ext_077_a_change_of_the_formats_point_is_handled_without_a_failure() {
    let (mut host, _pane) = server_with_pane("claude", true);
    host.extensions().contribute(
        FORMATS,
        "gemini",
        50,
        r#"{"id":"gemini","title":"Gemini CLI","direction":"both","service":"example.gemini/convert"}"#,
    );
    host.deliver_one(changed());
    assert!(host.failures().is_empty());
}

/// REQ: F-077
#[test]
fn ext_077_a_change_of_another_topic_is_ignored() {
    let (mut host, _pane) = server_with_pane("claude", true);
    let submissions = host.deliver_one(Event::TopicPublished(TopicPublishedEvent {
        topic: "other/topic".to_string(),
        from: PluginId("ilium.kernel".to_string()),
        payload_json: "{}".to_string(),
    }));
    assert!(submissions.is_empty());
}

/// REQ: F-077
#[test]
fn ext_077_a_contributed_format_with_a_bad_direction_is_rejected_by_the_point_schema() {
    let (mut host, _pane) = server_with_pane("claude", true);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        host.extensions().contribute(FORMATS, "bad", 50, r#"{"id":"bad","title":"Bad","direction":"sideways","service":"x"}"#);
    }));
    assert!(result.is_err() || !host.failures().is_empty());
}

/// REQ: F-077
#[test]
fn ext_077_after_convert_runs_once_after_a_success() {
    assert_eq!(hook_payloads_after_success().len(), 1);
}

/// REQ: F-077
#[test]
fn ext_077_after_convert_carries_from_to_messages_and_path() {
    let payload = hook_payloads_after_success().remove(0);
    assert_eq!(payload["from"], "codex");
    assert_eq!(payload["to"], "claude");
    assert_eq!(payload["messages"], 3);
    assert!(payload["path"].as_str().unwrap().ends_with(".jsonl"));
}

/// REQ: F-077
#[test]
fn ext_077_after_convert_payload_has_exactly_the_four_documented_fields() {
    let payload = hook_payloads_after_success().remove(0);
    let mut keys: Vec<&String> = payload.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["from", "messages", "path", "to"]);
}

/// REQ: F-077
#[test]
fn ext_077_after_convert_does_not_run_after_a_failure() {
    let (mut host, pane) = server_with_pane("codex", true);
    let request = run_until_job(&mut host, pane);
    let error = PluginError { code: ErrorCode::Io, message: "unreadable".into(), retry_after_ms: None };
    finish_job(&mut host, request, Err(error));
    assert!(host.extensions().hook_payloads(AFTER).is_empty());
}

/// REQ: F-077
#[test]
fn ext_077_after_convert_does_not_run_before_the_job_ends() {
    let (mut host, pane) = server_with_pane("codex", true);
    run_until_job(&mut host, pane);
    assert!(host.extensions().hook_payloads(AFTER).is_empty());
}

/// REQ: F-077
#[test]
fn ext_077_the_observer_answer_never_changes_the_outcome() {
    let (mut host, pane) = server_with_pane("codex", true);
    host.extensions().on_hook(AFTER, ilium_plugin_sdk::exports::HookReply::Refuse("no".to_string()));
    let request = run_until_job(&mut host, pane);
    let submissions = finish_job(&mut host, request, Ok(job_output(3, 3)));
    assert_eq!(created(&submissions).len(), 1);
}
