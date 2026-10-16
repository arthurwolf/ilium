//! F-080 Remote compaction: the `compact` job, its runner, progress, cancellation and the resume of the agent (page sample 1, user doc Remote compaction).

use ilium_plugin_sdk::types::{AgentActivity, CommandOrigin, ErrorCode, Event, Intent, JobProgressEvent, NodeId, Outcome, PluginError, RequestId, RequestRef, RequestResult};
use serde_json::{json, Value};

use crate::common::*;

fn start(host: &mut ilium_plugin_sdk::testing::FakeHost, pane: u64) -> Vec<ilium_plugin_sdk::types::Submission> {
    host.deliver_one(command_on(COMPACT_COMMAND, CommandOrigin::Palette, Some(pane)))
}

fn complete_job(host: &mut ilium_plugin_sdk::testing::FakeHost, request: u64) -> Vec<ilium_plugin_sdk::types::Submission> {
    host.deliver_one(Event::RequestCompleted(RequestResult { request: RequestRef::Call(RequestId(request)), outcome: Outcome::Ok }))
}

fn reply(text: &str) -> Result<String, PluginError> {
    Ok(text.to_string())
}

const GOOD_SUMMARY: &str = "<analysis>scratch</analysis><summary>1. Primary Request and Intent: refactor the parser. 2. Key Technical Concepts: tokens. 3. Files and Code Sections: parser.rs. 4. Errors and fixes: none. 5. Problem Solving: done. 6. All user messages: refactor. 7. Pending Tasks: none. 8. Current Work: parser. 9. Optional Next Step: tests.</summary>";

/// REQ: F-080
#[test]
fn remote_080_the_compact_command_starts_one_job_of_kind_compact() {
    let transcript = transcript_of(1_000);
    let (mut host, pane) = server_with_transcript(&transcript);
    start(&mut host, pane);
    let jobs = host.jobs().list();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].kind, "compact");
}

/// REQ: F-080
#[test]
fn remote_080_the_job_input_is_the_transcript_of_the_pane() {
    let transcript = transcript_of(70_000);
    let (mut host, pane) = server_with_transcript(&transcript);
    start(&mut host, pane);
    assert_eq!(host.jobs().list()[0].input, transcript);
}

/// REQ: F-080
#[test]
fn remote_080_a_second_compact_command_while_the_job_runs_starts_no_second_job() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    start(&mut host, pane);
    start(&mut host, pane);
    assert_eq!(host.jobs().list().len(), 1);
}

/// REQ: F-080
#[test]
fn remote_080_another_command_id_starts_no_job() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    host.deliver_one(command_on("ilium.core.compaction/other", CommandOrigin::Palette, Some(pane)));
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-080
#[test]
fn remote_080_the_toolbar_origin_starts_the_same_job_as_the_palette() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    host.deliver_one(command_on(COMPACT_COMMAND, CommandOrigin::Toolbar, Some(pane)));
    let jobs = host.jobs().list();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].kind, "compact");
}

/// REQ: F-080
#[test]
fn remote_080_a_pane_without_a_verified_transcript_starts_no_job() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    host.agents().set_transcript_verified(pane, false);
    start(&mut host, pane);
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-080
#[test]
fn remote_080_a_pane_that_is_not_an_agent_starts_no_job() {
    let mut host = server_host();
    let pane = host.tree().add(ilium_plugin_sdk::testing::TreeNode {
        id: 0,
        parent: None,
        name: "shell".to_string(),
        class: ilium_plugin_sdk::types::NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    });
    start(&mut host, pane);
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-080
#[test]
fn remote_080_a_command_without_a_pane_context_starts_no_job() {
    let (mut host, _pane) = server_with_transcript(&transcript_of(500));
    host.deliver_one(command_on(COMPACT_COMMAND, CommandOrigin::Palette, None));
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-080
#[test]
fn remote_080_starting_the_job_stops_the_agent_after_freezing_the_pane() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    let submissions = start(&mut host, pane);
    let kinds: Vec<&'static str> = intents(&submissions)
        .iter()
        .filter_map(|intent| match intent {
            Intent::FreezePane(spec) if spec.pane == NodeId(pane) => Some("freeze"),
            Intent::TerminatePaneProcess(spec) if spec.pane == NodeId(pane) => Some("terminate"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["freeze", "terminate"]);
}

/// REQ: F-080
#[test]
fn remote_080_job_progress_events_produce_no_submission() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    start(&mut host, pane);
    let request = host.jobs().list()[0].request;
    let submissions = host.deliver_one(Event::JobProgress(JobProgressEvent { job: RequestId(request), percent: 50.0 }));
    assert!(submissions.is_empty());
}

/// REQ: F-080
#[test]
fn remote_080_a_progress_event_of_a_foreign_job_is_ignored() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    start(&mut host, pane);
    let submissions = host.deliver_one(Event::JobProgress(JobProgressEvent { job: RequestId(9_999), percent: 10.0 }));
    assert!(submissions.is_empty());
}

/// REQ: F-080
#[test]
fn remote_080_a_finished_job_writes_the_summary_and_resumes_the_same_session() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    start(&mut host, pane);
    let request = host.jobs().list()[0].request;
    host.jobs().finish(request, Ok(GOOD_SUMMARY.as_bytes().to_vec()));
    let submissions = host.submitted();
    let resumed = intents(&submissions).iter().any(|intent| matches!(intent, Intent::ReplacePaneCommand(spec) if spec.pane == NodeId(pane)));
    let unfrozen = intents(&submissions).iter().any(|intent| matches!(intent, Intent::UnfreezePane(spec) if spec.pane == NodeId(pane)));
    assert!(resumed && unfrozen);
}

