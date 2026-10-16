//! The consent gate, the evidence rules, the record states and the model answer contract of
//! `ilium recap` (F-112, behaviour of the page "The consent gate" and "Evidence from ..." samples).

use serde_json::{json, Value};

use ilium_plugin_sdk::testing::FakeHost;

use crate::common::*;

fn words(count: usize) -> String {
    vec!["word"; count].join(" ")
}

fn answer(short_words: usize, long_words: usize) -> Vec<u8> {
    json!({"short": {"text": words(short_words), "pointers": []}, "long": {"text": words(long_words), "pointers": []}})
        .to_string()
        .into_bytes()
}

/// One shell pane with this screen, gate open; returns the host, the pane id and the invocation.
fn start_shell_run(screen: &[&str]) -> (FakeHost, u64, ilium_plugin_sdk::testing::fakes::cli::Invocation) {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("build shell"));
    host.tree().set_screen(pane, screen.iter().map(|row| row.to_string()).collect());
    let invocation = host.cli().invoke(&["recap"], b"");
    (host, pane, invocation)
}

/// Finishes the only job with `result`, lets the run end and returns the output records.
fn finish_run(host: &mut FakeHost, invocation: &ilium_plugin_sdk::testing::fakes::cli::Invocation, result: Vec<u8>) -> Vec<Value> {
    let request = host.jobs().list()[0].request;
    host.jobs().finish(request, Ok(result));
    host.settle();
    jsonl(&host.cli().refresh(invocation).stdout())
}

/// REQ: F-112
#[test]
fn f_112_closed_gate_gives_every_selected_pane_a_denied_record() {
    let mut host = activated_server();
    close_gate(&mut host);
    host.tree().add(agent_pane("a", "claude"));
    host.tree().add(shell_pane("s"));
    let (stdout, _, code) = run_cli(&mut host, &["recap"]);
    assert_eq!(code, Some(0));
    let records = jsonl(&stdout);
    let recaps = of_type(&records, "recap");
    assert_eq!(recaps.len(), 2);
    assert!(recaps.iter().all(|record| record["state"] == "denied"));
}

/// REQ: F-112
#[test]
fn f_112_closed_gate_summary_counts_all_panes_as_denied() {
    let mut host = activated_server();
    close_gate(&mut host);
    host.tree().add(shell_pane("a"));
    host.tree().add(shell_pane("b"));
    host.tree().add(shell_pane("c"));
    let (stdout, _, _) = run_cli(&mut host, &["recap"]);
    assert_eq!(
        summary(&jsonl(&stdout)),
        json!({"type":"summary","panes":3,"summarized":0,"idle":0,"denied":3,"unavailable":0,"stale":0,"failed":0,"cancelled":false})
    );
}

/// REQ: F-112
#[test]
fn f_112_closed_gate_reads_nothing_and_starts_no_job() {
    let mut host = activated_server();
    close_gate(&mut host);
    let pane = host.tree().add(shell_pane("s"));
    host.tree().set_screen(pane, vec!["secret screen".to_string()]);
    run_cli(&mut host, &["recap"]);
    assert!(host.jobs().list().is_empty());
    assert_eq!(host.screen_reads(), 0);
    assert_eq!(host.transcript_reads(), 0);
}

/// REQ: F-112
#[test]
fn f_112_unreadable_kernel_setting_means_closed() {
    let mut host = activated_server();
    host.set_kernel_settings_unreadable("inference");
    host.set_onboarding_finished(true);
    host.tree().add(shell_pane("s"));
    let (stdout, _, code) = run_cli(&mut host, &["recap"]);
    assert_eq!(code, Some(0));
    assert_eq!(of_type(&jsonl(&stdout), "recap")[0]["state"], "denied");
}

/// REQ: F-112
#[test]
fn f_112_automatic_ai_that_is_not_a_boolean_means_closed() {
    for value in [json!("true"), json!(1), json!(null)] {
        let mut host = activated_server();
        host.set_kernel_settings("inference", json!({"automatic_ai": value}));
        host.set_onboarding_finished(true);
        host.tree().add(shell_pane("s"));
        let (stdout, _, _) = run_cli(&mut host, &["recap"]);
        assert_eq!(of_type(&jsonl(&stdout), "recap")[0]["state"], "denied", "value {value}");
    }
}

