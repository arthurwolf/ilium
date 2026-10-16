//! F-052: `ilium voice say`. The server side validates the arguments, asks the attached client through a
//! `client-request` of kind `voice-say`, copies the client's JSONL answer to standard output and sets the exit
//! code. The client side runs the typed voice input and answers with the JSONL result or error record.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::testing::fakes::cli::Invocation;
use ilium_plugin_sdk::types::{ErrorCode, Outcome, PluginError, RequestRef};
use serde_json::{Value, json};

use crate::common::{client_attached, client_host, client_request, jsonl, server_host};

/// Runs `ilium voice <args>` on a server host without an attached client; returns the records and exit code.
fn run_without_client(args: &[&str], stdin: &[u8]) -> (Vec<Value>, Option<u8>) {
    let mut host = server_host();
    let run = host.cli().invoke(args, stdin);
    host.settle();
    (jsonl(&run.stdout()), run.exit_code())
}

fn last_error_code(records: &[Value]) -> String {
    let last = records.last().expect("at least one record");
    assert_eq!(last["type"], "error", "{records:?}");
    last["code"].as_str().expect("code").to_string()
}

fn many(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("sentence {index}")).collect()
}

fn say_args(sentences: &[String]) -> Vec<&str> {
    let mut args = vec!["say"];
    args.extend(sentences.iter().map(String::as_str));
    args
}

/// REQ: F-052
#[test]
fn say_052_no_sentence_is_invalid_request() {
    let (records, exit) = run_without_client(&["say"], b"");
    assert_eq!(last_error_code(&records), "invalid-request");
    assert_eq!(exit, Some(1));
}

/// REQ: F-052
#[test]
fn say_052_a_blank_sentence_is_invalid_request() {
    let (records, exit) = run_without_client(&["say", "   "], b"");
    assert_eq!(last_error_code(&records), "invalid-request");
    assert_eq!(exit, Some(1));
}

/// REQ: F-052
#[test]
fn say_052_one_blank_sentence_among_valid_ones_rejects_the_whole_request() {
    let (records, _) = run_without_client(&["say", "fine", "", "also fine"], b"");
    assert_eq!(last_error_code(&records), "invalid-request");
}

/// REQ: F-052
#[test]
fn say_052_32_sentences_pass_the_argument_check() {
    let sentences = many(32);
    let (records, _) = run_without_client(&say_args(&sentences), b"");
    assert_ne!(last_error_code(&records), "invalid-request");
}

/// REQ: F-052
#[test]
fn say_052_33_sentences_are_invalid_request() {
    let sentences = many(33);
    let (records, exit) = run_without_client(&say_args(&sentences), b"");
    assert_eq!(last_error_code(&records), "invalid-request");
    assert_eq!(exit, Some(1));
}

/// REQ: F-052
#[test]
fn say_052_31_sentences_pass_the_argument_check() {
    let sentences = many(31);
    let (records, _) = run_without_client(&say_args(&sentences), b"");
    assert_ne!(last_error_code(&records), "invalid-request");
}

/// REQ: F-052
#[test]
fn say_052_a_sentence_of_4000_characters_passes_the_argument_check() {
    let sentence = "x".repeat(4000);
    let (records, _) = run_without_client(&["say", &sentence], b"");
    assert_ne!(last_error_code(&records), "invalid-request");
}

/// REQ: F-052
#[test]
fn say_052_a_sentence_of_4001_characters_is_invalid_request() {
    let sentence = "x".repeat(4001);
    let (records, exit) = run_without_client(&["say", &sentence], b"");
    assert_eq!(last_error_code(&records), "invalid-request");
    assert_eq!(exit, Some(1));
}

/// REQ: F-052
#[test]
fn say_052_a_sentence_of_3999_characters_passes_the_argument_check() {
    let sentence = "x".repeat(3999);
    let (records, exit) = run_without_client(&["say", &sentence], b"");
    // The argument check passes; with no client attached the next refusal is no-voice-client.
    assert_eq!(last_error_code(&records), "no-voice-client");
    assert_eq!(exit, Some(1));
}

