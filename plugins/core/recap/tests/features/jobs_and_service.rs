//! The `recap-pane` job runner, the `ilium.core.recap/recap` service and the client side (F-112).

use serde_json::{json, Value};

use ilium_plugin_sdk::exports::ServiceReply;
use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::ErrorCode;

use crate::common::*;

const SERVICE: &str = "ilium.core.recap/recap";

fn session_with_recap() -> FakeSession {
    let mut session = FakeSession::new();
    session.install_core("recap").start();
    session
}

fn call(
    session: &mut FakeSession,
    payload: Value,
) -> Result<ServiceReply, ilium_plugin_sdk::types::PluginError> {
    session.call_service(
        "com.example.caller",
        SERVICE,
        "^0.1.0",
        &payload.to_string(),
    )
}

fn done_json(reply: Result<ServiceReply, ilium_plugin_sdk::types::PluginError>) -> Value {
    match reply.expect("service answers") {
        ServiceReply::Done(text) => serde_json::from_str(&text).expect("reply is JSON"),
        ServiceReply::Deferred => panic!("expected an immediate reply"),
    }
}

/// REQ: F-112
#[test]
fn f_112_service_get_with_no_panes_answers_an_empty_recaps_array() {
    let mut session = session_with_recap();
    let reply = done_json(call(&mut session, json!({"panes": []})));
    assert_eq!(reply, json!({"recaps": []}));
}