/// REQ: F-112
#[test]
fn f_112_open_ai_with_unfinished_onboarding_gives_denied_records() {
    let mut host = activated_server();
    host.set_kernel_settings("inference", json!({"automatic_ai": true}));
    host.set_onboarding_finished(false);
    host.tree().add(shell_pane("s"));
    let (stdout, _, _) = run_cli(&mut host, &["recap"]);
    assert_eq!(of_type(&jsonl(&stdout), "recap")[0]["state"], "denied");
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-112
#[test]
fn f_112_server_asks_the_client_the_onboarding_state_request_with_empty_object_payload() {
    let mut host = activated_server();
    host.set_kernel_settings("inference", json!({"automatic_ai": true}));
    host.tree().add(shell_pane("s"));
    run_cli(&mut host, &["recap"]);
    let requests = host.client_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].kind, "onboarding-state");
    assert_eq!(requests[0].payload_json, "{}");
    assert_eq!(requests[0].timeout_ms, 2_000);
}

/// REQ: F-112
#[test]
fn f_112_client_answers_onboarding_state_with_the_finished_flag() {
    for finished in [true, false] {
        let mut client = activated_client();
        client.set_onboarding_finished(finished);
        let reply = client.answer_client_request("onboarding-state", "{}");
        let parsed: Value = serde_json::from_str(&reply.expect("client answers")).expect("JSON");
        assert_eq!(parsed, json!({"finished": finished}));
    }
}

/// REQ: F-112
#[test]
fn f_112_unanswered_onboarding_request_within_two_seconds_means_closed() {
    let mut host = activated_server();
    host.set_kernel_settings("inference", json!({"automatic_ai": true}));
    host.leave_client_requests_unanswered();
    host.tree().add(shell_pane("s"));
    let invocation = host.cli().invoke(&["recap"], b"");
    host.advance_clock(2_001);
    host.settle();
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    assert_eq!(of_type(&records, "recap")[0]["state"], "denied");
}

/// REQ: F-112
#[test]
fn f_112_agent_pane_without_a_verified_session_is_unavailable() {
    let mut host = activated_server();
    open_gate(&mut host);
    host.tree().add(agent_pane("worker", "claude"));
    let (stdout, _, code) = run_cli(&mut host, &["recap"]);
    assert_eq!(code, Some(0));
    assert_eq!(of_type(&jsonl(&stdout), "recap")[0]["state"], "unavailable");
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-112
#[test]
fn f_112_agent_pane_never_falls_back_to_the_screen() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(agent_pane("worker", "claude"));
    host.tree().set_screen(pane, vec!["the agent is fixing the parser".to_string()]);
    let (stdout, _, _) = run_cli(&mut host, &["recap"]);
    assert_eq!(of_type(&jsonl(&stdout), "recap")[0]["state"], "unavailable");
    assert_eq!(host.screen_reads(), 0);
}

/// REQ: F-112
#[test]
fn f_112_agent_with_a_verified_session_reads_a_bounded_transcript_and_starts_one_job() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(agent_pane("worker", "claude"));
    host.tree().set_agent_session(pane, "session-1", vec!["user: fix the parser".to_string(), "assistant: done".to_string()]);
    run_cli(&mut host, &["recap"]);
    let query = host.transcript_queries().pop().expect("a transcript query");
    assert_eq!(query.last_turns, 40);
    assert_eq!(query.max_bytes, 512 * 1024);
    assert_eq!(host.jobs().list().len(), 1);
}

/// REQ: F-112
#[test]
fn f_112_goal_lines_come_from_the_transcript_goal_not_from_the_model() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(agent_pane("worker", "claude"));
    host.tree().set_agent_session(pane, "session-1", vec!["user: /goal Finish the parser refactor".to_string()]);
    let invocation = host.cli().invoke(&["recap"], b"");
    let records = finish_run(&mut host, &invocation, answer(5, 20));
    assert_eq!(of_type(&records, "recap")[0]["goals"], json!(["Goal active: Finish the parser refactor"]));
}

/// REQ: F-112
#[test]
fn f_112_empty_shell_screen_is_idle_not_failed_and_starts_no_job() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("s"));
    host.tree().set_screen(pane, vec!["   ".to_string(), String::new()]);
    let (stdout, _, _) = run_cli(&mut host, &["recap"]);
    assert_eq!(of_type(&jsonl(&stdout), "recap")[0]["state"], "idle");
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-112
#[test]
fn f_112_shell_screen_is_read_visible_part_with_at_most_40_rows() {
    let (host, _, _) = start_shell_run(&["$ cargo test"]);
    let read = host.screen_requests().pop().expect("a screen read");
    assert_eq!(read.rows, 40);
    assert!(read.visible_only);
}