/// REQ: F-052
#[test]
fn say_052_the_length_limit_is_measured_after_trimming() {
    let sentence = format!("   {}   ", "x".repeat(4000));
    let (records, _) = run_without_client(&["say", &sentence], b"");
    assert_ne!(last_error_code(&records), "invalid-request");
}

/// REQ: F-052
#[test]
fn say_052_the_length_limit_counts_characters_not_bytes() {
    let at_limit = "\u{e9}".repeat(4000);
    let (records, _) = run_without_client(&["say", &at_limit], b"");
    assert_ne!(last_error_code(&records), "invalid-request");
    let over = "\u{e9}".repeat(4001);
    let (records, _) = run_without_client(&["say", &over], b"");
    assert_eq!(last_error_code(&records), "invalid-request");
}

/// REQ: F-052
#[test]
fn say_052_without_an_attached_client_the_error_is_no_voice_client_with_the_attach_hint() {
    let (records, exit) = run_without_client(&["say", "what agents are running?"], b"");
    let error = records.last().expect("record");
    assert_eq!(error["type"], "error");
    assert_eq!(error["command"], "voice say");
    assert_eq!(error["ok"], false);
    assert_eq!(error["code"], "no-voice-client");
    assert_eq!(
        error["hint"],
        "attach an interactive Ilium client to this session with `ilium`; the voice session runs inside it"
    );
    assert_eq!(exit, Some(1));
}

/// REQ: F-052
#[test]
fn say_052_an_error_run_ends_with_exactly_one_error_record_and_no_result() {
    let (records, _) = run_without_client(&["say", "hello"], b"");
    assert_eq!(records.iter().filter(|record| record["type"] == "error").count(), 1);
    assert_eq!(records.iter().filter(|record| record["type"] == "result").count(), 0);
    assert!(records.iter().all(|record| record.get("type").is_some()));
}

/// REQ: F-052
#[test]
fn say_052_a_lone_hyphen_with_empty_standard_input_is_invalid_request() {
    let (records, _) = run_without_client(&["say", "-"], b"\n\n");
    assert_eq!(last_error_code(&records), "invalid-request");
}

/// REQ: F-052
#[test]
fn say_052_a_lone_hyphen_reads_one_sentence_per_non_empty_line() {
    let (records, _) = run_without_client(&["say", "-"], b"first\n\n  second  \n");
    assert_ne!(last_error_code(&records), "invalid-request");
}

/// REQ: F-052
#[test]
fn say_052_standard_input_with_33_lines_is_invalid_request() {
    let lines = many(33).join("\n");
    let (records, _) = run_without_client(&["say", "-"], lines.as_bytes());
    assert_eq!(last_error_code(&records), "invalid-request");
}

/// REQ: F-052
#[test]
fn say_052_timeout_of_zero_seconds_is_rejected_with_a_nonzero_exit() {
    let (records, exit) = run_without_client(&["say", "--timeout-s", "0", "hello"], b"");
    assert_eq!(last_error_code(&records), "invalid-request");
    assert_eq!(exit, Some(1));
}

/// REQ: F-052
#[test]
fn say_052_timeout_of_601_seconds_is_rejected_with_a_nonzero_exit() {
    let (records, exit) = run_without_client(&["say", "--timeout-s", "601", "hello"], b"");
    assert_eq!(last_error_code(&records), "invalid-request");
    assert_eq!(exit, Some(1));
}

/// REQ: F-052
#[test]
fn say_052_timeouts_of_1_and_600_seconds_pass_the_argument_check() {
    for seconds in ["1", "600"] {
        let (records, _) = run_without_client(&["say", "--timeout-s", seconds, "hello"], b"");
        assert_ne!(last_error_code(&records), "invalid-request", "timeout {seconds}");
    }
}

/// Attaches a client, runs `ilium voice <args>` and returns the host with the one pending request.
fn run_with_client(args: &[&str]) -> (FakeHost, Invocation, RequestRef) {
    let mut host = server_host();
    host.deliver_one(client_attached(1));
    let run = host.cli().invoke(args, b"");
    host.settle();
    let request = host.pending_requests().first().cloned().expect("the server asked the client");
    (host, run, request)
}