/// REQ: F-080
#[test]
fn remote_080_a_finished_job_keeps_a_backup_of_the_original_transcript() {
    let original = transcript_of(500);
    let (mut host, pane) = server_with_transcript(&original);
    start(&mut host, pane);
    let request = host.jobs().list()[0].request;
    host.jobs().finish(request, Ok(GOOD_SUMMARY.as_bytes().to_vec()));
    let backups = host.agents().transcript_backups(pane);
    assert_eq!(backups.len(), 1);
    assert!(backups[0].0.contains(".pre-compaction-") && backups[0].0.ends_with(".bak"));
    assert_eq!(backups[0].1, original);
}

/// REQ: F-080
#[test]
fn remote_080_only_the_newest_three_backups_are_kept_by_default() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    for _round in 0..5 {
        start(&mut host, pane);
        let request = host.jobs().list().last().expect("job").request;
        host.jobs().finish(request, Ok(GOOD_SUMMARY.as_bytes().to_vec()));
    }
    assert_eq!(host.agents().transcript_backups(pane).len(), 3);
}

/// REQ: F-080
#[test]
fn remote_080_a_failed_job_leaves_the_transcript_untouched() {
    let original = transcript_of(500);
    let (mut host, pane) = server_with_transcript(&original);
    start(&mut host, pane);
    let request = host.jobs().list()[0].request;
    let error = PluginError { code: ErrorCode::Internal, message: "model failed".to_string(), retry_after_ms: None };
    host.jobs().finish(request, Err(error));
    assert_eq!(host.agents().transcript(pane), Some(original));
    assert!(host.agents().transcript_backups(pane).is_empty());
}

/// REQ: F-080
#[test]
fn remote_080_a_failed_job_does_not_replace_the_pane_command() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    start(&mut host, pane);
    let request = host.jobs().list()[0].request;
    let error = PluginError { code: ErrorCode::Internal, message: "model failed".to_string(), retry_after_ms: None };
    host.jobs().finish(request, Err(error));
    assert!(!intents(&host.submitted()).iter().any(|intent| matches!(intent, Intent::ReplacePaneCommand(_))));
}

/// REQ: F-080
#[test]
fn remote_080_nothing_is_written_when_the_transcript_changed_while_the_job_ran() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    start(&mut host, pane);
    host.agents().set_transcript(pane, transcript_of(600));
    let request = host.jobs().list()[0].request;
    host.jobs().finish(request, Ok(GOOD_SUMMARY.as_bytes().to_vec()));
    assert_eq!(host.agents().transcript(pane), Some(transcript_of(600)));
}

/// REQ: F-080
#[test]
fn remote_080_after_the_job_ends_a_new_compact_command_starts_a_new_job() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    start(&mut host, pane);
    let first = host.jobs().list()[0].request;
    host.jobs().finish(first, Ok(GOOD_SUMMARY.as_bytes().to_vec()));
    start(&mut host, pane);
    assert_eq!(host.jobs().list().len(), 2);
}

/// REQ: F-080
#[test]
fn remote_080_a_completion_of_a_foreign_request_changes_nothing() {
    let (mut host, pane) = server_with_transcript(&transcript_of(500));
    start(&mut host, pane);
    let submissions = complete_job(&mut host, 9_999);
    assert!(submissions.is_empty());
    assert_eq!(host.jobs().list().len(), 1);
}

/// REQ: F-080
#[test]
fn remote_080_the_runner_summarises_one_chunk_per_32_kib_of_input() {
    let mut host = server_host();
    host.script_inference(vec![reply(GOOD_SUMMARY), reply(GOOD_SUMMARY), reply(GOOD_SUMMARY)]);
    let output = host.run_job("compact", transcript_of(2 * CHUNK_BYTES + 1)).expect("job succeeds");
    assert!(!output.is_empty());
    assert_eq!(host.job_progress_reports().len(), 3);
}