/// REQ: F-112
#[test]
fn f_112_shell_screen_that_cannot_be_read_is_unavailable() {
    let mut host = activated_server();
    open_gate(&mut host);
    let pane = host.tree().add(shell_pane("s"));
    host.tree().set_screen_unavailable(pane);
    let (stdout, _, _) = run_cli(&mut host, &["recap"]);
    assert_eq!(of_type(&jsonl(&stdout), "recap")[0]["state"], "unavailable");
}

/// REQ: F-112
#[test]
fn f_112_shell_evidence_is_capped_at_64_kib() {
    let big = "x".repeat(70 * 1024);
    let (mut host, _, _) = start_shell_run(&[&big]);
    let job = host.jobs().list().pop().expect("a job");
    let input: Value = serde_json::from_slice(&job.input).expect("job input is JSON");
    assert!(input["evidence"].as_str().expect("evidence").len() <= 64 * 1024);
}

/// REQ: F-112
#[test]
fn f_112_shell_evidence_just_under_the_cap_is_sent_whole() {
    let rows = "y".repeat(64 * 1024 - 1);
    let (mut host, _, _) = start_shell_run(&[&rows]);
    let job = host.jobs().list().pop().expect("a job");
    let input: Value = serde_json::from_slice(&job.input).expect("job input is JSON");
    assert!(input["evidence"].as_str().expect("evidence").contains(&rows));
}

/// REQ: F-112
#[test]
fn f_112_gate_open_shell_with_text_starts_exactly_one_recap_pane_job() {
    let (mut host, _, _) = start_shell_run(&["$ cargo test", "ok"]);
    let jobs = host.jobs().list();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].kind, "recap-pane");
}

/// REQ: F-112
#[test]
fn f_112_secret_values_are_redacted_before_the_text_reaches_a_job_but_key_names_stay() {
    let (mut host, _, _) = start_shell_run(&["export API_KEY=sk-live-0123456789abcdef", "password=hunter2"]);
    let job = host.jobs().list().pop().expect("a job");
    let text = String::from_utf8(job.input).expect("UTF-8");
    assert!(!text.contains("sk-live-0123456789abcdef"));
    assert!(!text.contains("hunter2"));
    assert!(text.contains("[redacted]"));
    assert!(text.contains("API_KEY"));
    assert!(text.contains("password"));
}

/// REQ: F-112
#[test]
fn f_112_ordinary_prose_passes_the_redaction_untouched() {
    let (mut host, _, _) = start_shell_run(&["the parser refactor is in progress"]);
    let job = host.jobs().list().pop().expect("a job");
    let text = String::from_utf8(job.input).expect("UTF-8");
    assert!(text.contains("the parser refactor is in progress"));
    assert!(!text.contains("[redacted]"));
}

/// REQ: F-112
#[test]
fn f_112_a_valid_model_answer_gives_a_summarized_record_with_both_texts() {
    let (mut host, pane, invocation) = start_shell_run(&["$ cargo test"]);
    let records = finish_run(&mut host, &invocation, answer(10, 100));
    let record = &of_type(&records, "recap")[0];
    assert_eq!(record["state"], "summarized");
    assert_eq!(record["pane_id"], pane.to_string());
    assert_eq!(record["pane_name"], "build shell");
    assert_eq!(record["short"]["text"], words(10));
    assert_eq!(record["long"]["text"], words(100));
    assert_eq!(record["short"]["rejected_pointers"], 0);
    assert!(record["failure"].is_null());
    assert_eq!(summary(&records)["summarized"], 1);
}

/// REQ: F-112
#[test]
fn f_112_a_finished_run_exits_zero() {
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    finish_run(&mut host, &invocation, answer(3, 3));
    assert_eq!(host.cli().refresh(&invocation).exit_code(), Some(0));
}

/// REQ: F-112
#[test]
fn f_112_short_answer_at_40_words_is_accepted() {
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    let records = finish_run(&mut host, &invocation, answer(40, 41));
    assert_eq!(of_type(&records, "recap")[0]["state"], "summarized");
}

