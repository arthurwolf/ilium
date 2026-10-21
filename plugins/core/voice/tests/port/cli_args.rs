//! Twins of `ilium/src/voice.rs` and `ilium-ipc/src/voice_text.rs`: argument collection and record shape.

use serde_json::Value;

use crate::common::{jsonl, server_host};

fn run(args: &[&str], stdin: &[u8]) -> (Vec<Value>, Option<u8>) {
    let mut host = server_host();
    let invocation = host.cli().invoke(args, stdin);
    host.settle();
    (jsonl(&invocation.stdout()), invocation.exit_code())
}

fn last_code(records: &[Value]) -> String {
    let last = records.last().expect("a record");
    assert_eq!(last["type"], "error", "{records:?}");
    last["code"].as_str().expect("code").to_string()
}

/// REQ: PORT:ilium/src/voice.rs::nothing_to_say_and_oversized_input_are_invalid_requests, PORT:ilium-ipc/src/voice_text.rs::malformed_requests_are_rejected_with_the_invalid_request_code
#[test]
fn say_052_nothing_to_say_and_oversized_input_are_invalid_request() {
    for (args, stdin) in [
        (vec!["say"], ""),
        (vec!["say", "  "], ""),
        (vec!["say", "-"], "\n\n"),
    ] {
        let (records, exit) = run(&args, stdin.as_bytes());
        assert_eq!(last_code(&records), "invalid-request", "{args:?}");
        assert_eq!(exit, Some(1));
    }
    let many: Vec<String> = (0..33).map(|n| format!("s{n}")).collect();
    let mut args = vec!["say"];
    args.extend(many.iter().map(String::as_str));
    assert_eq!(last_code(&run(&args, b"").0), "invalid-request");
    let long = "x".repeat(4001);
    assert_eq!(last_code(&run(&["say", &long], b"").0), "invalid-request");
}

/// REQ: PORT:ilium-ipc/src/voice_text.rs::the_length_limit_counts_characters_not_bytes
#[test]
fn say_052_the_length_limit_counts_characters_not_bytes() {
    let at_limit = "\u{e9}".repeat(4000);
    assert_ne!(
        last_code(&run(&["say", &at_limit], b"").0),
        "invalid-request"
    );
    let over = "\u{e9}".repeat(4001);
    assert_eq!(last_code(&run(&["say", &over], b"").0), "invalid-request");
}

/// REQ: PORT:ilium/src/voice.rs::say_needs_at_least_one_sentence_and_a_sane_timeout
#[test]
fn say_052_timeout_outside_one_to_600_seconds_is_invalid_request() {
    for seconds in ["0", "601"] {
        let (records, exit) = run(&["say", "--timeout-s", seconds, "hello"], b"");
        assert_eq!(last_code(&records), "invalid-request", "timeout {seconds}");
        assert_eq!(exit, Some(1));
    }
    for seconds in ["1", "600"] {
        assert_ne!(
            last_code(&run(&["say", "--timeout-s", seconds, "hello"], b"").0),
            "invalid-request"
        );
    }
}

/// REQ: PORT:ilium/src/voice.rs::a_lone_hyphen_expands_to_the_non_empty_lines_of_stdin_in_place, PORT:ilium/src/voice.rs::say_parses_sentences_the_stdin_token_and_flags
#[test]
fn say_052_a_lone_hyphen_expands_to_the_non_empty_lines_of_stdin_in_place() {
    let mut host = server_host();
    host.deliver_one(crate::common::client_attached(1));
    host.cli().invoke(
        &["say", "--start", "before", "-", "after"],
        b"first\n\n  second  \n",
    );
    host.settle();
    let payload: Value =
        serde_json::from_str(&host.client_requests().sent()[0].payload_json).expect("payload");
    assert_eq!(
        payload["sentences"],
        serde_json::json!(["before", "first", "second", "after"])
    );
    assert_eq!(payload["start"], true);
}

/// REQ: PORT:ilium/src/voice.rs::positional_sentences_keep_order_and_are_trimmed, PORT:ilium-ipc/src/voice_text.rs::sentences_are_trimmed_and_kept_in_order
#[test]
fn say_052_positional_sentences_keep_order_and_are_trimmed() {
    let mut host = server_host();
    host.deliver_one(crate::common::client_attached(1));
    host.cli()
        .invoke(&["say", "  one ", "two", "--", "-three"], b"");
    host.settle();
    let payload: Value =
        serde_json::from_str(&host.client_requests().sent()[0].payload_json).expect("payload");
    assert_eq!(
        payload["sentences"],
        serde_json::json!(["one", "two", "-three"])
    );
}

/// REQ: PORT:ilium/src/voice.rs::records_are_single_line_json_objects_with_a_type
#[test]
fn say_052_every_output_line_is_a_single_json_object_with_a_type() {
    let (records, _) = run(&["say", "quote \" and\nnewline"], b"");
    assert!(!records.is_empty());
    assert!(records.iter().all(|record| record.get("type").is_some()));
}

/// REQ: PORT:ilium/src/voice.rs::every_rejection_code_has_a_distinct_stable_name
#[test]
fn say_052_the_no_voice_client_and_invalid_request_codes_have_their_documented_names() {
    assert_eq!(last_code(&run(&["say", "hello"], b"").0), "no-voice-client");
    assert_eq!(last_code(&run(&["say"], b"").0), "invalid-request");
}
