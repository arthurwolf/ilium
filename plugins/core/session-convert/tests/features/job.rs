//! F-077: Codex to Claude Code runs as the job `codex-to-claude` (page sample "Codex to Claude Code as a job").
//! It keeps user prompts, assistant text and tool calls with results, drops reasoning and token events,
//! reports progress every 50 lines and stops when cancelled.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ErrorCode, FsPath, Intent, Outcome, PluginSide, RequestRef, RequestResult,
};
use serde_json::{json, Value};

use crate::common::*;
use crate::start::{server_with_pane, start_message};

const ROLLOUT: &str = "2030/01/01/rollout-11111111-1111-4111-8111-111111111111.jsonl";

fn user_line(text: &str) -> String {
    json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]}})
        .to_string()
}

fn assistant_line(text: &str) -> String {
    json!({"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}})
        .to_string()
}

fn call_line() -> String {
    json!({"type": "response_item", "payload": {"type": "function_call", "name": "shell", "arguments": "{\"command\":[\"ls\"]}", "call_id": "c1"}})
        .to_string()
}

fn result_line() -> String {
    json!({"type": "response_item", "payload": {"type": "function_call_output", "call_id": "c1", "output": "{\"output\":\"a.txt\\n\"}"}})
        .to_string()
}

fn reasoning_line() -> String {
    json!({"type": "response_item", "payload": {"type": "reasoning", "summary": [{"type": "summary_text", "text": "SECRET-THOUGHT"}]}})
        .to_string()
}

fn token_line() -> String {
    json!({"type": "event_msg", "payload": {"type": "token_count", "info": null}}).to_string()
}

fn input() -> Vec<u8> {
    json!({"rollout": ROLLOUT}).to_string().into_bytes()
}

fn host_with_rollout(lines: &[String]) -> FakeHost {
    let mut host = server_host();
    host.fs().write(
        FsPath {
            scope: "agent-sessions:codex".into(),
            path: ROLLOUT.into(),
        },
        lines.join("\n").as_bytes(),
    );
    host
}

fn standard_lines() -> Vec<String> {
    vec![
        user_line("hello there"),
        assistant_line("hi, how can I help"),
        call_line(),
        result_line(),
        reasoning_line(),
        token_line(),
    ]
}

fn output_of(host: &mut FakeHost) -> (String, u64, u64) {
    let bytes = host
        .run_job("codex-to-claude", input())
        .expect("job result");
    let value: Value = serde_json::from_slice(&bytes).expect("JSON output");
    (
        value["claude_jsonl"]
            .as_str()
            .expect("claude_jsonl")
            .to_string(),
        value["kept"].as_u64().expect("kept"),
        value["dropped"].as_u64().expect("dropped"),
    )
}

/// REQ: F-077
#[test]
fn f_077_job_carries_over_user_prompts_and_assistant_text() {
    let mut host = host_with_rollout(&standard_lines());
    let (jsonl, _, _) = output_of(&mut host);
    assert!(jsonl.contains("hello there"));
    assert!(jsonl.contains("hi, how can I help"));
}

/// REQ: F-077
#[test]
fn f_077_job_carries_over_shell_calls_with_their_results() {
    let mut host = host_with_rollout(&standard_lines());
    let (jsonl, _, _) = output_of(&mut host);
    // manual does not pin the Claude block shape; pin one separate JSON line per call and result.
    let lines: Vec<Value> = jsonl
        .split('\n')
        .map(|line| serde_json::from_str(line).expect("each transcript line is JSON"))
        .collect();
    let call: Vec<&Value> = lines
        .iter()
        .filter(|line| line.to_string().contains("\"ls\""))
        .collect();
    let result: Vec<&Value> = lines
        .iter()
        .filter(|line| line.to_string().contains("a.txt"))
        .collect();
    assert_eq!(call.len(), 1, "{jsonl}");
    assert_eq!(result.len(), 1, "{jsonl}");
    assert_ne!(call[0], result[0], "call and result are separate lines");
}

/// REQ: F-077
#[test]
fn f_077_job_drops_reasoning() {
    let mut host = host_with_rollout(&standard_lines());
    let (jsonl, _, _) = output_of(&mut host);
    assert!(!jsonl.contains("SECRET-THOUGHT"));
}

/// REQ: F-077
#[test]
fn f_077_job_drops_token_events_and_counts_two_dropped_lines() {
    let mut host = host_with_rollout(&standard_lines());
    let (jsonl, _, dropped) = output_of(&mut host);
    assert!(!jsonl.contains("token_count"));
    assert_eq!(dropped, 2);
}

/// REQ: F-077
#[test]
fn f_077_job_kept_counts_the_lines_of_the_claude_transcript() {
    let mut host = host_with_rollout(&standard_lines());
    let (jsonl, kept, _) = output_of(&mut host);
    assert_eq!(kept, 4);
    assert_eq!(jsonl.split('\n').count() as u64, kept);
}

/// REQ: F-077
#[test]
fn f_077_job_every_output_line_is_json() {
    let mut host = host_with_rollout(&standard_lines());
    let (jsonl, _, _) = output_of(&mut host);
    for line in jsonl.split('\n') {
        serde_json::from_str::<Value>(line).unwrap_or_else(|error| panic!("line {line}: {error}"));
    }
}

/// REQ: F-077
#[test]
fn f_077_job_a_line_that_is_not_json_is_dropped() {
    let mut lines = standard_lines();
    lines.push("this is not json".to_string());
    let mut host = host_with_rollout(&lines);
    let (_, kept, dropped) = output_of(&mut host);
    assert_eq!((kept, dropped), (4, 3));
}

/// REQ: F-077
#[test]
fn f_077_job_preserves_the_order_of_the_conversation() {
    let mut host = host_with_rollout(&standard_lines());
    let (jsonl, _, _) = output_of(&mut host);
    let first = jsonl.find("hello there").unwrap();
    let second = jsonl.find("hi, how can I help").unwrap();
    assert!(first < second);
}

/// REQ: F-077
#[test]
fn f_077_job_reports_progress_every_fifty_lines() {
    let lines: Vec<String> = (0..120)
        .map(|index| user_line(&format!("prompt {index}")))
        .collect();
    let mut host = host_with_rollout(&lines);
    output_of(&mut host);
    let reported = host.jobs().reported_progress();
    let expected = [0.0_f32, 100.0 * 50.0 / 120.0, 100.0 * 100.0 / 120.0];
    assert_eq!(reported.len(), expected.len(), "{reported:?}");
    for (got, want) in reported.iter().zip(expected) {
        assert!((got - want).abs() < 0.01, "{got} vs {want}");
    }
}

/// REQ: F-077
#[test]
fn f_077_job_progress_never_moves_backwards() {
    let lines: Vec<String> = (0..260)
        .map(|index| assistant_line(&format!("answer {index}")))
        .collect();
    let mut host = host_with_rollout(&lines);
    output_of(&mut host);
    let reported = host.jobs().reported_progress();
    assert!(
        reported.windows(2).all(|pair| pair[0] <= pair[1]),
        "{reported:?}"
    );
    let expected: Vec<f32> = (0..6)
        .map(|step| 100.0 * (step * 50) as f32 / 260.0)
        .collect();
    assert_eq!(reported.len(), expected.len(), "{reported:?}");
    for (got, want) in reported.iter().zip(expected) {
        assert!((got - want).abs() < 0.01, "{got} vs {want}");
    }
}

/// REQ: F-077
#[test]
fn f_077_job_stops_with_cancelled_when_the_user_cancels() {
    let mut host = host_with_rollout(&standard_lines());
    host.jobs().request_cancel();
    let error = host
        .run_job("codex-to-claude", input())
        .expect_err("cancelled");
    assert_eq!(error.code, ErrorCode::Cancelled);
}

/// REQ: F-077
#[test]
fn f_077_job_a_missing_rollout_fails_with_not_found() {
    let mut host = server_host();
    let error = host
        .run_job("codex-to-claude", input())
        .expect_err("missing rollout");
    assert_eq!(error.code, ErrorCode::NotFound);
}

/// REQ: F-077
#[test]
fn f_077_job_malformed_input_fails_with_invalid_argument() {
    let mut host = host_with_rollout(&standard_lines());
    let error = host
        .run_job("codex-to-claude", b"{")
        .expect_err("bad input");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-077
#[test]
fn f_077_job_unknown_kind_fails_with_invalid_argument() {
    let mut host = host_with_rollout(&standard_lines());
    let error = host
        .run_job("claude-to-codex", input())
        .expect_err("no such job");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-077
#[test]
fn f_077_job_does_not_change_the_original_rollout() {
    let lines = standard_lines();
    let mut host = host_with_rollout(&lines);
    output_of(&mut host);
    let after = host
        .fs()
        .read(FsPath {
            scope: "agent-sessions:codex".into(),
            path: ROLLOUT.into(),
        })
        .expect("rollout");
    assert_eq!(after, lines.join("\n").into_bytes());
}

/// REQ: F-077
#[test]
fn f_077_job_input_with_extra_fields_still_runs() {
    let mut host = host_with_rollout(&standard_lines());
    let bytes = json!({"rollout": ROLLOUT, "unused": true})
        .to_string()
        .into_bytes();
    assert!(host.run_job("codex-to-claude", &bytes).is_ok());
}

/// Starts a Codex to Claude conversion and completes the stop of the agent, as the host would.
fn start_and_stop_codex(host: &mut FakeHost, pane: u64) {
    let submissions = host.deliver_one(start_message(pane, "claude"));
    let terminate = submissions
        .iter()
        .find(|s| matches!(s.intent, Intent::TerminatePaneProcess(_)))
        .expect("terminate submitted");
    host.deliver_one(ilium_plugin_sdk::types::Event::RequestCompleted(
        RequestResult {
            request: RequestRef::Submission(terminate.correlation),
            outcome: Outcome::Ok,
        },
    ));
}

/// REQ: F-077
#[test]
fn f_077_after_the_agent_stopped_the_server_starts_the_codex_to_claude_job() {
    let (mut host, pane) = server_with_pane("codex", true);
    start_and_stop_codex(&mut host, pane);
    let jobs = host.jobs().list();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(jobs[0].kind, "codex-to-claude");
    let sent: Value = serde_json::from_slice(&jobs[0].input).expect("JSON input");
    assert_eq!(
        sent["rollout"],
        format!("projects/p/{SOURCE_SESSION}.jsonl")
    );
}

/// REQ: F-077
#[test]
fn f_077_no_job_starts_while_the_agent_is_still_running() {
    let (mut host, pane) = server_with_pane("codex", true);
    host.deliver_one(start_message(pane, "claude"));
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_refused_stop_starts_no_job_and_offers_no_resume() {
    let (mut host, pane) = server_with_pane("codex", true);
    let submissions = host.deliver_one(start_message(pane, "claude"));
    let terminate = submissions
        .iter()
        .find(|s| matches!(s.intent, Intent::TerminatePaneProcess(_)))
        .expect("terminate");
    let reply = host.deliver_one(ilium_plugin_sdk::types::Event::RequestCompleted(
        RequestResult {
            request: RequestRef::Submission(terminate.correlation),
            outcome: Outcome::Failed(ilium_plugin_sdk::types::PluginError {
                code: ErrorCode::Busy,
                message: "still running".into(),
                retry_after_ms: None,
            }),
        },
    ));
    assert!(host.jobs().list().is_empty());
    let sent = messages(&reply);
    let failed = sent
        .iter()
        .find(|m| m.1 == PluginSide::Client && m.2 == "failed")
        .expect("failed message");
    let payload: Value = serde_json::from_str(&failed.3).unwrap();
    assert_eq!(payload["can_resume"], false);
}