/// REQ: F-112
#[test]
fn f_112_short_answer_at_41_words_fails_instead_of_being_truncated() {
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    let records = finish_run(&mut host, &invocation, answer(41, 41));
    let record = &of_type(&records, "recap")[0];
    assert_eq!(record["state"], "failed");
    assert!(record["short"].is_null());
    assert_eq!(summary(&records)["failed"], 1);
}

/// REQ: F-112
#[test]
fn f_112_long_answer_at_250_words_is_accepted_and_at_251_fails() {
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    let ok = finish_run(&mut host, &invocation, answer(10, 250));
    assert_eq!(of_type(&ok, "recap")[0]["state"], "summarized");
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    let too_long = finish_run(&mut host, &invocation, answer(10, 251));
    assert_eq!(of_type(&too_long, "recap")[0]["state"], "failed");
}

/// REQ: F-112
#[test]
fn f_112_model_answer_that_is_not_json_fails_the_pane() {
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    let records = finish_run(&mut host, &invocation, b"no json here".to_vec());
    assert_eq!(of_type(&records, "recap")[0]["state"], "failed");
}

/// REQ: F-112
#[test]
fn f_112_model_answer_missing_the_long_text_fails_the_pane() {
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    let missing = json!({"short": {"text": "only short"}}).to_string().into_bytes();
    let records = finish_run(&mut host, &invocation, missing);
    assert_eq!(of_type(&records, "recap")[0]["state"], "failed");
}

/// REQ: F-112
#[test]
fn f_112_model_answer_inside_a_code_fence_is_still_parsed() {
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    let fenced = format!("```json\n{}\n```", String::from_utf8(answer(4, 8)).expect("UTF-8"));
    let records = finish_run(&mut host, &invocation, fenced.into_bytes());
    assert_eq!(of_type(&records, "recap")[0]["state"], "summarized");
}

/// REQ: F-112
#[test]
fn f_112_failed_pane_also_writes_a_warning_record_naming_the_pane_and_state() {
    let (mut host, pane, invocation) = start_shell_run(&["$ ls"]);
    let records = finish_run(&mut host, &invocation, b"nope".to_vec());
    let warnings = of_type(&records, "warning");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["pane_id"], pane.to_string());
    assert_eq!(warnings[0]["state"], "failed");
}

/// REQ: F-112
#[test]
fn f_112_provider_error_fails_the_pane_and_the_run_still_exits_zero() {
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    let request = host.jobs().list()[0].request;
    host.jobs().finish(request, Err(ilium_plugin_sdk::types::PluginError {
        code: ilium_plugin_sdk::types::ErrorCode::Network,
        message: "provider timed out".to_string(),
        retry_after_ms: None,
    }));
    host.settle();
    let done = host.cli().refresh(&invocation);
    assert_eq!(done.exit_code(), Some(0));
    assert_eq!(summary(&jsonl(&done.stdout()))["failed"], 1);
}

/// REQ: F-112
#[test]
fn f_112_pane_closed_while_the_model_runs_gives_a_stale_record() {
    let (mut host, pane, invocation) = start_shell_run(&["$ ls"]);
    host.tree().remove(pane);
    let records = finish_run(&mut host, &invocation, answer(5, 5));
    assert_eq!(of_type(&records, "recap")[0]["state"], "stale");
    assert_eq!(summary(&records)["stale"], 1);
}

/// REQ: F-112
#[test]
fn f_112_pointer_not_named_in_the_evidence_is_dropped_and_counted() {
    let (mut host, _, invocation) = start_shell_run(&["$ cargo test"]);
    host.fs().write("project:src/ghost.rs", b"fn main() {}");
    let body = json!({
        "short": {"text": "tests ran", "pointers": [{"path": "src/ghost.rs", "note": "not in the evidence"}]},
        "long": {"text": "tests ran", "pointers": []}
    });
    let records = finish_run(&mut host, &invocation, body.to_string().into_bytes());
    let short = &of_type(&records, "recap")[0]["short"];
    assert_eq!(short["pointers"], json!([]));
    assert_eq!(short["rejected_pointers"], 1);
}

