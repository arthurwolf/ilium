//! Twins of ilium-client/src/recap/pipeline_tests.rs: the whole recap run, pane by pane.

use serde_json::json;

use ilium_plugin_sdk::types::{ErrorCode, PluginError};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::goal_is_extracted_from_evidence_and_text_is_validated
#[test]
fn port_pipeline_goal_is_extracted_from_evidence_and_text_is_validated() {
    let goals = [("ship recap".to_string(), "done")];
    let (mut host, _, invocation) = start_agent_run_with_goals(&["user: ship it", "assistant: edited src/lib.rs"], &goals);
    host.fs().write("project:src/lib.rs", b"x");
    let records = finish_run(&mut host, &invocation, GOOD_ANSWER.as_bytes().to_vec());
    let record = first_recap(&records);
    assert_eq!(record["state"], "summarized");
    assert_eq!(record["goals"], json!(["Goal done: ship recap"]));
    assert_eq!(record["short"]["pointers"].as_array().map(Vec::len), Some(1));
    assert_eq!(record["short"]["rejected_pointers"], 1);
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::secrets_never_reach_the_prompt
#[test]
fn port_pipeline_secrets_never_reach_the_prompt() {
    let (mut host, _, _) = start_shell_run(&["export OPENAI_API_KEY=sk-abcdefghijklmnopqrstu", "password=hunter2hunter2"]);
    let job = host.jobs().list().remove(0);
    let text = String::from_utf8_lossy(&job.input).to_string();
    assert!(!text.contains("sk-abcdefghijklmnopqrstu"));
    assert!(!text.contains("hunter2hunter2"));
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::malformed_answer_is_a_labelled_per_pane_failure
#[test]
fn port_pipeline_malformed_answer_is_a_labelled_per_pane_failure() {
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    let records = finish_run(&mut host, &invocation, b"nonsense".to_vec());
    let record = first_recap(&records);
    assert_eq!(record["state"], "failed");
    assert_eq!(record["pane_name"], "build shell");
    assert!(record["failure"].as_str().unwrap().contains("not the expected JSON"));
    assert!(!of_type(&records, "warning").is_empty());
    assert_eq!(summary(&records)["failed"], 1);
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::provider_failure_on_one_pane_keeps_the_other_pane_text
#[test]
fn port_pipeline_provider_failure_on_one_pane_keeps_the_other_pane_text() {
    let mut host = activated_server();
    open_gate(&mut host);
    let first = host.tree().add(shell_pane("first shell"));
    let second = host.tree().add(shell_pane("second shell"));
    host.tree().set_screen(first, rows(&["$ one"]));
    host.tree().set_screen(second, rows(&["$ two"]));
    let invocation = host.cli().invoke(&["recap"], b"");
    let jobs = host.jobs().list();
    assert_eq!(jobs.len(), 2);
    host.jobs().finish(jobs[0].request, Err(PluginError { code: ErrorCode::Network, message: "boom".into(), retry_after_ms: None }));
    host.jobs().finish(jobs[1].request, Ok(answer(3, 3)));
    host.settle();
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    let recaps = of_type(&records, "recap");
    assert_eq!(recaps.len(), 2);
    let states: Vec<&str> = recaps.iter().map(|record| record["state"].as_str().unwrap()).collect();
    assert!(states.contains(&"failed") && states.contains(&"summarized"), "{states:?}");
    assert_eq!(summary(&records)["summarized"], 1);
    assert_eq!(summary(&records)["failed"], 1);
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::cancellation_before_start_returns_labelled_partial_results
#[test]
fn port_pipeline_cancellation_before_start_returns_labelled_partial_results() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("build shell"));
    host.tree().set_screen(pane, rows(&["$ ls"]));
    let invocation = host.cli().invoke(&["recap"], b"");
    host.cli().cancel(&invocation);
    host.settle();
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    assert_eq!(summary(&records)["cancelled"], true);
    let record = first_recap(&records);
    assert_eq!(record["state"], "failed");
    assert!(record["failure"].as_str().unwrap().contains("cancelled"));
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::session_change_during_inference_drops_text_as_stale
#[test]
fn port_pipeline_session_change_during_inference_drops_text_as_stale() {
    let (mut host, pane, invocation) = start_agent_run(&["user: work"]);
    host.tree().set_agent_session(pane, "session-2", rows(&["user: work"]));
    let record = first_recap(&finish_run(&mut host, &invocation, answer(3, 3)));
    assert_eq!(record["state"], "stale");
    assert!(record["short"].is_null());
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::final_fence_drops_text_when_session_changed_after_inference
#[test]
fn port_pipeline_final_fence_drops_text_when_the_pane_changed_after_inference() {
    let (mut host, pane, invocation) = start_shell_run(&["$ ls"]);
    let request = host.jobs().list()[0].request;
    host.tree().set_screen(pane, rows(&["$ pwd"]));
    host.jobs().finish(request, Ok(answer(3, 3)));
    host.settle();
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    let record = first_recap(&records);
    assert!(record["short"].is_null() || record["state"] != "summarized", "{record}");
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::vanished_pane_is_stale_not_summarized
#[test]
fn port_pipeline_vanished_pane_is_stale_not_summarized() {
    let (mut host, pane, invocation) = start_shell_run(&["$ ls"]);
    host.tree().remove(pane);
    let records = finish_run(&mut host, &invocation, answer(3, 3));
    assert_eq!(first_recap(&records)["state"], "stale");
    assert_eq!(summary(&records)["stale"], 1);
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::cache_reuses_summarized_text_for_unchanged_evidence_only
#[test]
fn port_pipeline_cache_reuses_summarized_text_for_unchanged_evidence_only() {
    let (mut host, pane, invocation) = start_shell_run(&["a"]);
    finish_run(&mut host, &invocation, answer(3, 3));
    let reused = host.cli().invoke(&["recap"], b"");
    host.settle();
    assert!(host.jobs().list().is_empty(), "unchanged evidence starts no job");
    let record = first_recap(&jsonl(&host.cli().refresh(&reused).stdout()));
    assert_eq!(record["state"], "summarized");
    assert_eq!(record["short"]["text"], words(3));
    host.tree().set_screen(pane, rows(&["b"]));
    host.cli().invoke(&["recap"], b"");
    assert_eq!(host.jobs().list().len(), 1, "changed evidence starts a job");
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::agent_without_verified_transcript_is_unavailable_and_never_calls_the_model
#[test]
fn port_pipeline_agent_without_verified_transcript_is_unavailable_and_never_calls_the_model() {
    let mut host = activated_server();
    open_gate(&mut host);
    host.tree().add(agent_pane("worker", "claude"));
    let invocation = host.cli().invoke(&["recap"], b"");
    assert!(host.jobs().list().is_empty());
    let record = first_recap(&jsonl(&host.cli().refresh(&invocation).stdout()));
    assert_eq!(record["state"], "unavailable");
    assert!(record["failure"].as_str().unwrap().contains("screen text is not used"));
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::empty_shell_screen_is_idle_and_skips_the_model
#[test]
fn port_pipeline_empty_shell_screen_is_idle_and_skips_the_model() {
    let (mut host, _, invocation) = start_shell_run(&["", "   "]);
    assert!(host.jobs().list().is_empty());
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    assert_eq!(first_recap(&records)["state"], "idle");
    assert_eq!(summary(&records)["idle"], 1);
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::unavailable_shell_screen_is_unavailable_and_skips_the_model
#[test]
fn port_pipeline_unavailable_shell_screen_is_unavailable_and_skips_the_model() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("build shell"));
    host.tree().set_screen_unavailable(pane);
    let invocation = host.cli().invoke(&["recap"], b"");
    assert!(host.jobs().list().is_empty());
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    assert_eq!(first_recap(&records)["state"], "unavailable");
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::permission_denied_gives_one_denied_record_per_pane_without_inference
#[test]
fn port_pipeline_permission_denied_gives_one_denied_record_per_pane_without_inference() {
    let mut host = activated_server();
    close_gate(&mut host);
    for index in 0..2 {
        let pane = host.tree().add(shell_pane(&format!("shell {index}")));
        host.tree().set_screen(pane, rows(&["$ ls"]));
    }
    let invocation = host.cli().invoke(&["recap"], b"");
    host.settle();
    assert!(host.jobs().list().is_empty());
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    let recaps = of_type(&records, "recap");
    assert_eq!(recaps.len(), 2);
    assert!(recaps.iter().all(|record| record["state"] == "denied"));
    assert_eq!(summary(&records)["denied"], 2);
    assert_eq!(host.screen_reads(), 0, "nothing is read when access is denied");
}

/// REQ: PORT:ilium-client/src/recap/pipeline_tests.rs::protected_project_directory_is_denied_without_reading
#[test]
fn port_pipeline_protected_project_directory_is_denied_without_reading() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("keys shell"));
    host.tree().set_pane_directory(pane, "project:.ssh");
    host.tree().set_screen(pane, rows(&["$ ls"]));
    let invocation = host.cli().invoke(&["recap"], b"");
    assert!(host.jobs().list().is_empty());
    let record = first_recap(&jsonl(&host.cli().refresh(&invocation).stdout()));
    assert_eq!(record["state"], "denied");
}