/// REQ: F-080
#[test]
fn remote_080_input_of_exactly_one_chunk_reports_100_percent_once() {
    let mut host = server_host();
    host.script_inference(vec![reply(GOOD_SUMMARY)]);
    host.run_job("compact", transcript_of(CHUNK_BYTES)).expect("job succeeds");
    assert_eq!(host.job_progress_reports(), [100.0]);
}

/// REQ: F-080
#[test]
fn remote_080_progress_is_reported_as_a_percent_of_the_chunks_done() {
    let mut host = server_host();
    host.script_inference(vec![reply(GOOD_SUMMARY), reply(GOOD_SUMMARY), reply(GOOD_SUMMARY), reply(GOOD_SUMMARY)]);
    host.run_job("compact", transcript_of(4 * CHUNK_BYTES)).expect("job succeeds");
    assert_eq!(host.job_progress_reports(), [25.0, 50.0, 75.0, 100.0]);
}

/// REQ: F-080
#[test]
fn remote_080_a_cancelled_job_stops_between_chunks_with_the_cancelled_code() {
    let mut host = server_host();
    host.script_inference(vec![reply(GOOD_SUMMARY), reply(GOOD_SUMMARY), reply(GOOD_SUMMARY)]);
    host.cancel_job_after_chunks(1);
    let error = host.run_job("compact", transcript_of(3 * CHUNK_BYTES)).expect_err("cancelled");
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.message, "compaction stopped");
    assert_eq!(host.job_progress_reports().len(), 1);
}

/// REQ: F-080
#[test]
fn remote_080_cancelling_before_the_first_chunk_makes_no_inference_call() {
    let mut host = server_host();
    host.cancel_job_after_chunks(0);
    let error = host.run_job("compact", transcript_of(CHUNK_BYTES)).expect_err("cancelled");
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(host.inference_requests().is_empty());
}

/// REQ: F-080
#[test]
fn remote_080_a_model_that_keeps_failing_gives_the_offline_fallback_summary() {
    let mut host = server_host();
    let failure = PluginError { code: ErrorCode::Network, message: "provider down".to_string(), retry_after_ms: None };
    host.script_inference(vec![Err(failure.clone()), Err(failure.clone()), Err(failure)]);
    let output = host.run_job("compact", b"first request\nsecond request".to_vec()).expect("fallback instead of failure");
    assert!(!output.is_empty());
}

/// REQ: F-080
#[test]
fn remote_080_inference_requests_are_made_with_the_server_inference_grant_and_a_purpose() {
    let mut host = server_host();
    host.script_inference(vec![reply(GOOD_SUMMARY)]);
    host.run_job("compact", transcript_of(100)).expect("job succeeds");
    let requests = host.inference_requests();
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|(_, request)| !request.purpose.is_empty()));
    assert!(requests.iter().all(|(_, request)| request.messages.iter().any(|message| message.role == "system")));
}

/// REQ: F-080
#[test]
fn remote_080_the_job_output_is_the_cleaned_summary_without_the_analysis_scratchpad() {
    let mut host = server_host();
    host.script_inference(vec![reply(GOOD_SUMMARY)]);
    let output = host.run_job("compact", transcript_of(100)).expect("job succeeds");
    let text = String::from_utf8(output).expect("utf-8 summary");
    assert!(text.starts_with("1. Primary Request and Intent:"));
    assert!(!text.contains("<analysis>") && !text.contains("<summary>") && !text.contains("scratch"));
}

/// REQ: F-080
#[test]
fn remote_080_an_unknown_job_kind_is_refused_with_invalid_argument() {
    let mut host = server_host();
    let error = host.run_job("something-else", Vec::new()).expect_err("unknown kind");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-080
#[test]
fn remote_080_secrets_are_masked_before_the_transcript_reaches_the_model() {
    let mut host = server_host();
    host.settings().set("remote_compaction.redact_secrets", json!(true)).expect("setting accepted");
    host.script_inference(vec![reply(GOOD_SUMMARY)]);
    let secret = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789";
    host.run_job("compact", format!("my key is {secret} please keep it").into_bytes()).expect("job succeeds");
    let sent: Value = json!(host.inference_requests().iter().flat_map(|(_, request)| request.messages.iter().map(|message| message.content.clone())).collect::<Vec<_>>());
    assert!(!sent.to_string().contains(secret));
}

/// REQ: F-080
#[test]
fn remote_080_with_redaction_off_the_transcript_text_is_sent_as_is() {
    let mut host = server_host();
    host.settings().set("remote_compaction.redact_secrets", json!(false)).expect("setting accepted");
    host.script_inference(vec![reply(GOOD_SUMMARY)]);
    host.run_job("compact", b"plain text with token-12345".to_vec()).expect("job succeeds");
    let sent: Vec<String> = host.inference_requests().iter().flat_map(|(_, request)| request.messages.iter().map(|message| message.content.clone())).collect();
    assert!(sent.iter().any(|content| content.contains("token-12345")));
}