/// REQ: F-112
#[test]
fn f_112_service_get_without_panes_is_invalid_argument() {
    let mut session = session_with_recap();
    let error = call(&mut session, json!({"length": "short"})).expect_err("panes is required");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-112
#[test]
fn f_112_service_get_with_a_non_integer_pane_is_invalid_argument() {
    let mut session = session_with_recap();
    let error = call(&mut session, json!({"panes": ["a"]})).expect_err("panes are integers");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-112
#[test]
fn f_112_service_get_with_length_medium_is_invalid_argument() {
    let mut session = session_with_recap();
    let error =
        call(&mut session, json!({"panes": [], "length": "medium"})).expect_err("bad length");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-112
#[test]
fn f_112_service_get_accepts_each_length_value() {
    let mut session = session_with_recap();
    for length in ["short", "long", "both"] {
        assert!(
            call(&mut session, json!({"panes": [], "length": length})).is_ok(),
            "length {length}"
        );
    }
}

/// REQ: F-112
#[test]
fn f_112_service_get_for_a_pane_with_a_closed_gate_answers_one_denied_recap() {
    let mut session = session_with_recap();
    let pane = session.server(PLUGIN_ID).tree().add(shell_pane("s"));
    let reply = done_json(call(&mut session, json!({"panes": [pane]})));
    let recaps = reply["recaps"].as_array().expect("array");
    assert_eq!(recaps.len(), 1);
    assert_eq!(recaps[0]["state"], "denied");
    assert_eq!(recaps[0]["pane_id"], pane.to_string());
}

/// REQ: F-112
#[test]
fn f_112_service_get_for_an_unknown_pane_does_not_fail_the_call() {
    let mut session = session_with_recap();
    let reply = call(&mut session, json!({"panes": [9999]}));
    assert!(reply.is_ok(), "{reply:?}");
}

/// REQ: F-112
#[test]
fn f_112_service_is_not_found_without_the_recap_plugin() {
    let mut session = FakeSession::new();
    session.start();
    let error = call(&mut session, json!({"panes": []})).expect_err("no provider");
    assert_eq!(error.code, ErrorCode::NotFound);
}

/// REQ: F-112
#[test]
fn f_112_job_with_input_that_is_not_json_is_invalid_argument() {
    let mut host = activated_server();
    let error = host
        .run_job("recap-pane", b"not json")
        .expect_err("bad input");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-112
#[test]
fn f_112_job_asks_the_model_once_for_both_lengths_with_purpose_recap() {
    let mut host = activated_server();
    host.set_inference_answer(json!({"short": {"text": "a"}, "long": {"text": "b"}}).to_string());
    let input = json!({"evidence": "$ cargo test"}).to_string();
    host.run_job("recap-pane", input.as_bytes())
        .expect("job succeeds");
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].1.purpose, "recap");
    assert_eq!(requests[0].1.messages[0].role, "system");
    assert_eq!(requests[0].1.messages[1].role, "user");
    assert_eq!(requests[0].1.messages[1].content, "$ cargo test");
    assert!(requests[0].1.json_schema.is_some());
    assert!(requests[0].1.max_output_tokens.is_none());
}

/// REQ: F-112
#[test]
fn f_112_job_returns_the_completion_text_bytes() {
    let mut host = activated_server();
    let answer = json!({"short": {"text": "a"}, "long": {"text": "b"}}).to_string();
    host.set_inference_answer(answer.clone());
    let bytes = host
        .run_job(
            "recap-pane",
            json!({"evidence": "x"}).to_string().as_bytes(),
        )
        .expect("job succeeds");
    assert_eq!(bytes, answer.into_bytes());
}

/// REQ: F-112
#[test]
fn f_112_job_reports_progress_10_then_100() {
    let mut host = activated_server();
    host.set_inference_answer("{}".to_string());
    host.run_job(
        "recap-pane",
        json!({"evidence": "x"}).to_string().as_bytes(),
    )
    .expect("job succeeds");
    assert_eq!(host.reported_progress(), [10.0, 100.0]);
}

/// REQ: F-112
#[test]
fn f_112_cancelled_job_ends_with_the_cancelled_error() {
    let mut host = activated_server();
    host.cancel_current_job();
    let error = host
        .run_job(
            "recap-pane",
            json!({"evidence": "x"}).to_string().as_bytes(),
        )
        .expect_err("cancelled");
    assert_eq!(error.code, ErrorCode::Cancelled);
}

/// REQ: F-112
#[test]
fn f_112_job_waits_at_most_55_seconds_for_the_model() {
    let mut host = activated_server();
    host.set_inference_answer("{}".to_string());
    host.run_job(
        "recap-pane",
        json!({"evidence": "x"}).to_string().as_bytes(),
    )
    .expect("job succeeds");
    assert_eq!(host.inference_wait_ms(), vec![55_000]);
}

/// REQ: F-112
#[test]
fn f_112_client_side_activates_and_answers_only_the_onboarding_state_kind() {
    let mut client = activated_client();
    assert!(client
        .answer_client_request("onboarding-state", "{}")
        .is_some());
    assert!(client.answer_client_request("other-kind", "{}").is_none());
}

/// REQ: F-112
#[test]
fn f_112_client_side_submits_nothing_for_an_unrelated_event() {
    let mut client = activated_client();
    let submissions = client.deliver_one(ilium_plugin_sdk::types::Event::ThemeChanged);
    assert!(submissions.is_empty());
}

/// REQ: F-112
#[test]
fn f_112_fact_sources_are_listed_again_after_the_extensions_changed_topic() {
    let mut host = activated_server();
    host.set_extension_entries("ilium.core.recap/fact-sources", vec![json!({"id": "branch-ticket", "title": "Branch ticket", "service": "example.tickets/for-pane"}).to_string()]);
    host.deliver_one(ilium_plugin_sdk::types::Event::TopicPublished(
        ilium_plugin_sdk::types::TopicPublishedEvent {
            topic: "ilium.extensions/changed".to_string(),
            from: ilium_plugin_sdk::types::PluginId("ilium.kernel".to_string()),
            payload_json: "{}".to_string(),
        },
    ));
    host.settle();
    assert_eq!(
        host.extension_list_calls("ilium.core.recap/fact-sources"),
        2
    );
}

/// REQ: F-112
#[test]
fn f_112_a_fact_source_failure_never_stops_the_recap() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("s"));
    host.tree().set_screen(pane, vec!["$ ls".to_string()]);
    host.set_extension_entries(
        "ilium.core.recap/fact-sources",
        vec![json!({"id": "x", "title": "Broken", "service": "example.broken/facts"}).to_string()],
    );
    host.fail_service("example.broken/facts");
    host.cli().invoke(&["recap"], b"");
    assert_eq!(host.jobs().list().len(), 1);
}

