//! F-077: the Claude Code transcript is written with an atomic write that must not replace a file, read back
//! with its revision and checked by line count before the session is registered (page sample "Write the
//! Claude Code transcript and verify it").

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, Event, FsPath, Intent, Outcome, PluginError, PluginSide, RequestRef, RequestResult, Submission};
use serde_json::{json, Value};

use crate::common::*;
use crate::start::{server_with_pane, start_message};

const WRITE_SCOPE: &str = "agent-config:claude";
const ROLLOUT_LINE: &str = "{\"type\":\"user\"}";

/// Runs a Codex to Claude conversion until the job started; returns the job request number.
pub fn run_until_job(host: &mut FakeHost, pane: u64) -> u64 {
    let submissions = host.deliver_one(start_message(pane, "claude"));
    let terminate = submissions
        .iter()
        .find(|s| matches!(s.intent, Intent::TerminatePaneProcess(_)))
        .expect("terminate submitted");
    host.deliver_one(Event::RequestCompleted(RequestResult {
        request: RequestRef::Submission(terminate.correlation),
        outcome: Outcome::Ok,
    }));
    host.jobs().list().first().expect("job started").request
}

/// The job output for `lines` converted lines claiming `kept` kept items.
pub fn job_output(lines: usize, kept: usize) -> Vec<u8> {
    let jsonl: Vec<String> = (0..lines).map(|index| json!({"type": "user", "n": index}).to_string()).collect();
    json!({"claude_jsonl": jsonl.join("\n"), "kept": kept, "dropped": 2}).to_string().into_bytes()
}

/// Finishes the job with a result and returns everything the plugin submitted afterwards.
pub fn finish_job(host: &mut FakeHost, request: u64, result: Result<Vec<u8>, PluginError>) -> Vec<Submission> {
    host.jobs().finish(request, result);
    host.settle();
    host.submitted()
}

fn codex_host() -> (FakeHost, u64) {
    let (mut host, pane) = server_with_pane("codex", true);
    host.fs().write(FsPath { scope: "agent-sessions:codex".into(), path: format!("projects/p/{SOURCE_SESSION}.jsonl") }, ROLLOUT_LINE.as_bytes());
    (host, pane)
}

fn failed_payload(submissions: &[Submission]) -> Option<Value> {
    messages(submissions)
        .into_iter()
        .rev()
        .find(|m| m.1 == PluginSide::Client && m.2 == "failed")
        .map(|m| serde_json::from_str(&m.3).expect("JSON payload"))
}

/// REQ: F-077
#[test]
fn f_077_the_transcript_is_written_into_the_claude_config_scope() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    finish_job(&mut host, request, Ok(job_output(3, 3)));
    let paths = host.fs().paths(WRITE_SCOPE);
    let transcripts: Vec<&String> = paths.iter().filter(|path| path.ends_with(".jsonl")).collect();
    assert_eq!(transcripts.len(), 1, "{paths:?}");
}

/// REQ: F-077
#[test]
fn f_077_the_written_transcript_is_the_converted_output_byte_for_byte() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    let output = job_output(3, 3);
    finish_job(&mut host, request, Ok(output.clone()));
    let expected: Value = serde_json::from_slice(&output).unwrap();
    let path = host.fs().paths(WRITE_SCOPE).into_iter().find(|p| p.ends_with(".jsonl")).expect("transcript");
    let written = host.fs().read(FsPath { scope: WRITE_SCOPE.into(), path }).expect("file");
    assert_eq!(String::from_utf8(written).unwrap(), expected["claude_jsonl"].as_str().unwrap());
}

/// REQ: F-077
#[test]
fn f_077_a_write_that_finds_an_existing_file_completes_as_failed_conflict() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    host.inject_failure(
        "fs.write-atomic",
        PluginError { code: ErrorCode::Conflict, message: "exists".into(), retry_after_ms: None },
    );
    let submissions = finish_job(&mut host, request, Ok(job_output(3, 3)));
    assert!(created(&submissions).is_empty(), "no replacement pane after a clash");
    assert!(failed_payload(&submissions).is_some());
}

/// REQ: F-077
#[test]
fn f_077_a_clash_never_replaces_the_existing_transcript() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    host.inject_failure(
        "fs.write-atomic",
        PluginError { code: ErrorCode::Conflict, message: "exists".into(), retry_after_ms: None },
    );
    finish_job(&mut host, request, Ok(job_output(3, 3)));
    assert!(host.fs().paths(WRITE_SCOPE).iter().all(|path| !path.ends_with(".jsonl")));
}

/// REQ: F-077
#[test]
fn f_077_a_line_count_that_differs_after_the_write_fails_the_verification() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    let submissions = finish_job(&mut host, request, Ok(job_output(3, 5)));
    let payload = failed_payload(&submissions).expect("failed message");
    assert_eq!(payload["message"], "The Claude Code transcript did not verify.");
    assert!(created(&submissions).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_matching_line_count_passes_the_verification() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    let submissions = finish_job(&mut host, request, Ok(job_output(4, 4)));
    assert!(failed_payload(&submissions).is_none());
    assert_eq!(created(&submissions).len(), 1);
}

/// REQ: F-077
#[test]
fn f_077_a_failed_verification_reports_the_carried_and_left_out_counts() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    let submissions = finish_job(&mut host, request, Ok(job_output(3, 5)));
    let payload = failed_payload(&submissions).expect("failed message");
    assert_eq!(payload["carried"], 5);
    assert_eq!(payload["left_out"], 2);
}

/// REQ: F-077
#[test]
fn f_077_a_failed_verification_offers_to_resume_the_original_session() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    let submissions = finish_job(&mut host, request, Ok(job_output(3, 5)));
    assert_eq!(failed_payload(&submissions).expect("failed message")["can_resume"], true);
}

/// REQ: F-077
#[test]
fn f_077_a_failed_job_after_the_stop_offers_to_resume_the_original_session() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    let error = PluginError { code: ErrorCode::Io, message: "rollout unreadable".into(), retry_after_ms: None };
    let submissions = finish_job(&mut host, request, Err(error));
    let payload = failed_payload(&submissions).expect("failed message");
    assert_eq!(payload["can_resume"], true);
    assert!(created(&submissions).is_empty());
}

/// REQ: F-077
#[test]
fn f_077_a_cancelled_job_leaves_no_target_transcript() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    let error = PluginError { code: ErrorCode::Cancelled, message: "conversion cancelled".into(), retry_after_ms: None };
    finish_job(&mut host, request, Err(error));
    assert!(host.fs().paths(WRITE_SCOPE).iter().all(|path| !path.ends_with(".jsonl")));
}

/// REQ: F-077
#[test]
fn f_077_the_original_rollout_is_never_modified() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    finish_job(&mut host, request, Ok(job_output(3, 3)));
    let rollout = host
        .fs()
        .read(FsPath { scope: "agent-sessions:codex".into(), path: format!("projects/p/{SOURCE_SESSION}.jsonl") })
        .expect("rollout");
    assert_eq!(rollout, ROLLOUT_LINE.as_bytes());
}

/// REQ: F-077
#[test]
fn f_077_the_codex_session_scope_is_never_written() {
    let (mut host, pane) = codex_host();
    let request = run_until_job(&mut host, pane);
    finish_job(&mut host, request, Ok(job_output(3, 3)));
    assert_eq!(host.fs().paths("agent-sessions:codex").len(), 1, "only the original rollout exists");
}
