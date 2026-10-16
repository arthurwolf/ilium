//! Twins of `ilium-server/src/voice_relay.rs` and `ilium-server/tests/voice_text_relay.rs`: the server side
//! offers a typed request to the attached clients and returns the first accepting answer.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ClientId, Event};
use serde_json::{Value, json};

use crate::common::{client_attached, jsonl, server_host};

fn error_line(code: &str, message: &str) -> String {
    json!({ "type": "error", "command": "voice say", "ok": false, "request_id": 1, "code": code, "message": message, "hint": null })
        .to_string()
}

fn result_line(started: bool) -> String {
    json!({ "type": "result", "command": "voice say", "ok": true, "request_id": 1, "session": "default",
        "sentence_count": 1, "accepted_sentences": 1, "voice_phase": "listening", "started_voice": started,
        "delivery": "queued-to-voice-session" })
    .to_string()
}

fn host_with_clients(clients: &[u64]) -> FakeHost {
    let mut host = server_host();
    for client in clients {
        host.deliver_one(client_attached(*client));
    }
    host
}

fn last_record(run: &ilium_plugin_sdk::testing::fakes::cli::Invocation) -> Value {
    jsonl(&run.stdout()).last().cloned().expect("a record")
}

/// REQ: PORT:ilium-server/src/voice_relay.rs::without_a_voice_client_the_request_is_rejected
#[test]
fn relay_052_without_a_voice_client_the_request_is_rejected_with_no_voice_client() {
    let mut host = host_with_clients(&[]);
    let run = host.cli().invoke(&["say", "hello"], b"");
    host.settle();
    assert_eq!(last_record(&run)["code"], "no-voice-client");
}

/// REQ: PORT:ilium-server/src/voice_relay.rs::a_running_session_receives_the_normalized_sentences
/// REQ: PORT:ilium-server/tests/voice_text_relay.rs::typed_sentences_are_offered_to_the_registered_voice_client_and_the_answer_returns
#[test]
fn relay_052_a_running_session_receives_the_trimmed_sentences_with_start_false() {
    let mut host = host_with_clients(&[1]);
    let run = host.cli().invoke(&["say", "  open the settings ", "close it"], b"");
    host.settle();
    let sent = host.client_requests().sent();
    let payload: Value = serde_json::from_str(&sent[0].payload_json).expect("payload");
    assert_eq!(payload["sentences"], json!(["open the settings", "close it"]));
    assert_eq!(payload["start"], false);
    host.client_requests().answer(sent[0].request.clone(), Ok(result_line(false)));
    host.settle();
    let record = last_record(&run);
    assert_eq!(record["ok"], true);
    assert_eq!(run.exit_code(), Some(0));
}

/// REQ: PORT:ilium-server/src/voice_relay.rs::a_client_with_voice_running_wins_over_a_newer_one_that_is_off
#[test]
fn relay_052_a_client_with_voice_running_wins_over_a_newer_one_that_is_off() {
    let mut host = host_with_clients(&[1, 2]);
    let run = host.cli().invoke(&["say", "hello"], b"");
    host.settle();
    let mut answered = 0;
    while let Some(offer) = host.client_requests().sent().get(answered).cloned() {
        let reply = if offer.client == 1 { result_line(false) } else { error_line("voice-off", "voice control is off") };
        host.client_requests().answer(offer.request.clone(), Ok(reply));
        host.settle();
        answered += 1;
        if run.exit_code().is_some() {
            break;
        }
    }
    assert_eq!(last_record(&run)["ok"], true);
    assert_eq!(run.exit_code(), Some(0));
}

/// REQ: PORT:ilium-server/src/voice_relay.rs::start_is_offered_only_when_asked_for_and_only_to_one_client
#[test]
fn relay_052_start_is_offered_only_after_the_plain_offers_were_declined() {
    let mut host = host_with_clients(&[1, 2]);
    let _run = host.cli().invoke(&["say", "--start", "hello"], b"");
    host.settle();
    let mut seen: Vec<(u64, bool)> = Vec::new();
    let mut index = 0;
    for _ in 0..8 {
        let Some(offer) = host.client_requests().sent().get(index).cloned() else { break };
        let payload: Value = serde_json::from_str(&offer.payload_json).expect("payload");
        seen.push((offer.client, payload["start"].as_bool().expect("start")));
        host.client_requests().answer(offer.request.clone(), Ok(error_line("voice-off", "voice control is off")));
        host.settle();
        index += 1;
    }
    let start_offers: Vec<_> = seen.iter().filter(|(_, start)| *start).collect();
    assert_eq!(start_offers.len(), 1, "{seen:?}");
    let first_start = seen.iter().position(|(_, start)| *start).expect("a start offer");
    assert!(seen[..first_start].iter().all(|(_, start)| !start));
    assert_eq!(start_offers[0].0, 2, "the newest client is tried first");
}