/// REQ: F-112
#[test]
fn f_112_pointer_in_the_evidence_that_exists_under_the_project_is_kept() {
    let (mut host, _, invocation) = start_shell_run(&["error in src/parser.rs line 3"]);
    host.fs().write("project:src/parser.rs", b"fn parse() {}");
    let body = json!({
        "short": {"text": "parser fails", "pointers": [{"path": "src/parser.rs", "note": "parser entry point"}]},
        "long": {"text": "parser fails", "pointers": []}
    });
    let records = finish_run(&mut host, &invocation, body.to_string().into_bytes());
    let short = &of_type(&records, "recap")[0]["short"];
    assert_eq!(short["pointers"], json!([{"path": "src/parser.rs", "note": "parser entry point"}]));
    assert_eq!(short["rejected_pointers"], 0);
}

/// REQ: F-112
#[test]
fn f_112_pointer_named_in_the_evidence_but_missing_on_disk_is_dropped() {
    let (mut host, _, invocation) = start_shell_run(&["see src/gone.rs"]);
    let body = json!({
        "short": {"text": "see file", "pointers": [{"path": "src/gone.rs", "note": "removed"}]},
        "long": {"text": "see file", "pointers": []}
    });
    let records = finish_run(&mut host, &invocation, body.to_string().into_bytes());
    assert_eq!(of_type(&records, "recap")[0]["short"]["rejected_pointers"], 1);
}

/// REQ: F-112
#[test]
fn f_112_pointer_into_a_denied_credential_folder_is_dropped_even_when_named_and_present() {
    let (mut host, _, invocation) = start_shell_run(&["cat .ssh/id_ed25519"]);
    host.fs().write("project:.ssh/id_ed25519", b"key");
    let body = json!({
        "short": {"text": "read a key", "pointers": [{"path": ".ssh/id_ed25519", "note": "key"}]},
        "long": {"text": "read a key", "pointers": []}
    });
    let records = finish_run(&mut host, &invocation, body.to_string().into_bytes());
    assert_eq!(of_type(&records, "recap")[0]["short"]["rejected_pointers"], 1);
}

/// REQ: F-112
#[test]
fn f_112_summary_record_has_exactly_the_documented_keys() {
    let mut host = activated_server();
    close_gate(&mut host);
    host.tree().add(shell_pane("s"));
    let (stdout, _, _) = run_cli(&mut host, &["recap"]);
    let record = summary(&jsonl(&stdout));
    let mut keys: Vec<&String> = record.as_object().expect("object").keys().collect();
    keys.sort();
    assert_eq!(keys, ["cancelled", "denied", "failed", "idle", "panes", "stale", "summarized", "type", "unavailable"]);
}

/// REQ: F-112
#[test]
fn f_112_cancelling_the_run_marks_unfinished_panes_failed_and_the_summary_cancelled() {
    let (mut host, _, invocation) = start_shell_run(&["$ ls"]);
    host.cli().cancel(&invocation);
    host.settle();
    let done = host.cli().refresh(&invocation);
    let summary = summary(&jsonl(&done.stdout()));
    assert_eq!(summary["cancelled"], true);
    assert_eq!(summary["failed"], 1);
    assert_eq!(done.exit_code(), Some(0));
}

/// REQ: F-112
#[test]
fn f_112_run_timeout_reports_panes_still_running_as_failed_and_keeps_finished_text() {
    let mut host = activated_server();
    open_gate(&mut host);
    for name in ["fast", "slow"] {
        let pane = host.tree().add(shell_pane(name));
        host.tree().set_screen(pane, vec![format!("$ {name}")]);
    }
    let invocation = host.cli().invoke(&["recap", "--timeout-seconds", "1"], b"");
    let first = host.jobs().list()[0].request;
    host.jobs().finish(first, Ok(answer(3, 3)));
    host.advance_clock(1_001);
    host.settle();
    let records = jsonl(&host.cli().refresh(&invocation).stdout());
    let states: Vec<&str> = of_type(&records, "recap").iter().map(|r| r["state"].as_str().expect("state")).collect();
    assert_eq!(states, ["summarized", "failed"]);
}

/// REQ: F-112
#[test]
fn f_112_max_concurrency_limits_the_jobs_running_at_the_same_time() {
    let mut host = activated_server();
    open_gate(&mut host);
    for index in 0..6 {
        let pane = host.tree().add(shell_pane(&format!("shell {index}")));
        host.tree().set_screen(pane, vec![format!("$ job {index}")]);
    }
    host.cli().invoke(&["recap", "--max-concurrency", "2"], b"");
    assert_eq!(host.jobs().list().len(), 6, "every pane is one job; extra jobs wait in the queue");
    assert_eq!(host.running_job_limit(), 2);
}