/// REQ: F-112
#[test]
fn f_112_fact_source_answers_are_added_to_the_evidence_with_the_entry_title() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("s"));
    host.tree().set_screen(pane, vec!["$ ls".to_string()]);
    host.set_extension_entries("ilium.core.recap/fact-sources", vec![json!({"id": "branch-ticket", "title": "Branch ticket", "service": "example.tickets/for-pane"}).to_string()]);
    host.set_service_reply("example.tickets/for-pane", "TICKET-42".to_string());
    host.cli().invoke(&["recap"], b"");
    let job = host.jobs().list().pop().expect("a job");
    assert!(String::from_utf8(job.input)
        .expect("UTF-8")
        .contains("Branch ticket: TICKET-42"));
}

/// REQ: F-112
#[test]
fn f_112_before_recap_hook_reply_replaces_the_text_of_the_record() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("s"));
    host.tree().set_screen(pane, vec!["$ ls".to_string()]);
    host.set_hook_reply(
        "ilium.core.recap/before-recap",
        ilium_plugin_sdk::exports::HookReply::Replace(json!({"text": "Redacted text"}).to_string()),
    );
    let invocation = host.cli().invoke(&["recap", "--length", "short"], b"");
    let request = host.jobs().list()[0].request;
    host.jobs().finish(
        request,
        Ok(
            json!({"short": {"text": "Customer Acme did x"}, "long": {"text": "y"}})
                .to_string()
                .into_bytes(),
        ),
    );
    host.settle();
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    assert_eq!(
        of_type(&records, "recap")[0]["short"]["text"],
        "Redacted text"
    );
}

/// REQ: F-112
#[test]
fn f_112_before_recap_runs_once_per_length_with_pane_kind_and_text() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("s"));
    host.tree().set_screen(pane, vec!["$ ls".to_string()]);
    let invocation = host.cli().invoke(&["recap"], b"");
    let request = host.jobs().list()[0].request;
    host.jobs().finish(
        request,
        Ok(json!({"short": {"text": "s1"}, "long": {"text": "l1"}})
            .to_string()
            .into_bytes()),
    );
    host.settle();
    let _ = host.cli().refresh(&invocation);
    let payloads: Vec<Value> = host
        .hook_runs("ilium.core.recap/before-recap")
        .iter()
        .map(|text| serde_json::from_str(text).expect("JSON"))
        .collect();
    assert_eq!(payloads.len(), 2);
    assert_eq!(
        payloads[0],
        json!({"pane": pane, "kind": "short", "text": "s1"})
    );
    assert_eq!(
        payloads[1],
        json!({"pane": pane, "kind": "long", "text": "l1"})
    );
}

/// REQ: F-112
#[test]
fn f_112_a_failing_before_recap_hook_keeps_the_original_text() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("s"));
    host.tree().set_screen(pane, vec!["$ ls".to_string()]);
    host.fail_hook("ilium.core.recap/before-recap");
    let invocation = host.cli().invoke(&["recap", "--length", "short"], b"");
    let request = host.jobs().list()[0].request;
    host.jobs().finish(
        request,
        Ok(json!({"short": {"text": "kept"}, "long": {"text": "y"}})
            .to_string()
            .into_bytes()),
    );
    host.settle();
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    assert_eq!(of_type(&records, "recap")[0]["short"]["text"], "kept");
    assert_eq!(of_type(&records, "recap")[0]["state"], "summarized");
}