/// REQ: F-052
#[test]
fn say_052_with_a_client_the_server_sends_one_voice_say_client_request() {
    let (mut host, _run, request) = run_with_client(&["say", "what agents are running?"]);
    let sent = host.client_requests().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].kind, "voice-say");
    assert_eq!(sent[0].client, 1);
    assert_eq!(sent[0].request, request);
}

/// REQ: F-052
#[test]
fn say_052_the_client_request_waits_at_most_5000_milliseconds() {
    let (mut host, _run, _request) = run_with_client(&["say", "hello"]);
    assert_eq!(host.client_requests().sent()[0].timeout_ms, 5000);
}

/// REQ: F-052
#[test]
fn say_052_the_client_request_payload_carries_the_trimmed_sentences_in_order() {
    let (mut host, _run, _request) = run_with_client(&["say", "  focus the first agent ", "say hello to it"]);
    let payload: Value = serde_json::from_str(&host.client_requests().sent()[0].payload_json).expect("JSON payload");
    let text = payload.to_string();
    let first = text.find("focus the first agent").expect("first sentence");
    let second = text.find("say hello to it").expect("second sentence");
    assert!(first < second);
    assert!(!text.contains("  focus"));
}

/// REQ: F-052
#[test]
fn say_052_the_start_flag_reaches_the_client_in_the_payload() {
    let (mut host, _run, _request) = run_with_client(&["say", "--start", "hello"]);
    let payload: Value = serde_json::from_str(&host.client_requests().sent()[0].payload_json).expect("JSON payload");
    assert_eq!(payload["start"], true);
}

/// REQ: F-052
#[test]
fn say_052_without_the_start_flag_the_payload_says_start_false() {
    let (mut host, _run, _request) = run_with_client(&["say", "hello"]);
    let payload: Value = serde_json::from_str(&host.client_requests().sent()[0].payload_json).expect("JSON payload");
    assert_eq!(payload["start"], false);
}

/// REQ: F-052
#[test]
fn say_052_the_first_record_is_a_progress_record_with_the_documented_fields() {
    let (mut host, run, _request) = run_with_client(&["say", "--start", "hello"]);
    host.settle();
    let records = jsonl(&run.stdout());
    let progress = &records[0];
    assert_eq!(progress["type"], "progress");
    assert_eq!(progress["command"], "voice say");
    assert_eq!(progress["stage"], "sending");
    assert_eq!(progress["sentence_count"], 1);
    assert_eq!(progress["start"], true);
    for key in ["request_id", "session", "socket", "session_source"] {
        assert!(progress.get(key).is_some(), "progress field {key}");
    }
}

/// REQ: F-052
#[test]
fn say_052_the_client_answer_line_is_copied_to_stdout_and_the_exit_code_is_zero() {
    let (mut host, run, request) = run_with_client(&["say", "hello"]);
    let result = json!({
        "type": "result", "command": "voice say", "ok": true, "request_id": 1, "session": "default",
        "sentence_count": 1, "accepted_sentences": 1, "voice_phase": "listening", "started_voice": false,
        "delivery": "queued-to-voice-session"
    });
    host.client_requests().answer(request, Ok(result.to_string()));
    host.settle();
    let records = jsonl(&run.stdout());
    assert_eq!(records.last().expect("record"), &result);
    assert_eq!(run.exit_code(), Some(0));
}

/// REQ: F-052
#[test]
fn say_052_a_failed_client_request_ends_with_exit_code_one() {
    let (mut host, run, request) = run_with_client(&["say", "hello"]);
    host.complete(
        request,
        Outcome::Failed(PluginError { code: ErrorCode::TimedOut, message: "no answer".to_string(), retry_after_ms: None }),
    );
    host.settle();
    assert_eq!(run.exit_code(), Some(1));
}

/// REQ: F-052
#[test]
fn say_052_a_failed_client_request_still_ends_with_one_error_record() {
    let (mut host, run, request) = run_with_client(&["say", "hello"]);
    host.complete(
        request,
        Outcome::Failed(PluginError { code: ErrorCode::TimedOut, message: "no answer".to_string(), retry_after_ms: None }),
    );
    host.settle();
    let records = jsonl(&run.stdout());
    assert_eq!(records.last().expect("record")["type"], "error");
    assert_eq!(records.iter().filter(|record| record["type"] == "error").count(), 1);
}