/// REQ: PORT:ilium-server/src/voice_relay.rs::a_failed_voice_session_is_reported_over_a_switched_off_one
/// REQ: PORT:ilium-server/tests/voice_text_relay.rs::typed_sentences_are_offered_to_the_registered_voice_client_and_the_answer_returns
#[test]
fn relay_052_voice_unavailable_outranks_voice_off_and_keeps_its_message() {
    let mut host = host_with_clients(&[1]);
    let run = host.cli().invoke(&["say", "--start", "hello"], b"");
    host.settle();
    for index in 0..2 {
        let offer = host.client_requests().sent()[index].clone();
        let reply = if index == 0 {
            error_line("voice-off", "voice control is off")
        } else {
            error_line("voice-unavailable", "OpenAI API key must not be empty")
        };
        host.client_requests().answer(offer.request.clone(), Ok(reply));
        host.settle();
    }
    let record = last_record(&run);
    assert_eq!(record["code"], "voice-unavailable");
    assert!(record["message"].as_str().expect("message").contains("API key"));
    assert_eq!(run.exit_code(), Some(1));
}

/// REQ: PORT:ilium-server/src/voice_relay.rs::a_silent_client_times_out_instead_of_hanging_the_request
#[test]
fn relay_052_a_silent_client_ends_in_client_unresponsive_and_a_late_answer_is_ignored() {
    let mut host = host_with_clients(&[1]);
    let run = host.cli().invoke(&["say", "hello"], b"");
    host.settle();
    let offer = host.client_requests().sent()[0].clone();
    host.advance_clock(5_001);
    host.settle();
    assert_eq!(last_record(&run)["code"], "client-unresponsive");
    host.client_requests().answer(offer.request.clone(), Ok(result_line(false)));
    host.settle();
    assert_eq!(last_record(&run)["code"], "client-unresponsive");
}

/// REQ: PORT:ilium-server/src/voice_relay.rs::disconnected_clients_are_forgotten
#[test]
fn relay_052_a_detached_client_is_forgotten_and_the_error_is_no_voice_client() {
    let mut host = host_with_clients(&[1]);
    host.deliver_one(Event::ClientDetached(ClientId(1)));
    let run = host.cli().invoke(&["say", "hello"], b"");
    host.settle();
    assert_eq!(last_record(&run)["code"], "no-voice-client");
}

/// REQ: PORT:ilium-server/src/voice_relay.rs::registering_the_same_connection_twice_keeps_one_entry
#[test]
fn relay_052_attaching_the_same_client_twice_sends_one_offer() {
    let mut host = host_with_clients(&[1, 1]);
    let _run = host.cli().invoke(&["say", "hello"], b"");
    host.settle();
    assert_eq!(host.client_requests().sent().len(), 1);
}

/// REQ: PORT:ilium-server/src/voice_relay.rs::malformed_requests_are_rejected_before_any_client_is_asked
/// REQ: PORT:ilium-server/tests/voice_text_relay.rs::a_malformed_request_is_refused_with_the_invalid_request_code
#[test]
fn relay_052_a_malformed_request_is_invalid_request_before_any_client_is_asked() {
    let mut host = host_with_clients(&[1]);
    let run = host.cli().invoke(&["say", "   "], b"");
    host.settle();
    assert_eq!(last_record(&run)["code"], "invalid-request");
    assert!(host.client_requests().sent().is_empty());
}

/// REQ: PORT:ilium-server/src/voice_relay.rs::a_request_id_cannot_be_reused_while_it_is_in_flight
#[test]
fn relay_052_two_concurrent_runs_get_distinct_request_ids() {
    let mut host = host_with_clients(&[1]);
    let first = host.cli().invoke(&["say", "one"], b"");
    let second = host.cli().invoke(&["say", "two"], b"");
    host.settle();
    let _ = (first, second);
    let sent = host.client_requests().sent();
    assert_eq!(sent.len(), 2);
    assert_ne!(sent[0].request, sent[1].request);
}