/// REQ: F-052
#[test]
fn say_052_a_detached_client_before_the_answer_does_not_leave_the_run_hanging() {
    let (mut host, run, _request) = run_with_client(&["say", "hello"]);
    host.deliver_one(ilium_plugin_sdk::types::Event::ClientDetached(ilium_plugin_sdk::types::ClientId(1)));
    host.advance_clock(5_001);
    host.settle();
    assert!(run.exit_code().is_some());
}

/// Delivers a `voice-say` request to a client host and returns the text of its answer.
fn client_answer(host: &mut FakeHost, payload: Value) -> String {
    host.deliver_one(client_request(41, "voice-say", payload));
    host.settle();
    let responses = host.client_requests().responses();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].call, 41);
    match &responses[0].result {
        Ok(line) => line.clone(),
        Err(error) => error.message.clone(),
    }
}

/// REQ: F-052
#[test]
fn say_052_client_with_voice_on_accepts_and_reports_a_result_record() {
    let mut host = crate::common::enabled_client("semantic_vad");
    let line = client_answer(&mut host, json!({ "sentences": ["what agents are running?"], "start": false }));
    let record: Value = serde_json::from_str(&line).expect("one JSON object");
    assert_eq!(record["type"], "result");
    assert_eq!(record["command"], "voice say");
    assert_eq!(record["ok"], true);
    assert_eq!(record["sentence_count"], 1);
    assert_eq!(record["accepted_sentences"], 1);
    assert_eq!(record["started_voice"], false);
    assert_eq!(record["delivery"], "queued-to-voice-session");
}

/// REQ: F-052
#[test]
fn say_052_the_result_voice_phase_is_connecting_right_after_voice_is_enabled() {
    let mut host = crate::common::enabled_client("semantic_vad");
    let line = client_answer(&mut host, json!({ "sentences": ["hello"], "start": false }));
    let record: Value = serde_json::from_str(&line).expect("one JSON object");
    let phase = record["voice_phase"].as_str().expect("phase");
    // Voice was just switched on and no provider session is up yet: the phase is connecting.
    assert_eq!(phase, "connecting");
}

/// REQ: F-052
#[test]
fn say_052_the_result_counts_every_sentence_of_the_request() {
    let mut host = crate::common::enabled_client("semantic_vad");
    let line = client_answer(&mut host, json!({ "sentences": ["one", "two", "three"], "start": false }));
    let record: Value = serde_json::from_str(&line).expect("one JSON object");
    assert_eq!(record["sentence_count"], 3);
    assert_eq!(record["accepted_sentences"], 3);
}

/// REQ: F-052
#[test]
fn say_052_voice_off_without_start_is_the_voice_off_error_with_the_start_hint() {
    let mut host = client_host();
    let line = client_answer(&mut host, json!({ "sentences": ["hello"], "start": false }));
    assert!(line.contains("voice-off"), "{line}");
    assert!(line.contains("pass --start to switch voice control on, or press F8 in the Ilium client"), "{line}");
}

/// REQ: F-052
#[test]
fn say_052_voice_off_with_start_switches_voice_on_and_saves_the_setting() {
    let mut host = client_host();
    let line = client_answer(&mut host, json!({ "sentences": ["hello"], "start": true }));
    let record: Value = serde_json::from_str(&line).expect("one JSON object");
    assert_eq!(record["ok"], true);
    assert_eq!(record["started_voice"], true);
    assert_eq!(host.kernel_settings().get("voice-control", "enabled"), Some(json!(true)));
}

/// REQ: F-052
#[test]
fn say_052_start_on_a_running_session_reports_started_voice_false() {
    let mut host = crate::common::enabled_client("semantic_vad");
    let line = client_answer(&mut host, json!({ "sentences": ["hello"], "start": true }));
    let record: Value = serde_json::from_str(&line).expect("one JSON object");
    assert_eq!(record["started_voice"], false);
}

/// REQ: F-052
#[test]
fn say_052_the_client_registers_no_audio_capture_for_typed_input() {
    let mut host = crate::common::enabled_client("push_to_talk");
    let _ = client_answer(&mut host, json!({ "sentences": ["hello"], "start": false }));
    assert!(host.audio().captures().iter().all(|capture| !capture.open));
}
